//! One opened EPUB: its chapters read, set and paginated up front.
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use mareader_reflow::{
    Anchor, Block, Content, Flow, Fonts, PageLine, Picture, Pixels, Region, Role, Typography,
};
use rbook::Epub;
use rbook::epub::manifest::EpubManifestEntry;

use crate::css::{MAX_SHEET, Stylesheet};
use crate::xhtml::{self, Href, Library, Refs};
use crate::{OpenError, guard};

/// All chapters' markup together; past it the book is refused.
pub const MAX_MARKUP: usize = 64 << 20;
/// Blocks in one book; each keeps its line boxes while open.
pub const MAX_BLOCKS: usize = 1 << 20;
/// Encoded image bytes kept for one book; past it, alt text shows.
pub const MAX_PICTURE_BYTES: usize = 256 << 20;

/// Table of contents entries kept; a longer one is cut.
pub const MAX_TOC: usize = 20_000;
/// An entry's title is cut to this many characters.
const MAX_TITLE: usize = 200;

/// A table of contents entry, or a heading when the book lists none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub title: String,
    /// 0 for the top level.
    pub depth: usize,
    pub at: Anchor,
}

/// Where a link leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Page(usize),
    /// A footnote: shown where its mark is, its page to go to.
    Note {
        page: usize,
        text: String,
    },
    /// An address outside the book, its scheme not yet checked.
    Outside(String),
}

