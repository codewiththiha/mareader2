//! Hostile and broken books: each opens or is refused, never crashing.
use std::io::{Cursor, Write};
use std::sync::Arc;

use mareader_epub::{Book, MAX_BLOCKS, MAX_ENTRIES, MAX_ENTRY, OpenError, sample};
use mareader_reflow::Typography;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Levels of nesting in the deep tests, past any worker's stack.
const DEEP: usize = 100_000;

/// A zip of `files`, the mimetype stored first as EPUB asks.
fn pack(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let _ = zip.start_file("mimetype", stored);
    let _ = zip.write_all(b"application/epub+zip");
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in files {
        let _ = zip.start_file(name.as_str(), deflated);
        let _ = zip.write_all(data);
    }
    zip.finish().map(Cursor::into_inner).unwrap_or_default()
}

/// The parts of a book a test may change.
struct Spec {
    version: &'static str,
    metadata: String,
    /// Each chapter's file, as bytes so they can be broken.
    chapters: Vec<Vec<u8>>,
    /// Spine entries whose files are missing.
    ghosts: Vec<&'static str>,
    nav: Option<String>,
    ncx: Option<String>,
    /// Path under `OEBPS/`, bytes, and the manifest item's attributes.
    extra: Vec<(String, Vec<u8>, String)>,
}

impl Spec {
    fn new(chapters: &[&str]) -> Self {
        Self {
            version: "3.0",
            metadata: String::new(),
            chapters: chapters.iter().map(|c| chapter(c).into_bytes()).collect(),
            ghosts: Vec::new(),
            nav: Some(nav(&format!(
                "<ol><li><a href=\"c1.xhtml\">{}</a></li></ol>",
                "Listed"
            ))),
            ncx: None,
            extra: Vec::new(),
        }
    }

    fn build(&self) -> Vec<u8> {
        let mut files = vec![("META-INF/container.xml".to_owned(), CONTAINER.into())];
        let mut manifest = String::new();
        let mut spine = String::new();
        for (i, c) in self.chapters.iter().enumerate() {
            let n = i + 1;
            manifest += &format!(
                "<item id=\"c{n}\" href=\"c{n}.xhtml\" media-type=\"application/xhtml+xml\"/>"
            );
            spine += &format!("<itemref idref=\"c{n}\"/>");
            files.push((format!("OEBPS/c{n}.xhtml"), c.clone()));
        }
        for g in &self.ghosts {
            let item = "media-type=\"application/xhtml+xml\"";
            manifest += &format!("<item id=\"{g}\" href=\"{g}.xhtml\" {item}/>");
            spine += &format!("<itemref idref=\"{g}\"/>");
        }
        if let Some(nav) = &self.nav {
            manifest += "<item id=\"nav\" href=\"nav.xhtml\" \
                         media-type=\"application/xhtml+xml\" properties=\"nav\"/>";
            files.push(("OEBPS/nav.xhtml".to_owned(), nav.clone().into_bytes()));
        }
        let mut toc = "";
        if let Some(ncx) = &self.ncx {
            manifest += "<item id=\"ncx\" href=\"toc.ncx\" \
                         media-type=\"application/x-dtbncx+xml\"/>";
            files.push(("OEBPS/toc.ncx".to_owned(), ncx.clone().into_bytes()));
            toc = " toc=\"ncx\"";
        }
        for (path, data, attributes) in &self.extra {
            manifest += &format!("<item href=\"{path}\" {attributes}/>");
            files.push((format!("OEBPS/{path}"), data.clone()));
        }
        let opf = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"{}\" \
             unique-identifier=\"uid\"><metadata \
             xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
             xmlns:opf=\"http://www.idpf.org/2007/opf\">\
             <dc:identifier id=\"uid\">urn:test</dc:identifier>\
             <dc:title id=\"title\">Hostile</dc:title><dc:language>en</dc:language>\
             {}</metadata><manifest>{manifest}</manifest>\
             <spine{toc}>{spine}</spine></package>",
            self.version, self.metadata
        );
        files.push(("OEBPS/content.opf".to_owned(), opf.into_bytes()));
        pack(&files)
    }
}

const CONTAINER: &str = "<?xml version=\"1.0\"?>\
    <container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
    <rootfiles><rootfile full-path=\"OEBPS/content.opf\" \
    media-type=\"application/oebps-package+xml\"/></rootfiles></container>";

fn chapter(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>c</title></head>\
         <body>{body}</body></html>"
    )
}

fn nav(list: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <html xmlns=\"http://www.w3.org/1999/xhtml\" \
         xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>n</title></head>\
         <body><nav epub:type=\"toc\">{list}</nav></body></html>"
    )
}

