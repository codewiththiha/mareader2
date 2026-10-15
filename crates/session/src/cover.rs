//! A book's cover art and names for the library, read off the frame.
use std::path::Path;
use std::sync::Arc;

use mareader_pdf::Raster;

use crate::document::read_picture;

/// A PDF's first page or a book's picture, at most `side` long.
pub fn cover(path: &Path, side: u32) -> Option<Raster> {
    let bytes = std::fs::read(path).ok()?;
    let name = path.file_name()?.to_string_lossy();
    let bytes = match mareader_epub::Format::of_name(&name) {
        Some(format) => {
            let folder = path.parent();
            let read = |p: &str| folder.and_then(|f| read_picture(f, p));
            mareader_epub::convert(&bytes, format, Some(&name), &read).ok()?
        }
        None => bytes,
    };
    let bytes = Arc::new(bytes);
    if !mareader_epub::sniff(&bytes) {
        return first_page(bytes, side);
    }
    let (width, height, pixels) = mareader_epub::cover(&bytes)?.thumbnail(side)?;
    Some(Raster {
        width,
        height,
        pixels,
    })
}

/// A file's own title and author; text converts without its pictures.
pub fn describe(path: &Path) -> (Option<String>, Option<String>) {
    let read = || {
        let bytes = std::fs::read(path).ok()?;
        let name = path.file_name()?.to_string_lossy();
        let bytes = match mareader_epub::Format::of_name(&name) {
            Some(format) => mareader_epub::convert(&bytes, format, Some(&name), &|_| None).ok()?,
            None => bytes,
        };
        let bytes = Arc::new(bytes);
        if mareader_epub::sniff(&bytes) {
            return mareader_epub::describe(&bytes);
        }
        let doc = mareader_pdf::Document::open(bytes).ok()?;
        let own = |s: Option<&str>| s.map(str::to_owned);
        Some((own(doc.raw_title()), own(doc.raw_author())))
    };
    read().unwrap_or_default()
}

fn first_page(bytes: Arc<Vec<u8>>, side: u32) -> Option<Raster> {
    let doc = mareader_pdf::Document::open(bytes).ok()?;
    let size = *doc.page_sizes().first()?;
    // Pages are never zero-sized: `PageSize::sane` sees to it.
    let long = size.width.max(size.height);
    doc.render(0, side as f32 / long).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).ok();
        path
    }

    fn size(r: Option<Raster>) -> Option<(u32, u32)> {
        r.map(|r| (r.width, r.height))
    }

    #[test]
    fn a_file_names_its_title_and_author_without_being_laid_out() {
        let dir = tempfile::tempdir().ok();
        let dir = dir.as_ref().map_or(Path::new("."), |d| d.path());
        let book = write(dir, "b.epub", &mareader_epub::sample::epub(2, "Bee"));
        assert_eq!(describe(&book).0.as_deref(), Some("Bee"));
        let note = write(
            dir,
            "n.md",
            b"---\ntitle: Notes\nauthor: Me\n---\n\n# One\n\nText.\n",
        );
        let names = describe(&note);
        assert_eq!(
            (names.0.as_deref(), names.1.as_deref()),
            (Some("Notes"), Some("Me"))
        );
        assert_eq!(describe(&dir.join("gone.pdf")), (None, None));
    }

    #[test]
    fn a_book_shows_its_cover_shrunk_never_enlarged() {
        let dir = tempfile::tempdir().ok();
        let dir = dir.as_ref().map_or(Path::new("."), |d| d.path());
        let book = write(dir, "b.epub", &mareader_epub::sample::epub(2, "B"));
        assert_eq!(size(cover(&book, 512)), Some((341, 512)));
        assert_eq!(size(cover(&book, 2048)), Some((600, 900)));
    }

    #[test]
    fn a_pdf_shows_its_first_page() {
        let dir = tempfile::tempdir().ok();
        let dir = dir.as_ref().map_or(Path::new("."), |d| d.path());
        let pdf = write(dir, "p.pdf", &mareader_pdf::sample::pdf(3, "P"));
        let got = size(cover(&pdf, 512));
        assert!(
            got.is_some_and(|(w, h)| h.abs_diff(512) <= 1 && w < h),
            "{got:?}"
        );
    }

    #[test]
    fn markdown_shows_its_opening_picture_and_text_has_none() {
        let dir = tempfile::tempdir().ok();
        let dir = dir.as_ref().map_or(Path::new("."), |d| d.path());
        let png = mareader_epub::sample::cover_png().unwrap_or_default();
        write(dir, "c.png", &png);
        let md = write(dir, "a.md", b"![Cover](c.png)\n\n# One\n\nWords.\n");
        assert_eq!(size(cover(&md, 300)), Some((200, 300)));
        let plain = write(dir, "a.txt", b"Chapter 1\n\nJust words.\n");
        assert!(cover(&plain, 300).is_none());
        let deep = write(dir, "d.md", b"# One\n\nA\n\nB\n\nC\n\nD\n\n![x](c.png)\n");
        assert!(
            cover(&deep, 300).is_none(),
            "a picture deep in isn't a cover"
        );
        assert!(cover(&dir.join("gone.pdf"), 300).is_none());
        let junk = write(dir, "j.epub", b"not a book");
        assert!(cover(&junk, 300).is_none());
    }

    #[test]
    fn a_books_author_is_its_first_creator() {
        let bytes = Arc::new(mareader_epub::sample::epub(1, "B"));
        let typo = crate::Typography::default();
        let book = crate::Document::open(bytes, &typo).ok();
        let author = match &book {
            Some(crate::Document::Epub { book, .. }) => book.author(),
            _ => None,
        };
        assert_eq!(author, Some("mareader"));
    }
}
