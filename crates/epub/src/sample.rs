//! A generated EPUB for tests, screenshots and the web demo.
use rbook::Epub;
use rbook::epub::EpubChapter;

const PROSE: [&str; 6] = [
    "This book was written by mareader itself, so the demo needs no file. \
     Its pages are laid out from flowing text, not drawn ahead of time.",
    "The river ran slow and brown past the old customs house, where the \
     boats unloaded rice and timber at dawn. Nobody remembered who had \
     built the jetty, and nobody asked.",
    "Change the window, the zoom or the theme: the words stay where they \
     are and only the paper and ink around them move.",
    "A long paragraph is cut between lines, never through one, and a \
     heading always keeps company with the text that follows it.",
    "The quick brown fox jumps over the lazy dog. Sphinx of black quartz, \
     judge my vow. Fine ligatures: fi, fl, ffi.",
    "Each chapter opens on a fresh page, as it would in a printed book.",
];

/// `chapters` chapters of a dozen paragraphs, with some emphasis.
pub fn epub(chapters: usize, title: &str) -> Vec<u8> {
    let chapters = (1..=chapters.max(1)).map(|n| {
        let mut body = format!("<h1>Chapter {n}</h1>");
        for i in 0..12 {
            let text = PROSE
                .get((n + i) % PROSE.len())
                .copied()
                .unwrap_or_default();
            if i == 6 {
                body.push_str(&format!("<h2>Part {n}.2</h2>"));
            }
            body.push_str(&format!(
                "<p><em>{n}.{i}</em> {text} <strong>Bold</strong> end.</p>"
            ));
        }
        EpubChapter::new(format!("Chapter {n}")).xhtml_body(body)
    });
    let book = Epub::builder()
        .title(title)
        .author("mareader")
        .chapter(chapters.collect::<Vec<_>>());
    book.write().to_vec().unwrap_or_default()
}