fn ncx(map: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\
         <head/><docTitle><text>Hostile</text></docTitle><navMap>{map}</navMap></ncx>"
    )
}

fn open(bytes: Vec<u8>) -> Result<Book, OpenError> {
    Book::open(Arc::new(bytes), &Typography::default())
}

/// Opens on a thread with a worker's usual stack, as the reader does.
fn open_on_worker(bytes: Vec<u8>) -> Result<Book, OpenError> {
    let worker = std::thread::Builder::new().stack_size(2 << 20);
    let opened = worker.spawn(move || open(bytes));
    let opened = opened.map(|h| h.join());
    match opened {
        Ok(Ok(result)) => result,
        _ => Err(OpenError::Invalid),
    }
}

/// Every page's text, run together.
fn words(book: &Book) -> String {
    let lines = (0..book.page_count()).flat_map(|p| book.text(p));
    lines.flat_map(|l| l.chars).map(|c| c.ch).collect()
}

fn titles(book: &Book) -> Vec<(&str, usize)> {
    book.toc()
        .iter()
        .map(|e| (e.title.as_str(), e.depth))
        .collect()
}

/// Sets every size the archive declares for `name` to `size`.
fn declare(zip: &mut [u8], name: &str, size: u32) {
    let name = name.as_bytes();
    // Size and name offsets: local headers, then the central directory.
    let headers = [(b"PK\x03\x04", 22, 26, 30), (b"PK\x01\x02", 24, 28, 46)];
    for (sig, at, len_at, name_at) in headers {
        let mut i = 0;
        while i + name_at + name.len() <= zip.len() {
            let here = &zip[i..];
            let len = u16::from_le_bytes([here[len_at], here[len_at + 1]]) as usize;
            if here.starts_with(sig) && len == name.len() && &here[name_at..][..len] == name {
                zip[i + at..i + at + 4].copy_from_slice(&size.to_le_bytes());
            }
            i += 1;
        }
    }
}

#[test]
fn a_plain_book_opens_with_its_title_and_contents() {
    let book = open(Spec::new(&["<h1>One</h1><p>Hello there.</p>"]).build());
    let book = book.ok();
    assert_eq!(book.as_ref().and_then(|b| b.title()), Some("Hostile"));
    assert_eq!(book.as_ref().map(titles), Some(vec![("Listed", 0)]));
    assert!(
        book.as_ref()
            .map(words)
            .unwrap_or_default()
            .contains("Hello there.")
    );
}

#[test]
fn an_epub_2_book_reads_its_ncx_cover_and_entities() {
    let mut spec = Spec::new(&[
        "<h1>One</h1><p>Rice&nbsp;and timber &mdash; at dawn.</p>",
        "<h1>Two</h1><p>Before.</p><h2><a name=\"later\">Later</a></h2><p>After.</p>",
    ]);
    spec.version = "2.0";
    spec.nav = None;
    spec.ncx = Some(ncx(
        "<navPoint id=\"p1\" playOrder=\"1\"><navLabel><text>Chapter One</text></navLabel>\
         <content src=\"c1.xhtml\"/></navPoint>\
         <navPoint id=\"p2\" playOrder=\"2\"><navLabel><text>Chapter Two</text></navLabel>\
         <content src=\"c2.xhtml\"/><navPoint id=\"p3\" playOrder=\"3\">\
         <navLabel><text>Later on</text></navLabel><content src=\"c2.xhtml#later\"/>\
         </navPoint></navPoint>",
    ));
    // An EPUB 2 cover is named in the metadata, not by a property.
    spec.metadata = "<meta name=\"cover\" content=\"cover-image\"/>".to_owned();
    let cover = sample::cover_png().unwrap_or_default();
    let attributes = "id=\"cover-image\" media-type=\"image/png\"".to_owned();
    spec.extra.push(("cover.png".to_owned(), cover, attributes));
    // EPUB 2 chapters often carry the XHTML 1.1 doctype.
    let doctype = "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.1//EN\" \
                   \"http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd\">";
    for c in &mut spec.chapters {
        let with = String::from_utf8_lossy(c).replacen("?>", &format!("?>{doctype}"), 1);
        *c = with.into_bytes();
    }
    let book = open(spec.build());
    let Ok(book) = book else {
        panic!("the EPUB 2 book didn't open: {book:?}");
    };
    let expected = vec![("Chapter One", 0), ("Chapter Two", 0), ("Later on", 1)];
    assert_eq!(titles(&book), expected);
    // The cover fills the first page; the text starts after it.
    assert!(book.text(0).is_empty());
    assert_eq!(book.pictures(0).len(), 1);
    assert!(words(&book).contains("Rice\u{a0}and timber — at dawn."));
    let later = book.toc().get(2).map(|e| book.locate(e.at));
    assert!(later.is_some_and(|p| p > 1));
}