/// A link's box on one line of a page, in page units.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    /// Left, top, width, height.
    pub rect: [f32; 4],
    pub to: Destination,
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
    refs: Arc<Refs>,
    /// What each note link shows, by its target.
    notes: Arc<HashMap<u32, String>>,
    toc: Arc<Vec<TocEntry>>,
    title: Option<String>,
    author: Option<String>,
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
                refs: Arc::clone(&self.refs),
                notes: Arc::clone(&self.notes),
                toc: Arc::clone(&self.toc),
                title: self.title.clone(),
                author: self.author.clone(),
                language: self.language.clone(),
            }
        };
        catch_unwind(AssertUnwindSafe(set)).ok()
    }

    fn parse(bytes: Arc<Vec<u8>>, typo: &Typography) -> Result<Self, OpenError> {
        let epub = guard::open(&bytes)?;
        let (title, author) = names(&epub);
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
        let mut refs = Refs::default();
        let mut markup = 0;
        // The first chapter with content, where a cover may already be.
        let mut opening = None;
        // A chapter that won't read is skipped, as a reader would.
        for entry in epub.spine().iter().filter_map(|s| s.manifest_entry()) {
            let Some(content) = chapter_text(&entry) else {
                continue;
            };
            markup += content.len();
            if markup > MAX_MARKUP {
                return Err(OpenError::TooLarge);
            }
            let path = entry.href().decode();
            let start = blocks.len();
            let out = (&mut blocks, &mut refs);
            xhtml::read_chapter(&content, &path, &mut shelf, out);
            if blocks.len() > MAX_BLOCKS {
                return Err(OpenError::TooLarge);
            }
            if opening.is_none() && blocks.len() > start {
                opening = Some(start..blocks.len());
            }
        }
        let cover = epub.manifest().cover_image().map(|e| e.href().decode());
        let cover = cover.and_then(|path| shelf.picture(&path));
        if let Some(cover) = cover
            && place_cover(&mut blocks, opening.unwrap_or(0..0), cover)
        {
            for at in refs.places.values_mut() {
                at.block += 1;
            }
            for e in refs.extents.values_mut() {
                e.blocks = e.blocks.start + 1..e.blocks.end + 1;
            }
        }
        if blocks.is_empty() {
            return Err(OpenError::Empty);
        }
        let blocks = Arc::new(blocks);
        let flow = set(&blocks, typo, language.as_deref());
        let toc = contents(&epub, &refs, &blocks);
        let notes = crate::notes::find(&blocks, &refs);
        Ok(Self {
            blocks,
            flow,
            refs: Arc::new(refs),
            notes: Arc::new(notes),
            toc: Arc::new(toc),
            title,
            author,
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

    /// The first `dc:creator` that isn't blank.
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// Where page `page` starts, to find again after re-setting.
    pub fn anchor(&self, page: usize) -> Option<Anchor> {
        self.flow.anchor(page)
    }

    /// The page showing `anchor`, or the first after it.
    pub fn locate(&self, anchor: Anchor) -> usize {
        self.flow.locate(anchor)
    }

    /// The book's table of contents, in reading order.
    pub fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    /// Page `page`'s links that lead somewhere: a box per line.
    pub fn links(&self, page: usize) -> Vec<Link> {
        let lead = |target: u32| {
            let href = self.refs.hrefs.get(usize::try_from(target).ok()?)?;
            Some(match href {
                Href::Inside(key) => {
                    let page = self.locate(self.refs.place(key)?);
                    match self.notes.get(&target) {
                        Some(text) => Destination::Note {
                            page,
                            text: text.clone(),
                        },
                        None => Destination::Page(page),
                    }
                }
                Href::Outside(address) => Destination::Outside(address.clone()),
            })
        };
        let links = self.flow.links(page).into_iter();
        links
            .filter_map(|l| {
                Some(Link {
                    rect: l.rect,
                    to: lead(l.target)?,
                })
            })
            .collect()
    }

    /// Page `page`'s text by line, each character boxed.
    pub fn text(&self, page: usize) -> Vec<PageLine> {
        self.flow.text(page)
    }

    /// Page `page`'s pictures, which keep their colours under the ink dial.
    pub fn pictures(&self, page: usize) -> Vec<[f32; 4]> {
        self.flow.pictures(page)
    }

    /// The picture drawn at a point of page `page`, in page units.
    pub fn picture_at(&self, page: usize, x: f32, y: f32) -> Option<Arc<Picture>> {
        self.flow.picture_at(page, x, y)
    }

    /// Page `page`'s colour glyphs, which keep their colours too.
    pub fn emojis(&self, page: usize) -> Vec<[f32; 4]> {
        self.flow.emojis(page)
    }

    /// What of each page a stream shows.
    pub fn bands(&self) -> &[mareader_reflow::Band] {
        self.flow.bands()
    }

    /// `region` of page `page` at `scale` pixels per unit.
    pub fn render(&self, page: usize, scale: f32, region: Region) -> Option<Pixels> {
        self.flow.render(page, scale, region)
    }

    /// The blocks the book is made of, in reading order.
    pub fn blocks(&self) -> &Arc<Vec<Block>> {
        &self.blocks
    }

    /// What page `page` shows of each block; see [`Flow::excerpts`].
    pub fn excerpts(&self, page: usize) -> Vec<(usize, std::ops::Range<usize>)> {
        self.flow.excerpts(page)
    }

    /// The book's language tag, if it names one.
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    /// Shares the book's decoded pictures with `flow`, set from its blocks.
    pub fn with_pictures(&self, flow: Flow) -> Flow {
        flow.keep_pictures(&self.flow)
    }

    /// Page `page`'s column: its left edge and width, in layout units.
    pub fn column(&self, page: usize) -> (f32, f32) {
        self.flow.column(page)
    }
}

/// The `dc:title` and first non-blank `dc:creator`, as written.
fn names(epub: &Epub) -> (Option<String>, Option<String>) {
    let title = epub
        .metadata()
        .title()
        .map(|t| t.value().trim().to_owned())
        .filter(|t| !t.is_empty());
    let author = epub
        .metadata()
        .creators()
        .map(|c| c.value().trim().to_owned())
        .find(|c| !c.is_empty());
    (title, author)
}

/// A book's title and author without laying it out, for the library.
pub fn describe(bytes: &Arc<Vec<u8>>) -> Option<(Option<String>, Option<String>)> {
    let read = || guard::open(bytes).ok().map(|epub| names(&epub));
    catch_unwind(AssertUnwindSafe(read)).ok().flatten()
}

/// A picture this near the opening counts as the cover.
const COVER_BLOCKS: usize = 4;

/// The declared cover, else a picture opening the first chapter; sets no type.
pub fn cover(bytes: &Arc<Vec<u8>>) -> Option<Arc<Picture>> {
    let find = || {
        let epub = guard::open(bytes).ok()?;
        let mut shelf = Shelf {
            epub: &epub,
            pictures: HashMap::new(),
            sheets: HashMap::new(),
            budget: MAX_PICTURE_BYTES,
        };
        let declared = epub.manifest().cover_image().map(|e| e.href().decode());
        if let Some(picture) = declared.and_then(|path| shelf.picture(&path)) {
            return Some(picture);
        }
        let (mut blocks, mut refs) = (Vec::new(), Refs::default());
        for entry in epub.spine().iter().filter_map(|s| s.manifest_entry()) {
            let Some(content) = chapter_text(&entry).filter(|c| c.len() <= MAX_MARKUP) else {
                continue;
            };
            let path = entry.href().decode();
            xhtml::read_chapter(&content, &path, &mut shelf, (&mut blocks, &mut refs));
            if !blocks.is_empty() {
                break;
            }
        }
        blocks
            .iter()
            .take(COVER_BLOCKS)
            .find_map(|b| match &b.content {
                Content::Image { picture, .. } => Some(Arc::clone(picture)),
                _ => None,
            })
    };
    catch_unwind(AssertUnwindSafe(find)).ok().flatten()
}

/// A chapter's markup; stray bytes that aren't UTF-8 become U+FFFD.
fn chapter_text(entry: &EpubManifestEntry) -> Option<String> {
    let bytes = || entry.read_bytes().ok();
    let lossy = |b: Vec<u8>| String::from_utf8_lossy(&b).into_owned();
    entry.read_str().ok().or_else(|| bytes().map(lossy))
}

/// Lays the blocks out; the book's language beats the default.
fn set(blocks: &Arc<Vec<Block>>, typo: &Typography, language: Option<&str>) -> Flow {
    let (blocks, fonts) = (Arc::clone(blocks), Fonts::system());
    match language {
        Some(tag) if *tag != typo.language => {
            let typo = Typography {
                language: tag.to_owned(),
                ..typo.clone()
            };
            Flow::new(blocks, &typo, fonts)
        }
        _ => Flow::new(blocks, typo, fonts),
    }
}

/// The listed contents whose places are known; else the headings.
fn contents(epub: &Epub, refs: &Refs, blocks: &[Block]) -> Vec<TocEntry> {
    let root = epub.toc().contents();
    let listed: Vec<TocEntry> = root
        .iter()
        .flat_map(|root| root.flatten())
        .take(MAX_TOC)
        .filter_map(|e| {
            Some(TocEntry {
                title: one_line(e.label())?,
                depth: e.depth().saturating_sub(1),
                at: refs.place(&e.href()?.decode())?,
            })
        })
        .collect();
    if !listed.is_empty() {
        return listed;
    }
    let headings = blocks.iter().enumerate().filter_map(|(block, b)| {
        let Content::Text(text) = &b.content else {
            return None;
        };
        let Role::Heading(level) = text.role else {
            return None;
        };
        Some(TocEntry {
            title: one_line(&text.text)?,
            depth: usize::from(level.saturating_sub(1)),
            at: Anchor { block, offset: 0 },
        })
    });
    headings.take(MAX_TOC).collect()
}

/// `s` on one line and cut short; `None` if blank.
fn one_line(s: &str) -> Option<String> {
    let words: Vec<&str> = s.split_whitespace().collect();
    let line: String = words.join(" ").chars().take(MAX_TITLE).collect();
    (!line.is_empty()).then_some(line)
}

/// Makes the cover fill a page; true if it had to be added first.
fn place_cover(
    blocks: &mut Vec<Block>,
    opening: std::ops::Range<usize>,
    cover: Arc<Picture>,
) -> bool {
    let shown = blocks.get_mut(opening).into_iter().flatten().find(
        |b| matches!(&b.content, Content::Image { picture, .. } if Arc::ptr_eq(picture, &cover)),
    );
    if let Some(block) = shown {
        block.content = Content::Image {
            picture: cover,
            cover: true,
        };
        return false;
    }
    let mut block = Block::new(Content::Image {
        picture: cover,
        cover: true,
    });
    block.break_before = true;
    blocks.insert(0, block);
    true
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

    fn open(bytes: Vec<u8>, typo: &Typography) -> Option<Book> {
        Book::open(Arc::new(bytes), typo).ok()
    }

    fn page_of(book: &Book, title: &str) -> Option<usize> {
        let entry = book.toc().iter().find(|e| e.title == title);
        entry.map(|e| book.locate(e.at))
    }

    #[test]
    fn the_contents_list_chapters_and_parts_where_they_are() {
        let book = open(sample::epub(3, "S"), &Typography::default());
        let toc = book.as_ref().map_or(&[][..], Book::toc);
        let listed: Vec<(&str, usize)> = toc.iter().map(|e| (e.title.as_str(), e.depth)).collect();
        assert_eq!(
            listed,
            [
                ("Chapter 1", 0),
                ("Part 1.2", 1),
                ("Chapter 2", 0),
                ("Part 2.2", 1),
                ("Chapter 3", 0),
                ("Part 3.2", 1)
            ]
        );
        // The cover takes page 0; no entry is before the one above it.
        let pages: Vec<usize> = toc
            .iter()
            .filter_map(|e| Some(book.as_ref()?.locate(e.at)))
            .collect();
        assert_eq!(pages.first(), Some(&1));
        assert!(pages.windows(2).all(|w| w.first() <= w.get(1)), "{pages:?}");
        let chapters: Vec<usize> = pages.iter().step_by(2).copied().collect();
        assert!(
            chapters.windows(2).all(|w| w.first() < w.get(1)),
            "{pages:?}"
        );
        // Each part's place is its heading, whatever the type size.
        let big = Typography {
            font_size: 30.0,
            ..Typography::default()
        };
        for typo in [Typography::default(), big] {
            let book = open(sample::epub(3, "S"), &typo);
            for part in ["Part 1.2", "Part 2.2"] {
                let at = book
                    .as_ref()
                    .and_then(|b| b.toc().iter().find(|e| e.title == part));
                let at = at.map(|e| e.at);
                let block = at.and_then(|a| book.as_ref()?.blocks.get(a.block));
                let text = match block.map(|b| &b.content) {
                    Some(Content::Text(t)) => Some(t.text.as_str()),
                    _ => None,
                };
                assert_eq!(text, Some(part));
            }
        }
    }

    #[test]
    fn links_lead_to_their_pages_or_out_of_the_book() {
        let book = open(sample::epub(2, "S"), &Typography::default());
        assert!(book.is_some(), "open");
        let Some(book) = book else {
            return;
        };
        let links: Vec<Link> = (0..book.page_count()).flat_map(|p| book.links(p)).collect();
        let to = |page: Option<usize>| page.map(Destination::Page);
        let leads = |d: Option<Destination>| links.iter().any(|l| Some(&l.to) == d.as_ref());
        assert!(leads(to(page_of(&book, "Chapter 2"))), "{links:?}");
        assert!(leads(to(page_of(&book, "Part 1.2"))), "{links:?}");
        let web = "https://github.com/codewiththiha/mareader2".to_owned();
        assert!(leads(Some(Destination::Outside(web))), "{links:?}");
        // The boxes sit on the page, a line high.
        let (w, h) = book.page_size();
        for l in &links {
            let [x, y, lw, lh] = l.rect;
            assert!(x >= 0.0 && y >= 0.0 && x + lw <= w && y + lh <= h, "{l:?}");
            assert!(lh > 10.0 && lh < 60.0, "{l:?}");
        }
    }

    #[test]
    fn a_book_without_contents_lists_its_headings() {
        let body = "<h1>One</h1><p>x</p><h2>Two  <em>parts</em></h2><p>y</p><h4>Deep</h4>";
        let chapter = rbook::epub::EpubChapter::unlisted("a.xhtml").xhtml_body(body);
        let bytes = Epub::builder().title("T").chapter(chapter).write().to_vec();
        let book = bytes.ok().and_then(|b| open(b, &Typography::default()));
        let toc = book.as_ref().map_or(&[][..], Book::toc);
        let listed: Vec<(&str, usize, usize)> = toc
            .iter()
            .map(|e| (e.title.as_str(), e.depth, e.at.block))
            .collect();
        assert_eq!(listed, [("One", 0, 0), ("Two parts", 1, 2), ("Deep", 3, 4)]);
    }
}
