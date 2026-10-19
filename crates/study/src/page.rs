//! A page as pieces for the AI to rewrite, and the result it gives back.

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// A text piece's part on the page; its result keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Heading(u8),
    Para,
    Item,
    Quote,
}

/// What stays on the page as it is, never sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kept {
    Picture { alt: String },
    Code { lines: usize },
    Table { rows: usize },
    Other,
}

/// One piece of a page, in reading order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    /// Text the model reads; `code` are inline code runs, in bytes.
    Text {
        role: Role,
        text: String,
        code: Vec<Range<usize>>,
    },
    Kept(Kept),
}

impl Piece {
    pub fn text(role: Role, text: &str) -> Self {
        Self::Text {
            role,
            text: text.to_owned(),
            code: Vec::new(),
        }
    }
}

/// One part of a result, in reading order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Out {
    /// `from` is the piece it rewrites; a summary's come from none.
    Text {
        role: Role,
        text: String,
        code: Vec<Range<usize>>,
        from: Option<usize>,
    },
    /// The piece at this index, as it was.
    Kept(usize),
}

/// A page's result; `missing` pieces came back untouched.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rewritten {
    pub outs: Vec<Out>,
    pub missing: usize,
}

/// One line of a PDF page, in reading order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfLine {
    pub text: String,
    /// It begins a paragraph: a gap or an indent before it.
    pub starts: bool,
}

/// A PDF page as paragraphs, without page numbers or running heads.
pub fn pdf_pieces(lines: &[PdfLine], running: &[String]) -> Vec<Piece> {
    let edge = |i: usize| i < 2 || i + 2 >= lines.len();
    let runs: Vec<String> = running.iter().map(|r| plain(r)).collect();
    let mut paras: Vec<String> = Vec::new();
    let mut next_starts = true;
    for (i, line) in lines.iter().enumerate() {
        let text = line.text.trim();
        if text.is_empty() {
            next_starts = true;
            continue;
        }
        let plain_line = plain(text);
        if edge(i) && (folio(text) || runs.iter().any(|r| !r.is_empty() && *r == plain_line)) {
            continue;
        }
        match paras.last_mut() {
            Some(para) if !(line.starts || next_starts) => join(para, text),
            _ => paras.push(text.to_owned()),
        }
        next_starts = false;
    }
    paras.iter().map(|p| Piece::text(Role::Para, p)).collect()
}

/// Joins a wrapped line; a hyphen splitting a word goes.
fn join(para: &mut String, next: &str) {
    let soft = para.ends_with('\u{ad}');
    let hard = para.ends_with('-') && {
        let mut back = para.chars().rev().skip(1);
        back.next().is_some_and(char::is_alphabetic)
    };
    let lower = next.chars().next().is_some_and(char::is_lowercase);
    if soft || (hard && lower) {
        para.pop();
        para.push_str(next);
    } else {
        para.push(' ');
        para.push_str(next);
    }
}

/// A page number, as "12", "xii", "Page 3", "3 of 10" or "- 4 -".
fn folio(text: &str) -> bool {
    let t = text.trim_matches(|c: char| c == '-' || c == '–' || c.is_whitespace());
    let t = t.to_lowercase();
    let t = t.strip_prefix("page").unwrap_or(&t).trim();
    let numeric = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if let Some((a, b)) = t.split_once(" of ") {
        return numeric(a.trim()) && numeric(b.trim());
    }
    numeric(t) || (1..=100).any(|n| roman(n) == t)
}

/// `n` in lower-case roman numerals, as front matter numbers pages.
fn roman(mut n: u32) -> String {
    let steps = [(100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x")];
    let small = [(9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
    let mut out = String::new();
    for (value, digits) in steps.into_iter().chain(small) {
        while n >= value {
            out.push_str(digits);
            n -= value;
        }
    }
    out
}

/// A line without digits or case, so "Chapter 3 · 41" matches page to page.
fn plain(text: &str) -> String {
    let kept = text.chars().filter(|c| !c.is_ascii_digit());
    let words: String = kept.collect::<String>().to_lowercase();
    words.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "page_tests.rs"]
mod tests;
