//! Markdown and plain text, packed as a real EPUB that `Book::open` reads.
use std::path::Path;

use crate::OpenError;

mod markdown;
mod pack;
mod plain;

/// The largest text file converted, in bytes.
pub const MAX_TEXT: usize = 64 << 20;
/// The largest picture a Markdown file brings in, in bytes.
pub const MAX_PICTURE_FILE: usize = 32 << 20;

/// A text format that converts to an EPUB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Plain,
    Markdown,
}

impl Format {
    /// The format a file name's extension names, if any.
    pub fn of_name(name: &str) -> Option<Self> {
        let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "txt" | "text" => Some(Self::Plain),
            "md" | "markdown" | "mdown" => Some(Self::Markdown),
            _ => None,
        }
    }
}

/// A picture a Markdown file names, read by the caller; none if unreadable.
pub type Pictures<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

/// Converts a text file to EPUB bytes; `name` gives the fallback title.
pub fn convert(
    bytes: &[u8],
    format: Format,
    name: Option<&str>,
    pictures: Pictures<'_>,
) -> Result<Vec<u8>, OpenError> {
    if bytes.len() > MAX_TEXT {
        return Err(OpenError::TooLarge);
    }
    let text = decode(bytes);
    if text.trim().is_empty() {
        return Err(OpenError::Empty);
    }
    let title = name.and_then(stem);
    let draft = match format {
        Format::Plain => plain::draft(&text, title),
        Format::Markdown => markdown::draft(&text, title, pictures),
    };
    pack::pack(&draft)
}

/// A converted book before packing.
struct Draft {
    title: String,
    author: Option<String>,
    language: Option<String>,
    /// Each chapter's body markup, one spine item each.
    chapters: Vec<String>,
    toc: Vec<Entry>,
    pictures: Vec<Picture>,
}

impl Draft {
    fn new(title: Option<String>) -> Self {
        Self {
            title: title.unwrap_or_else(|| "Untitled".into()),
            author: None,
            language: None,
            chapters: Vec::new(),
            toc: Vec::new(),
            pictures: Vec::new(),
        }
    }
}

/// One contents entry: a heading in a chapter.
struct Entry {
    title: String,
    /// 1 for a top-level heading.
    level: u8,
    chapter: usize,
    /// The heading's id; none points at the chapter's start.
    id: Option<String>,
}

/// A picture packed into the book.
struct Picture {
    /// Its path inside the book, under `OEBPS/`.
    href: String,
    media: &'static str,
    bytes: Vec<u8>,
}

/// The file name of chapter `index`, relative to the other chapters.
fn chapter_file(index: usize) -> String {
    format!("chapter-{}.xhtml", index + 1)
}

/// The file name without its directory or extension.
fn stem(name: &str) -> Option<String> {
    let stem = Path::new(name).file_stem()?.to_str()?.trim();
    (!stem.is_empty()).then(|| stem.to_owned())
}

/// Text as UTF-8: a byte order mark wins, then UTF-8, then a guess.
fn decode(bytes: &[u8]) -> String {
    if let Some((encoding, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        let body = bytes.get(bom..).unwrap_or_default();
        return encoding.decode_without_bom_handling(body).0.into_owned();
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    // A megabyte is plenty for the guess.
    let sample = bytes.get(..1 << 20).unwrap_or(bytes);
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(sample, true);
    let encoding = detector.guess(None, true);
    encoding.decode_without_bom_handling(bytes).0.into_owned()
}

/// Every line ending as one LF.
fn fold_lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Text safe inside XML content and attribute values.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0 forbids most control characters.
            c if c.is_control() && !matches!(c, '\n' | '\t') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests;
