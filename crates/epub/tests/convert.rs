//! Real text files converted to EPUBs and opened as books.
use std::path::Path;
use std::sync::Arc;

use mareader_epub::{Book, Destination, Format, convert};
use mareader_reflow::Typography;

const RAVEN: &[u8] = include_bytes!("books/the-raven-gutenberg-17192.txt");
const RIVER: &[u8] = include_bytes!("books/river.md");

fn books() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/books"))
}

/// Pictures beside the fixture, as the session reads them.
fn beside(path: &str) -> Option<Vec<u8>> {
    std::fs::read(books().join(path)).ok()
}

fn open(bytes: &[u8], format: Format, name: &str) -> Book {
    let epub = convert(bytes, format, Some(name), &beside).unwrap();
    Book::open(Arc::new(epub), &Typography::default()).unwrap()
}

fn lines(book: &Book) -> Vec<(String, bool)> {
    let pages = (0..book.page_count()).flat_map(|p| book.text(p));
    pages
        .map(|l| (l.chars.iter().map(|c| c.ch).collect(), l.wraps))
        .collect()
}

#[test]
fn the_raven_as_text_finds_its_headings_wrapped_prose_and_verse() {
    let book = open(RAVEN, Format::Plain, "the-raven-gutenberg-17192.txt");
    // The header's `Title:` line wins over the file name.
    assert_eq!(book.title(), Some("The Raven"));
    let toc: Vec<&str> = book.toc().iter().map(|e| e.title.as_str()).collect();
    for want in [
        "THE RAVEN",
        "COMMENT ON THE POEM.",
        "THE POEM.",
        "THE RAVEN.",
    ] {
        assert!(toc.contains(&want), "{want} in {toc:?}");
    }
    // Capitals after one blank line aren't headings: a title page, a signature.
    assert!(!toc.contains(&"NEW YORK"), "{toc:?}");
    assert!(!toc.contains(&"EDMUND C. STEDMAN."), "{toc:?}");
    let pages: Vec<usize> = book.toc().iter().map(|e| book.locate(e.at)).collect();
    assert!(pages.windows(2).all(|w| w[0] < w[1]), "{pages:?}");
    let lines = lines(&book);
    // The comment's prose was wrapped at 75 columns: it flows on.
    let at = lines
        .iter()
        .position(|(l, _)| l.starts_with("The secret of a poem"))
        .unwrap();
    assert!(
        lines[at + 1].1,
        "the next line wraps on: {:?}",
        lines[at + 1]
    );
    // Each verse line is its own, as printed.
    let at = lines
        .iter()
        .position(|(l, _)| l.starts_with("Once upon a midnight dreary"))
        .unwrap();
    assert!(
        lines[at].0.trim_end().ends_with("weak and weary,"),
        "{:?}",
        lines[at]
    );
    let next = &lines[at + 1];
    assert!(
        next.0.starts_with("Over many a quaint") && !next.1,
        "{next:?}"
    );
}

#[test]
fn the_river_notes_open_with_chapters_links_a_note_and_a_picture() {
    let book = open(RIVER, Format::Markdown, "river.md");
    assert_eq!(book.title(), Some("The River Notes"));
    let toc: Vec<(&str, usize)> = book
        .toc()
        .iter()
        .map(|e| (e.title.as_str(), e.depth))
        .collect();
    assert_eq!(
        toc,
        [("The River", 0), ("What came in", 1), ("The Jetty", 0)]
    );
    let page = |i: usize| book.locate(book.toc()[i].at);
    let (river, jetty) = (page(0), page(2));
    assert!(river < jetty);
    let pictures: usize = (0..book.page_count()).map(|p| book.pictures(p).len()).sum();
    assert_eq!(pictures, 1);
    let links =
        |p: usize| -> Vec<Destination> { book.links(p).into_iter().map(|l| l.to).collect() };
    let from_river: Vec<Destination> = (river..jetty).flat_map(links).collect();
    assert!(
        from_river.contains(&Destination::Page(jetty)),
        "{from_river:?}"
    );
    assert!(links(jetty).contains(&Destination::Page(river)));
    let text: String = lines(&book).into_iter().map(|(l, _)| l + "\n").collect();
    for want in [
        "\u{25a0} Unload the rice",
        "Note\nThe tide decides",
        "let boats = 12;",
        "Timber",
        "1 Before the sun is up",
    ] {
        assert!(text.contains(want), "{want:?} in {text}");
    }
}
