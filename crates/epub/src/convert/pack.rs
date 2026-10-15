//! A draft packed as an EPUB 3 zip: container, package, contents, chapters.
use std::fmt::Write as _;
use std::io::{Cursor, Write};

use zip::write::{SimpleFileOptions, ZipWriter};
use zip::{CompressionMethod, DateTime};

use super::{Draft, Entry, chapter_file, escape};
use crate::OpenError;

const CONTAINER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>
"#;

/// The zip, stored uncompressed: it is read once, straight back.
pub(super) fn pack(d: &Draft) -> Result<Vec<u8>, OpenError> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    // The mimetype comes first, as the container format requires.
    put(&mut zip, "mimetype", b"application/epub+zip")?;
    put(&mut zip, "META-INF/container.xml", CONTAINER.as_bytes())?;
    put(&mut zip, "OEBPS/content.opf", package(d).as_bytes())?;
    put(&mut zip, "OEBPS/nav.xhtml", nav(d).as_bytes())?;
    for (i, body) in d.chapters.iter().enumerate() {
        let name = format!("OEBPS/text/{}", chapter_file(i));
        put(&mut zip, &name, page(&d.title, body).as_bytes())?;
    }
    for p in &d.pictures {
        put(&mut zip, &format!("OEBPS/{}", p.href), &p.bytes)?;
    }
    let bytes = zip.finish().map_err(|_| OpenError::Invalid)?;
    Ok(bytes.into_inner())
}

fn put(zip: &mut ZipWriter<Cursor<Vec<u8>>>, name: &str, bytes: &[u8]) -> Result<(), OpenError> {
    // A fixed date, so one text always packs to the same bytes.
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default());
    zip.start_file(name, stored)
        .map_err(|_| OpenError::Invalid)?;
    zip.write_all(bytes).map_err(|_| OpenError::Invalid)
}

/// `content.opf`: metadata, every file, and the reading order.
fn package(d: &Draft) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" \
         unique-identifier=\"id\">\n\
         <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n\
         <dc:identifier id=\"id\">urn:mareader:converted</dc:identifier>\n",
    );
    let _ = writeln!(s, "<dc:title>{}</dc:title>", escape(&d.title));
    if let Some(author) = &d.author {
        let _ = writeln!(s, "<dc:creator>{}</dc:creator>", escape(author));
    }
    // Without a language the reader's own setting hyphenates.
    if let Some(language) = &d.language {
        let _ = writeln!(s, "<dc:language>{}</dc:language>", escape(language));
    }
    s.push_str(
        "<meta property=\"dcterms:modified\">2000-01-01T00:00:00Z</meta>\n\
         </metadata>\n<manifest>\n\
         <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" \
         properties=\"nav\"/>\n",
    );
    for i in 0..d.chapters.len() {
        let _ = writeln!(
            s,
            "<item id=\"c{i}\" href=\"text/{}\" media-type=\"application/xhtml+xml\"/>",
            chapter_file(i)
        );
    }
    for (i, p) in d.pictures.iter().enumerate() {
        let _ = writeln!(
            s,
            "<item id=\"p{i}\" href=\"{}\" media-type=\"{}\"/>",
            p.href, p.media
        );
    }
    s.push_str("</manifest>\n<spine>\n");
    for i in 0..d.chapters.len() {
        let _ = writeln!(s, "<itemref idref=\"c{i}\"/>");
    }
    s.push_str("</spine>\n</package>\n");
    s
}

/// `nav.xhtml`: the headings as nested lists, deeper levels inside.
fn nav(d: &Draft) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\" \
         xmlns:epub=\"http://www.idpf.org/2007/ops\">\n\
         <head><title>Contents</title></head>\n<body>\n\
         <nav epub:type=\"toc\" id=\"toc\"><ol>\n",
    );
    // Each open entry: its level, and whether its own list is open.
    let mut open: Vec<(u8, bool)> = Vec::new();
    for e in &d.toc {
        close_to(&mut s, &mut open, e.level);
        if let Some((_, nested)) = open.last_mut()
            && !*nested
        {
            s.push_str("<ol>\n");
            *nested = true;
        }
        let _ = write!(s, "<li><a href=\"{}\">{}</a>", href(e), escape(&e.title));
        open.push((e.level, false));
    }
    close_to(&mut s, &mut open, 0);
    s.push_str("</ol></nav>\n</body>\n</html>\n");
    s
}

/// Closes the entries at `level` or deeper.
fn close_to(s: &mut String, open: &mut Vec<(u8, bool)>, level: u8) {
    while let Some(&(top, nested)) = open.last()
        && top >= level
    {
        open.pop();
        if nested {
            s.push_str("</ol>");
        }
        s.push_str("</li>\n");
    }
}

fn href(e: &Entry) -> String {
    let file = format!("text/{}", chapter_file(e.chapter));
    match &e.id {
        Some(id) => format!("{file}#{}", escape(id)),
        None => file,
    }
}

/// One chapter's XHTML document.
fn page(title: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\">\n\
         <head><title>{}</title></head>\n<body>\n{body}\n</body>\n</html>\n",
        escape(title)
    )
}