#[test]
fn a_real_epub_2_book_opens_with_its_ncx_and_cover() {
    let bytes = include_bytes!("books/the-raven-gutenberg-17192.epub");
    let book = open(bytes.to_vec());
    let Ok(book) = book else {
        panic!("The Raven didn't open: {book:?}");
    };
    assert_eq!(book.title(), Some("The Raven"));
    let toc = titles(&book);
    assert_eq!(toc.first(), Some(&("THE RAVEN", 0)));
    assert!(toc.contains(&("Transcriber's Notes", 1)));
    assert!(toc.contains(&("THE RAVEN.", 0)));
    let pages: Vec<usize> = book.toc().iter().map(|e| book.locate(e.at)).collect();
    assert!(pages.is_sorted(), "{pages:?}");
    // The cover named in the metadata fills the first page.
    assert!(book.text(0).is_empty());
    assert_eq!(book.pictures(0).len(), 1);
    assert!(
        words(&book).contains("Once upon a midnight dreary, while I pondered, weak and weary,")
    );
}

#[test]
fn a_lying_entry_size_can_t_inflate_past_it() {
    let bomb = format!("<p>{}</p>", "boom ".repeat(400_000));
    let spec = Spec::new(&["<p>Calm chapter.</p>", &bomb]);
    let mut bytes = spec.build();
    // Two megabytes deflate to a few kilobytes and now claim one.
    declare(&mut bytes, "OEBPS/c2.xhtml", 1000);
    let book = open(bytes);
    let text = book.as_ref().map(words).unwrap_or_default();
    assert!(text.contains("Calm chapter."), "{:?}", book.err());
    assert!(!text.contains("boom"));
}

#[test]
fn archives_past_the_caps_are_refused() {
    let mut bytes = Spec::new(&["<p>Small.</p>"]).build();
    let past = u32::try_from(MAX_ENTRY + 1).unwrap_or(u32::MAX);
    declare(&mut bytes, "OEBPS/c1.xhtml", past);
    assert_eq!(open(bytes).err(), Some(OpenError::TooLarge));
    let crowd: Vec<(String, Vec<u8>)> = (0..MAX_ENTRIES)
        .map(|i| (format!("junk/{i}"), Vec::new()))
        .collect();
    assert_eq!(open(pack(&crowd)).err(), Some(OpenError::TooLarge));
}

#[test]
fn a_deep_nav_falls_back_to_the_headings() {
    let item = "<li><a href=\"c1.xhtml\">x</a><ol>";
    let list = format!(
        "<ol>{}{}</ol>",
        item.repeat(DEEP),
        "</ol></li>".repeat(DEEP)
    );
    let mut spec = Spec::new(&["<h1>Heading</h1><p>Text.</p>"]);
    spec.nav = Some(nav(&list));
    let book = open_on_worker(spec.build());
    assert_eq!(book.as_ref().map(titles).ok(), Some(vec![("Heading", 0)]));
}

#[test]
fn a_deep_ncx_falls_back_to_the_headings() {
    let point = "<navPoint><navLabel><text>x</text></navLabel><content src=\"c1.xhtml\"/>";
    let map = format!("{}{}", point.repeat(DEEP), "</navPoint>".repeat(DEEP));
    let mut spec = Spec::new(&["<h1>Heading</h1><p>Text.</p>"]);
    spec.version = "2.0";
    spec.nav = None;
    spec.ncx = Some(ncx(&map));
    let book = open_on_worker(spec.build());
    assert_eq!(book.as_ref().map(titles).ok(), Some(vec![("Heading", 0)]));
}

#[test]
fn a_long_refinement_chain_costs_only_the_metadata() {
    let mut spec = Spec::new(&["<p>Still readable.</p>"]);
    spec.metadata = (1..=DEEP)
        .map(|i| {
            let of = if i == 1 {
                "title".to_owned()
            } else {
                format!("m{}", i - 1)
            };
            format!("<meta id=\"m{i}\" refines=\"#{of}\" property=\"file-as\">x</meta>")
        })
        .collect();
    let book = open_on_worker(spec.build());
    let Ok(book) = book else {
        panic!("the book didn't open: {book:?}");
    };
    assert_eq!(book.title(), None);
    assert!(words(&book).contains("Still readable."));
}

