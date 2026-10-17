//! A generated EPUB for tests, screenshots and the web demo.
use std::io::Cursor;

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

/// Everything the reader draws besides prose, as books write it.
const FEATURES: &str = r##"
<h2>Things a book holds</h2>
<p>Links read as underlined words: on <a href="chapter-2.xhtml">to the next
chapter</a>, down <a href="#part-2">to Part 1.2</a>, or out
<a href="https://github.com/codewiththiha/mareader2">to the web</a>. Notes
stay beside their mark<a epub:type="noteref" href="#note-1">1</a>.</p>
<p>Lists hang their markers in the margin:</p>
<ul>
  <li>Rice and timber at dawn</li>
  <li>Boats <em>unloading</em> by the jetty
    <ul><li>a nested item</li><li>and another</li></ul>
  </li>
  <li>The old customs house</li>
</ul>
<ol><li>Open the book</li><li>Turn the page</li><li>Read on</li></ol>
<blockquote><p>Nobody remembered who had built the jetty, and nobody
asked.</p></blockquote>
<p>Inline <code>code</code> sits in the line; code blocks keep spaces:</p>
<pre><code>fn main() {
    let boats = 12;
    println!("{boats} boats at dawn");
}</code></pre>
<table>
  <thead><tr><th>Cargo</th><th>Boats</th><th>Arrives</th></tr></thead>
  <tbody>
    <tr><td>Rice</td><td>12</td><td>Dawn</td></tr>
    <tr><td>Timber</td><td>5</td><td>Noon, when the tide allows</td></tr>
    <tr><td>Salt</td><td>2</td><td>Dusk</td></tr>
  </tbody>
</table>
<hr/>
<div class="figure"><img src="../images/river.png" alt="The river at dusk"/></div>
<p class="caption">The river at dusk.</p>
<aside id="note-1" epub:type="footnote"><p>A note opens in a card on its mark.</p></aside>
"##;

/// A dusk landscape `w × h`: sky, sun, and a rippled river.
fn landscape(w: u32, h: u32) -> image::RgbImage {
    let horizon = h * 62 / 100;
    let (sun_x, sun_y, sun_r) = (
        w as f32 / 2.0,
        horizon as f32 - h as f32 * 0.04,
        w as f32 * 0.15,
    );
    image::RgbImage::from_fn(w, h, |x, y| {
        let mix = |a: [f32; 3], b: [f32; 3], t: f32| {
            image::Rgb(std::array::from_fn(|i| {
                let (a, b) = (a.get(i).copied(), b.get(i).copied());
                let (a, b) = (a.unwrap_or(0.0), b.unwrap_or(0.0));
                (a + (b - a) * t.clamp(0.0, 1.0)) as u8
            }))
        };
        let (fx, fy) = (x as f32, y as f32);
        if y < horizon {
            let d = ((fx - sun_x).powi(2) + (fy - sun_y).powi(2)).sqrt();
            if d < sun_r {
                return image::Rgb([255, 214, 128]);
            }
            mix(
                [35.0, 40.0, 110.0],
                [240.0, 140.0, 70.0],
                fy / horizon as f32,
            )
        } else {
            let t = (fy - horizon as f32) / (h - horizon).max(1) as f32;
            let ripple = ((fy * 0.35).sin() * (fx * 0.02).cos()).abs() > 0.92;
            if ripple && (fx - sun_x).abs() < sun_r * (1.0 - t) {
                image::Rgb([250, 190, 110])
            } else {
                mix([30.0, 80.0, 100.0], [15.0, 35.0, 55.0], t)
            }
        }
    })
}

fn encode(img: &image::RgbImage) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// The sample's cover, a portrait PNG.
pub fn cover_png() -> Option<Vec<u8>> {
    encode(&landscape(600, 900))
}

fn chapter_xhtml(n: usize) -> String {
    let mut body = format!("<h1>Chapter {n}</h1>");
    for i in 0..12 {
        let text = PROSE
            .get((n + i) % PROSE.len())
            .copied()
            .unwrap_or_default();
        if i == 6 {
            body.push_str(&format!("<h2 id=\"part-2\">Part {n}.2</h2>"));
        }
        if n == 1 && i == 3 {
            body.push_str(FEATURES);
        }
        body.push_str(&format!(
            "<p><em>{n}.{i}</em> {text} <strong>Bold</strong> end.</p>"
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head>\
         <title>Chapter {n}</title><style>\
         .figure, .caption {{ text-align: center }} \
         .caption {{ font-style: italic }}</style></head>\
         <body>{body}</body></html>"
    )
}

/// A cover, then chapters listed with their parts; the first shows all.
pub fn epub(chapters: usize, title: &str) -> Vec<u8> {
    let chapters: Vec<EpubChapter> = (1..=chapters.max(1))
        .map(|n| {
            let href = format!("text/chapter-{n}.xhtml");
            let part = EpubChapter::new(format!("Part {n}.2")).href(format!("{href}#part-2"));
            EpubChapter::new(format!("Chapter {n}"))
                .href(href)
                .xhtml(chapter_xhtml(n))
                .children(part)
        })
        .collect();
    let mut book = Epub::builder()
        .title(title)
        .author("mareader")
        .chapter(chapters);
    if let Some(cover) = cover_png() {
        book = book.cover_image(("images/cover.png", cover));
    }
    if let Some(river) = encode(&landscape(480, 270)) {
        book = book.resource(("images/river.png", river));
    }
    book.write().to_vec().unwrap_or_default()
}
