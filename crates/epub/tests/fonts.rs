//! A book shipping its fonts packed and hidden every way readers meet them.
use std::sync::Arc;

use mareader_epub::Book;
use mareader_reflow::Typography;

const FONTS: &[u8] = include_bytes!("books/fonts.epub");

#[test]
fn every_shipped_font_loads_however_it_is_packed_or_hidden() {
    let book = Book::open(Arc::new(FONTS.to_vec()), &Typography::default()).unwrap();
    assert_eq!(book.title(), Some("Fonts and Figures"));
    let mut families: Vec<&str> = book.faces().iter().map(|f| f.family.as_str()).collect();
    families.sort_unstable();
    let named = [
        "hidden mono",
        "hidden sans",
        "packed mono",
        "packed sans",
        "second sans",
    ];
    assert_eq!(families.len(), named.len(), "{families:?}");
    for (family, name) in families.iter().zip(named) {
        assert!(family.ends_with(name), "{families:?}");
    }
    // Unpacked, each is a plain font, bigger than its packing.
    assert!(book.faces().iter().all(|f| f.len() > 20_000));
}

#[test]
fn an_unbroken_box_keeps_to_one_page() {
    let book = Book::open(Arc::new(FONTS.to_vec()), &Typography::default()).unwrap();
    let page_of = |words: &str| {
        (0..book.page_count()).find(|&p| {
            let text: String = book
                .text(p)
                .into_iter()
                .flat_map(|l| l.chars)
                .map(|c| c.ch)
                .collect();
            text.contains(words)
        })
    };
    let (top, foot) = (page_of("asks not"), page_of("next one"));
    assert!(top.is_some() && top == foot, "{top:?} {foot:?}");
}
