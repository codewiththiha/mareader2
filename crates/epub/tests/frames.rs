//! A book written to show what CSS draws around boxes.
use std::sync::Arc;

use mareader_epub::Book;
use mareader_reflow::Typography;

const FRAMES: &[u8] = include_bytes!("books/frames.epub");

#[test]
fn the_frames_book_opens_with_its_hidden_words_still_in_the_text() {
    let book = Book::open(Arc::new(FRAMES.to_vec()), &Typography::default()).unwrap();
    assert_eq!(book.title(), Some("Frames and Shades"));
    let text: String = (0..book.page_count())
        .flat_map(|p| book.text(p))
        .flat_map(|l| l.chars.into_iter().map(|c| c.ch))
        .collect();
    assert!(text.contains("A pictured frame"), "{text}");
    // Hidden words take their room and are still found.
    assert!(text.matches("hidden").count() >= 2, "{text}");
}
