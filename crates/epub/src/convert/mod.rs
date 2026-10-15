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
    let name = name.map(Name::read).unwrap_or_default();
    let draft = match format {
        Format::Plain => plain::draft(&text, name),
        Format::Markdown => markdown::draft(&text, name, pictures),
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

/// What a file's name says of its book, the fallback for its own words.
#[derive(Debug, Default, PartialEq)]
struct Name {
    title: Option<String>,
    author: Option<String>,
}

impl Name {
    /// `《河流》作者：林.txt` names both, as Chinese novels do; `notes (1)`
    /// is `notes`.
    fn read(path: &str) -> Self {
        let Some(stem) = Path::new(path).file_stem().and_then(|s| s.to_str()) else {
            return Self::default();
        };
        let stem = without_copy_mark(stem.trim());
        let (stem, author) = match stem.split_once("作者") {
            Some((before, after)) => (before, Some(after.trim_start_matches([':', '：']))),
            None => (stem, None),
        };
        let quoted = between(stem, '《', '》');
        let boxed = stem.strip_prefix('【').and_then(|s| s.split_once('】'));
        let (title, rest) = quoted.or(boxed).unwrap_or((stem, ""));
        // `《河流》[林]` and `《河流》（林 著）` bracket the author.
        let bracketed = ["[]", "【】", "()", "（）"].iter().find_map(|pair| {
            let mut ends = pair.chars();
            let (open, close) = (ends.next()?, ends.next()?);
            between(rest, open, close).map(|(inner, _)| inner.trim_end_matches('著'))
        });
        Self {
            title: kept(title),
            author: author.or(bracketed).and_then(kept),
        }
    }
}

/// The text between `open` and `close`, and what follows.
fn between(s: &str, open: char, close: char) -> Option<(&str, &str)> {
    let (_, rest) = s.split_once(open)?;
    rest.split_once(close)
}

/// `notes (1)` as `notes`: the mark a second download gets.
fn without_copy_mark(stem: &str) -> &str {
    let Some(open) = stem.rfind(['(', '（']) else {
        return stem;
    };
    let (base, mark) = stem.split_at(open);
    let digits = mark
        .trim_start_matches(['(', '（'])
        .trim_end_matches([')', '）']);
    let numbered = !digits.is_empty() && digits.len() <= 3;
    let closed = mark.ends_with([')', '）']);
    let base = base.trim_end();
    if numbered && closed && digits.bytes().all(|b| b.is_ascii_digit()) && !base.is_empty() {
        base
    } else {
        stem
    }
}

/// `s` trimmed, unless nothing is left.
fn kept(s: &str) -> Option<String> {
    let s = s.trim_matches(|c: char| c.is_whitespace() || "-_·".contains(c));
    (!s.is_empty()).then(|| s.to_owned())
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
