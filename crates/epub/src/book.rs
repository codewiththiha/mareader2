//! One opened EPUB: its chapters read, set and paginated up front.
use std::collections::HashMap;
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use mareader_reflow::{Anchor, Block, Content, Flow, Fonts, Picture, Pixels, Region, Typography};
use rbook::Epub;

use crate::css::{MAX_SHEET, Stylesheet};
use crate::xhtml::{self, Library};
use crate::{OpenError, guard};

/// All chapters' markup together; past it the book is refused.
pub const MAX_MARKUP: usize = 64 << 20;
/// Encoded image bytes kept for one book; past it, alt text shows.
pub const MAX_PICTURE_BYTES: usize = 256 << 20;

/// The file's bytes, shared with the session that read them.
struct Shared(Arc<Vec<u8>>);

impl AsRef<[u8]> for Shared {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// The archive's images and stylesheets, each read once per path.
struct Shelf<'e> {
    epub: &'e Epub,
    pictures: HashMap<String, Option<Arc<Picture>>>,
    sheets: HashMap<String, Option<Arc<Stylesheet>>>,
    budget: usize,
}

impl Library for Shelf<'_> {
    fn picture(&mut self, path: &str) -> Option<Arc<Picture>> {
        if let Some(known) = self.pictures.get(path) {
            return known.clone();
        }
        let bytes = self.epub.read_resource_bytes(path).ok();
        let bytes = bytes.filter(|b| b.len() <= self.budget);
        let picture = bytes.and_then(|b| {
            self.budget -= b.len();
            Picture::probe(b).ok().map(Arc::new)
        });
        self.pictures.insert(path.to_owned(), picture.clone());
        picture
    }

    fn stylesheet(&mut self, path: &str) -> Option<Arc<Stylesheet>> {
        if let Some(known) = self.sheets.get(path) {
            return known.clone();
        }
        let bytes = self.epub.read_resource_bytes(path).ok();
        let sheet = bytes.filter(|b| b.len() <= MAX_SHEET).map(|b| {
            let text = String::from_utf8_lossy(&b);
            Arc::new(Stylesheet::parse(&text))
        });
        self.sheets.insert(path.to_owned(), sheet.clone());
        sheet
    }
}

/// An open book. `Send + Sync`: workers share one through an `Arc`.
pub struct Book {
    /// What the chapters say, kept to set again in other type.
    blocks: Arc<Vec<Block>>,
    flow: Flow,
    title: Option<String>,
    /// The `dc:language` tag, for hyphenation.
    language: Option<String>,
}

impl std::fmt::Debug for Book {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Book({} pages)", self.flow.page_count())
    }
}

impl Book {
    /// Reads and lays out every chapter in `typo`; never panics.
    pub fn open(bytes: Arc<Vec<u8>>, typo: &Typography) -> Result<Self, OpenError> {
        let parse = || Self::parse(bytes, typo);
        catch_unwind(AssertUnwindSafe(parse)).unwrap_or(Err(OpenError::Invalid))
    }

    /// The same book set in `typo`; its pictures stay decoded.
    pub fn retypeset(&self, typo: &Typography) -> Option<Self> {
        let set = || {
            let flow = set(&self.blocks, typo, self.language.as_deref());
            Self {
                blocks: Arc::clone(&self.blocks),
                flow: flow.keep_pictures(&self.flow),
                title: self.title.clone(),
                language: self.language.clone(),
            }
        };
        catch_unwind(AssertUnwindSafe(set)).ok()
    }

    fn parse(bytes: Arc<Vec<u8>>, typo: &Typography) -> Result<Self, OpenError> {
        guard::check(&bytes)?;
        let epub = Epub::read(Cursor::new(Shared(bytes))).map_err(|_| OpenError::Invalid)?;
        let title = epub
            .metadata()
            .title()
            .map(|t| t.value().trim().to_owned())
            .filter(|t| !t.is_empty());
        let language = epub
            .metadata()
            .language()
            .map(|l| l.value().trim().to_owned())
            .filter(|l| !l.is_empty());
        let mut shelf = Shelf {
            epub: &epub,
            pictures: HashMap::new(),
            sheets: HashMap::new(),
            budget: MAX_PICTURE_BYTES,
        };
        let mut blocks = Vec::new();
        let mut markup = 0;
        // The first chapter with content, where a cover may already be.
        let mut opening = None;
        // A chapter that won't read is skipped, as a reader would.
        for chapter in epub.reader().flatten() {
            markup += chapter.content().len();
            if markup > MAX_MARKUP {
                return Err(OpenError::TooLarge);
            }
            let path = chapter.manifest_entry().href().decode();
            let start = blocks.len();
            xhtml::read_chapter(chapter.content(), &path, &mut shelf, &mut blocks);
            if opening.is_none() && blocks.len() > start {
                opening = Some(start..blocks.len());
            }
        }
        let cover = epub.manifest().cover_image().map(|e| e.href().decode());
        let cover = cover.and_then(|path| shelf.picture(&path));
        if let Some(cover) = cover {
            place_cover(&mut blocks, opening.unwrap_or(0..0), cover);
        }
        if blocks.is_empty() {
            return Err(OpenError::Empty);
        }
        let flow = set(&blocks, typo, language.as_deref());
        Ok(Self {
            blocks: Arc::new(blocks),
            flow,
            title,
            language,
        })
    }

    pub fn page_count(&self) -> usize {
        self.flow.page_count()
    }

    /// Every page's width and height, in layout units (CSS px).
    pub fn page_size(&self) -> (f32, f32) {
        self.flow.page_size()
    }

