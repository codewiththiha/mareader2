//! One opened EPUB: its chapters read, set and paginated up front.
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use mareader_reflow::{Flow, Fonts, Pixels, Region, Typography};
use rbook::Epub;

use crate::{OpenError, guard, xhtml};

/// All chapters' markup together; past it the book is refused.
pub const MAX_MARKUP: usize = 64 << 20;

/// The file's bytes, shared with the session that read them.
struct Shared(Arc<Vec<u8>>);

impl AsRef<[u8]> for Shared {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// An open book. `Send + Sync`: workers share one through an `Arc`.
pub struct Book {
    flow: Flow,
    title: Option<String>,
}

impl std::fmt::Debug for Book {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Book({} pages)", self.flow.page_count())
    }
}

impl Book {
    /// Reads and lays out every chapter; never panics.
    pub fn open(bytes: Arc<Vec<u8>>) -> Result<Self, OpenError> {
        catch_unwind(AssertUnwindSafe(|| Self::parse(bytes))).unwrap_or(Err(OpenError::Invalid))
    }

    fn parse(bytes: Arc<Vec<u8>>) -> Result<Self, OpenError> {
        guard::check(&bytes)?;
        let epub = Epub::read(Cursor::new(Shared(bytes))).map_err(|_| OpenError::Invalid)?;
        let title = epub
            .metadata()
            .title()
            .map(|t| t.value().trim().to_owned())
            .filter(|t| !t.is_empty());
        let mut blocks = Vec::new();
        let mut markup = 0;
        // A chapter that won't read is skipped, as a reader would.
        for chapter in epub.reader().flatten() {
            markup += chapter.content().len();
            if markup > MAX_MARKUP {
                return Err(OpenError::TooLarge);
            }
            xhtml::read_chapter(chapter.content(), &mut blocks);
        }
        if blocks.is_empty() {
            return Err(OpenError::Empty);
        }
        let flow = Flow::new(&blocks, &Typography::default(), &mut Fonts::bundled());
        Ok(Self { flow, title })
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

    /// `region` of page `page` at `scale` pixels per unit.
    pub fn render(&self, page: usize, scale: f32, region: Region) -> Option<Pixels> {
        self.flow.render(page, scale, region)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;

    #[test]
    fn the_sample_opens_with_its_title_and_chapters() {
        let book = Book::open(Arc::new(sample::epub(3, "Sample")));
        let book = book.as_ref().ok();
        assert_eq!(book.and_then(Book::title), Some("Sample"));
        // Each chapter starts a page and fills more than one.
        assert!(book.is_some_and(|b| b.page_count() >= 6), "{book:?}");
    }

    #[test]
    fn junk_is_refused() {
        assert_eq!(
            Book::open(Arc::new(b"not a zip".to_vec())).err(),
            Some(OpenError::Invalid)
        );
    }

    #[test]
    fn a_page_draws() {
        let book = Book::open(Arc::new(sample::epub(1, "S"))).ok();
        let px = book.and_then(|b| b.render(0, 0.25, [0, 0, 198, 280]));
        assert_eq!(px.map(|p| p.rgba.len()), Some(198 * 280 * 4));
    }
}
