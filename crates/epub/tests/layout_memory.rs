//! Measures what set books hold, for `LAYOUT_BYTES` and `CHAR_BYTES`.
//! Linux only; run with `--ignored --nocapture`.
use std::sync::Arc;
use std::time::Instant;

use mareader_epub::Book;
use mareader_reflow::Typography;
use rbook::Epub;
use rbook::epub::EpubChapter;

/// A `/proc/self/status` figure in bytes.
fn status(key: &str) -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let line = s.lines().find(|l| l.starts_with(key)).unwrap_or_default();
    let kb = line.split_whitespace().nth(1).unwrap_or("0");
    kb.parse::<u64>().unwrap_or(0) * 1024
}

fn book(body: &str) -> Vec<u8> {
    let xhtml = format!(
        "<?xml version=\"1.0\"?><html xmlns=\"http://www.w3.org/1999/xhtml\">\
         <head><title>c</title></head><body>{body}</body></html>"
    );
    let ch = EpubChapter::new("c").href("c.xhtml").xhtml(xhtml);
    let book = Epub::builder().title("t").chapter(ch);
    book.write().to_vec().unwrap_or_default()
}

/// Opens `body` and prints the resident growth per `units`.
fn measure(name: &str, body: &str, units: u64) {
    let bytes = Arc::new(book(body));
    let before = status("VmRSS:");
    let started = Instant::now();
    let opened = Book::open(bytes, &Typography::default());
    let secs = started.elapsed().as_secs_f32();
    let grew = status("VmRSS:").saturating_sub(before);
    let pages = opened.as_ref().map_or(0, Book::page_count);
    println!(
        "{name}: {units} units, {secs:.2} s, {pages} pages, +{} MB, {} B a unit",
        grew >> 20,
        grew / units
    );
}

#[test]
#[ignore = "a measurement, not a check"]
fn set_books_hold_about_this_much() {
    // A fresh process each run keeps the allocator's reuse out of it.
    let paragraph = format!("<p>{}</p>", "abcdefghi ".repeat(100));
    measure("characters", &paragraph.repeat(4000), 4_000_000);
    measure("paragraphs", &"<p>w</p>".repeat(200_000), 200_000);
}