    /// The `dc:title` as written, if any.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Where page `page` starts, to find again after re-setting.
    pub fn anchor(&self, page: usize) -> Option<Anchor> {
        self.flow.anchor(page)
    }

    /// The page showing `anchor`, or the first after it.
    pub fn locate(&self, anchor: Anchor) -> usize {
        self.flow.locate(anchor)
    }

    /// What of each page a stream shows.
    pub fn bands(&self) -> &[mareader_reflow::Band] {
        self.flow.bands()
    }

    /// `region` of page `page` at `scale` pixels per unit.
    pub fn render(&self, page: usize, scale: f32, region: Region) -> Option<Pixels> {
        self.flow.render(page, scale, region)
    }
}

/// Lays the blocks out; the book's language beats the default.
fn set(blocks: &[Block], typo: &Typography, language: Option<&str>) -> Flow {
    let mut fonts = Fonts::system();
    match language {
        Some(tag) if *tag != typo.language => {
            let typo = Typography {
                language: tag.to_owned(),
                ..typo.clone()
            };
            Flow::new(blocks, &typo, &mut fonts)
        }
        _ => Flow::new(blocks, typo, &mut fonts),
    }
}

/// The opening chapter's own cover image fills the page; else one is added.
fn place_cover(blocks: &mut Vec<Block>, opening: std::ops::Range<usize>, cover: Arc<Picture>) {
    let shown = blocks.get_mut(opening).into_iter().flatten().find(
        |b| matches!(&b.content, Content::Image { picture, .. } if Arc::ptr_eq(picture, &cover)),
    );
    if let Some(block) = shown {
        block.content = Content::Image {
            picture: cover,
            cover: true,
        };
        return;
    }
    let mut block = Block::new(Content::Image {
        picture: cover,
        cover: true,
    });
    block.break_before = true;
    blocks.insert(0, block);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;

    #[test]
    fn the_sample_opens_with_its_title_and_chapters() {
        let book = Book::open(Arc::new(sample::epub(3, "Sample")), &Typography::default());
        let book = book.as_ref().ok();
        assert_eq!(book.and_then(Book::title), Some("Sample"));
        // A cover, then chapters that each start a page and fill more.
        assert!(book.is_some_and(|b| b.page_count() >= 7), "{book:?}");
    }

    #[test]
    fn junk_is_refused() {
        assert_eq!(
            Book::open(Arc::new(b"not a zip".to_vec()), &Typography::default()).err(),
            Some(OpenError::Invalid)
        );
    }

    #[test]
    fn a_page_draws() {
        let book = Book::open(Arc::new(sample::epub(1, "S")), &Typography::default()).ok();
        let px = book.and_then(|b| b.render(1, 0.25, [0, 0, 198, 280]));
        assert_eq!(px.map(|p| p.rgba.len()), Some(198 * 280 * 4));
    }

    #[test]
    fn the_cover_comes_first_and_draws_in_colour() {
        let book = Book::open(Arc::new(sample::epub(1, "S")), &Typography::default()).ok();
        let px = book.and_then(|b| b.render(0, 0.25, [0, 0, 198, 280]));
        let (pixels, _) = px
            .as_ref()
            .map_or((&[][..], &[][..]), |p| p.rgba.as_chunks::<4>());
        // The sample cover is a dusk sky: plenty of saturated pixels.
        let colourful = pixels
            .iter()
            .filter(|[r, g, b, _]| r.max(g).max(b) - r.min(g).min(b) > 60)
            .count();
        assert!(colourful > 5_000, "{colourful}");
    }

    #[test]
    fn a_retypeset_book_keeps_its_place_in_bigger_type() {
        let book = Book::open(Arc::new(sample::epub(4, "S")), &Typography::default()).ok();
        let big = Typography {
            font_size: 26.0,
            ..Typography::default()
        };
        let bigger = book.as_ref().and_then(|b| b.retypeset(&big));
        assert!(
            book.is_some() && bigger.is_some(),
            "sample opens and re-sets"
        );
        let (Some(book), Some(bigger)) = (book, bigger) else {
            return;
        };
        assert!(bigger.page_count() > book.page_count());
        assert_eq!(bigger.title(), book.title());
        let page = book.page_count() / 2;
        let anchor = book.anchor(page);
        let found = anchor.map(|a| bigger.locate(a));
        let start = found.and_then(|p| bigger.anchor(p));
        assert!(start.is_some() && start <= anchor, "{start:?} {anchor:?}");
    }

    #[test]
    fn a_chapter_showing_the_cover_becomes_the_cover_page() {
        let Some(cover) = sample::cover_png().and_then(|b| Picture::probe(b).ok()) else {
            return assert!(sample::cover_png().is_some(), "cover encodes");
        };
        let cover = Arc::new(cover);
        let mut blocks = vec![
            Block::new(Content::Image {
                picture: cover.clone(),
                cover: false,
            }),
            Block::text(mareader_reflow::Role::Paragraph, "text"),
        ];
        place_cover(&mut blocks, 0..2, cover.clone());
        assert_eq!(blocks.len(), 2);
        let first = blocks.first().map(|b| &b.content);
        assert!(matches!(first, Some(Content::Image { cover: true, .. })));
        // Elsewhere, it's added in front on a page of its own.
        let mut blocks = vec![Block::text(mareader_reflow::Role::Paragraph, "text")];
        place_cover(&mut blocks, 0..1, cover);
        assert_eq!(blocks.len(), 2);
        assert!(blocks.first().is_some_and(|b| b.break_before));
    }
}
