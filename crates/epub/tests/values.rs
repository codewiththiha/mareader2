//! A book written to show style sheets that work things out.
use std::sync::Arc;

use mareader_epub::Book;
use mareader_reflow::Typography;

const VALUES: &[u8] = include_bytes!("books/values.epub");

#[test]
fn the_values_book_shows_only_what_its_tests_allow() {
    let book = Book::open(Arc::new(VALUES.to_vec()), &Typography::default()).unwrap();
    assert_eq!(book.title(), Some("Values and Queries"));
    let text: String = (0..book.page_count())
        .flat_map(|p| book.text(p))
        .flat_map(|l| l.chars.into_iter().map(|c| c.ch))
        .collect();
    // Sheets behind print, huge widths and alternate titles stay out.
    for shown in ["Sums", "newer", "whatever", "vendor", "italics", "initial"] {
        assert!(text.contains(shown), "{shown}: {text}");
    }
    // Every line saying "should not" is a fallback a passing test hides.
    assert!(!text.contains("should"), "{text}");
}
