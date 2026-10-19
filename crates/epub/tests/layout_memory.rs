//! What open books hold, then read through.
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

/// Opens `body`, reads every page, and tells resident growth per `units`.
fn measure(name: &str, body: &str, units: u64) -> String {
    let bytes = Arc::new(book(body));
    let before = status("VmRSS:");
    let started = Instant::now();
    let opened = Book::open(bytes, &Typography::default());
    let secs = started.elapsed().as_secs_f32();
    let open = status("VmRSS:").saturating_sub(before);
    let pages = opened.as_ref().map_or(0, Book::page_count);
    // Every page's text lays every block again, through the kept few.
    let started = Instant::now();
    let lines: usize = opened
        .iter()
        .flat_map(|b| (0..pages).map(|p| b.text(p).len()))
        .sum();
    let read = started.elapsed().as_secs_f32();
    let after = status("VmRSS:").saturating_sub(before);
    format!(
        "{name}: {units} units, {pages} pages; open {secs:.2} s, +{} MB, {} B a unit; \
         read {read:.2} s ({lines} lines), +{} MB; peak {} MB",
        open >> 20,
        open / units,
        after >> 20,
        status("VmHWM:") >> 20,
    )
}

#[test]
#[ignore = "a measurement, not a check"]
fn set_books_hold_about_this_much() {
    // A fresh process each run keeps the allocator's reuse out of it.
    let paragraph = format!("<p>{}</p>", "abcdefghi ".repeat(100));
    println!(
        "{}",
        measure("characters", &paragraph.repeat(4000), 4_000_000)
    );
    println!(
        "{}",
        measure("paragraphs", &"<p>w</p>".repeat(200_000), 200_000)
    );
}

/// Draws every page at Retina scales on four threads, round after round.
fn render_rounds(name: &str, markdown: &str) {
    let format = mareader_epub::Format::Markdown;
    let none = |_: &str| None;
    let epub = mareader_epub::convert(markdown.as_bytes(), format, Some("a.md"), &none);
    let opened = epub
        .ok()
        .and_then(|e| Book::open(Arc::new(e), &Typography::default()).ok());
    let Some(book) = opened else {
        return println!("{name}: did not open");
    };
    let (pages, start) = (book.page_count(), status("VmRSS:"));
    let mut line = format!(
        "{name}: {pages} pages, opened at {} MB; rounds:",
        start >> 20
    );
    for round in 0..12 {
        let scale = [2.0, 3.0, 2.5][round % 3];
        std::thread::scope(|s| {
            for lane in 0..4 {
                let book = &book;
                s.spawn(move || {
                    for page in (lane..pages).step_by(4) {
                        let (w, h) = (794.0 * scale, 1123.0 * scale);
                        book.render(page, scale, [0, 0, w as u32, h as u32]);
                    }
                });
            }
        });
        line.push_str(&format!(" {}", status("VmRSS:") >> 20));
    }
    println!("{line} MB; peak {} MB", status("VmHWM:") >> 20);
}

#[test]
#[ignore = "a measurement, not a check"]
fn markdown_drawn_again_and_again_holds_level() {
    let sample = mareader_epub::sample::markdown;
    render_rounds("markdown, emoji", &sample(6, true));
    render_rounds("markdown, no emoji", &sample(6, false));
    render_rounds("markdown, emoji again", &sample(6, true));
}