#[test]
fn broken_markup_still_reads() {
    let mut spec = Spec::new(&[
        "<p>unclosed <b>bold <i>both</p></div></span><p>after &bogus; &amp; \
         <x:y z>tag</x:y> <script>hidden()</script> end",
        "<h1>Second</h1><p>fi@ne</p>",
    ]);
    // Stray bytes that aren't UTF-8 cost a character, not the chapter.
    if let Some(c) = spec.chapters.get_mut(1)
        && let Some(at) = c.iter().position(|&b| b == b'@')
    {
        c.splice(at..=at, [0xff, 0xfe]);
    }
    let book = open(spec.build());
    let text = book.as_ref().map(words).unwrap_or_default();
    assert!(text.contains("unclosed bold both"), "{:?}", book.err());
    assert!(text.contains("after &bogus; & tag end"));
    assert!(!text.contains("hidden()"));
    assert!(text.contains("fi\u{fffd}\u{fffd}ne"));
}

#[test]
fn missing_and_escaping_paths_are_skipped() {
    let mut spec = Spec::new(&[
        "<p>Here.</p><img src=\"../../../etc/passwd\" alt=\"escaped\"/>\
         <img src=\"/abs.png\" alt=\"absolute\"/><img src=\"file:///etc/passwd\" alt=\"file\"/>",
    ]);
    // A spine entry whose file is missing is skipped.
    spec.ghosts.push("ghost");
    let book = open(spec.build());
    let text = book.as_ref().map(words).unwrap_or_default();
    assert!(text.contains("Here."), "{:?}", book.err());
    for alt in ["escaped", "absolute", "file"] {
        assert!(text.contains(alt), "{alt} should show as text");
    }
    let mut empty = Spec::new(&["<p> </p><script>no()</script>"]);
    empty.ghosts.push("ghost");
    assert_eq!(open(empty.build()).err(), Some(OpenError::Empty));
}

/// A PNG that only claims to be `w × h`; its header checks out.
fn claimed_png(w: u32, h: u32) -> Vec<u8> {
    let mut ihdr = b"IHDR".to_vec();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(13u32.to_be_bytes());
    png.extend(&ihdr);
    png.extend(crc32(&ihdr).to_be_bytes());
    png
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
fn a_giant_picture_shows_its_alt_text() {
    let mut spec = Spec::new(&["<p>Before.</p><img src=\"giant.png\" alt=\"Too big to draw\"/>"]);
    let attributes = "id=\"giant\" media-type=\"image/png\"".to_owned();
    spec.extra.push((
        "giant.png".to_owned(),
        claimed_png(100_000, 100_000),
        attributes,
    ));
    let book = open(spec.build());
    let text = book.as_ref().map(words).unwrap_or_default();
    assert!(text.contains("Too big to draw"), "{:?}", book.err());
}

#[test]
fn deep_markup_and_costly_styles_open() {
    let deep = format!(
        "{}<p>Deep.</p>{}",
        "<div>".repeat(DEEP),
        "</div>".repeat(DEEP)
    );
    let rules = "p { font-style: italic }".repeat(5000);
    let sheet = format!("<style>{rules}</style>").repeat(100);
    let paragraphs = "<p class=\"c\">w</p>".repeat(20_000);
    let styled = chapter(&format!("{paragraphs}<p>Styled.</p>")).replacen(
        "<title>c</title>",
        &format!("<title>c</title>{sheet}"),
        1,
    );
    let mut spec = Spec::new(&[&deep]);
    spec.chapters.push(styled.into_bytes());
    let started = std::time::Instant::now();
    let book = open_on_worker(spec.build());
    let text = book.as_ref().map(words).unwrap_or_default();
    assert!(
        text.contains("Deep.") && text.contains("Styled."),
        "{:?}",
        book.err()
    );
    assert!(started.elapsed().as_secs() < 60);
}

#[test]
fn a_flood_of_paragraphs_opens_and_past_the_cap_is_refused() {
    // Their layouts aren't kept, so 400,000 lines open.
    let book = open(Spec::new(&[&"<p>w</p>".repeat(400_000)]).build());
    assert!(book.is_ok_and(|b| b.page_count() > 9000));
    let flood = "<p>w</p>".repeat(MAX_BLOCKS + 1);
    assert_eq!(
        open(Spec::new(&[&flood]).build()).err(),
        Some(OpenError::TooLarge)
    );
}

#[test]
fn mangled_copies_of_a_book_never_crash() {
    let book = sample::epub(2, "Sample");
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for round in 0..48 {
        let mut copy = book.clone();
        if round % 4 == 0 {
            copy.truncate(next() as usize % copy.len());
        }
        for _ in 0..1 + round % 8 {
            let at = next() as usize % copy.len().max(1);
            if let Some(b) = copy.get_mut(at) {
                *b ^= 1 << (next() % 8);
            }
        }
        // Either result is fine; only a crash or a hang fails.
        let _ = open(copy);
    }
}
