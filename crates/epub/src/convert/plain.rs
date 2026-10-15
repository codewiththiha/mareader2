//! Plain text: paragraphs, wrapped lines and headings. Markup is never read.
use std::fmt::Write as _;

use super::{Draft, Entry, Name, escape, fold_lines};

/// How a file marks its paragraphs, chosen as calibre's auto mode does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Breaks {
    /// A blank line ends a paragraph.
    Blank,
    /// An indented line starts one.
    Indent,
    /// Every line is one.
    Line,
}

/// A paragraph's lines, and whether they were wrapped to fit a width.
struct Paragraph<'a> {
    lines: Vec<&'a str>,
    wrapped: bool,
    /// Two or more blank lines stand before it, as before headings.
    framed: bool,
}

pub(super) fn draft(text: &str, name: Name) -> Draft {
    let text = fold_lines(text);
    let lines: Vec<&str> = text.split('\n').map(str::trim_end).collect();
    let breaks = breaks(&lines);
    let head = lines.get(..HEAD_LINES).unwrap_or(&lines);
    let mut d = Draft::new(field(head, &TITLE_LABELS).or(name.title));
    d.author = field(head, &AUTHOR_LABELS).or(name.author);
    let mut body = String::new();
    for p in paragraphs(&lines, breaks) {
        let heading = match p.lines.as_slice() {
            [line] => heading_level(line, breaks == Breaks::Blank, p.framed),
            _ => None,
        };
        if let (Some(level), Some(line)) = (heading, p.lines.first()) {
            // Each heading opens a chapter, so it starts a page.
            if !body.is_empty() {
                d.chapters.push(std::mem::take(&mut body));
            }
            let title = line.trim();
            d.toc.push(Entry {
                title: title.to_owned(),
                level,
                chapter: d.chapters.len(),
                id: None,
            });
            let _ = writeln!(body, "<h{level}>{}</h{level}>", escape(title));
        } else {
            let _ = writeln!(body, "<p>{}</p>", paragraph(&p));
        }
    }
    d.chapters.push(body);
    if d.toc.is_empty() {
        d.toc.push(Entry {
            title: d.title.clone(),
            level: 1,
            chapter: 0,
            id: None,
        });
    }
    d
}

/// Lines read for labelled fields, as Project Gutenberg's header gives.
const HEAD_LINES: usize = 40;
const TITLE_LABELS: [&str; 5] = ["title", "书名", "書名", "标题", "標題"];
const AUTHOR_LABELS: [&str; 2] = ["author", "作者"];

/// The value of the first `Label: value` line whose label is in `labels`.
fn field(head: &[&str], labels: &[&str]) -> Option<String> {
    head.iter().find_map(|line| {
        let (label, value) = line.split_once([':', '：'])?;
        let label = label.trim().to_lowercase();
        let value = value.trim();
        let fits = !value.is_empty() && chars(value) <= 120;
        (fits && labels.contains(&label.as_str())).then(|| value.to_owned())
    })
}

fn filled(line: &str) -> bool {
    !line.trim().is_empty()
}

fn indented(line: &str) -> bool {
    (line.starts_with("  ") || line.starts_with('\t')) && filled(line)
}

/// Blank lines win if they are common; then indents; else every line.
fn breaks(lines: &[&str]) -> Breaks {
    let filled_lines = lines.iter().filter(|l| filled(l)).count();
    let mut blocks = 0;
    let mut previous = false;
    for l in lines {
        let now = filled(l);
        if now && !previous {
            blocks += 1;
        }
        previous = now;
    }
    if blocks * 20 >= filled_lines {
        return Breaks::Blank;
    }
    let indents = lines.iter().filter(|l| indented(l)).count();
    if indents * 20 >= filled_lines {
        Breaks::Indent
    } else {
        Breaks::Line
    }
}

fn paragraphs<'a>(lines: &[&'a str], breaks: Breaks) -> Vec<Paragraph<'a>> {
    let mut out: Vec<Paragraph<'a>> = Vec::new();
    let mut current: Vec<&'a str> = Vec::new();
    // Blank lines before each paragraph.
    let mut spaces = Vec::new();
    let mut blanks = 0;
    for &line in lines {
        let ends = match breaks {
            Breaks::Blank => !filled(line),
            Breaks::Indent => !filled(line) || indented(line),
            Breaks::Line => true,
        };
        if ends && !current.is_empty() {
            out.push(Paragraph {
                lines: std::mem::take(&mut current),
                wrapped: breaks == Breaks::Indent,
                framed: false,
            });
        }
        if filled(line) {
            if current.is_empty() {
                spaces.push(blanks);
            }
            current.push(line);
            blanks = 0;
        } else {
            blanks += 1;
        }
    }
    if !current.is_empty() {
        out.push(Paragraph {
            lines: current,
            wrapped: breaks == Breaks::Indent,
            framed: false,
        });
    }
    if breaks == Breaks::Blank {
        let width = width(lines);
        for (p, before) in out.iter_mut().zip(&spaces) {
            p.wrapped = wrapped(&p.lines, width);
            p.framed = *before >= 2;
        }
    }
    out
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

/// The width text was wrapped to: nearly the longest line.
fn width(lines: &[&str]) -> usize {
    let mut lengths: Vec<usize> = lines
        .iter()
        .filter(|l| filled(l))
        .map(|l| chars(l))
        .collect();
    lengths.sort_unstable();
    let at = lengths.len() * 95 / 100;
    lengths.get(at).or(lengths.last()).copied().unwrap_or(0)
}

/// Whether breaks fell where the next word wouldn't fit; verse's don't.
fn wrapped(lines: &[&str], width: usize) -> bool {
    let mut breaks = 0;
    let mut wraps = 0;
    for pair in lines.windows(2) {
        let [line, next] = pair else { continue };
        breaks += 1;
        let word = next.split_whitespace().next().map_or(0, chars);
        if chars(line.trim_end()) + 1 + word > width {
            wraps += 1;
        }
    }
    breaks > 0 && wraps * 5 >= breaks * 4
}

/// A paragraph's markup: wrapped lines joined, others kept as lines.
fn paragraph(p: &Paragraph<'_>) -> String {
    if p.wrapped {
        let mut joined = String::new();
        for line in &p.lines {
            let line = line.trim();
            // A word hyphenated at the line's end joins without a space.
            let hyphen = joined.ends_with('-')
                && joined.chars().rev().nth(1).is_some_and(char::is_alphabetic);
            if !joined.is_empty() && !hyphen {
                joined.push(' ');
            }
            joined.push_str(line);
        }
        return escape(&joined);
    }
    let lines: Vec<String> = p.lines.iter().map(|l| kept(l)).collect();
    lines.join("<br/>")
}

/// A kept line, its leading spaces held as no-break spaces.
fn kept(line: &str) -> String {
    let rest = line.trim_start();
    let lead: usize = line
        .chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum();
    format!("{}{}", "\u{a0}".repeat(lead), escape(rest))
}

const PARTS: [&str; 3] = ["part", "book", "volume"];
const CHAPTERS: [&str; 6] = ["chapter", "act", "section", "canto", "scene", "letter"];
const ALONE: [&str; 9] = [
    "prologue",
    "epilogue",
    "preface",
    "introduction",
    "foreword",
    "afterword",
    "appendix",
    "interlude",
    "conclusion",
];
const NUMBER_WORDS: [&str; 32] = [
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "last",
];

/// A heading's level: 1 for `Part` and kin, 2 for `Chapter` and others.
fn heading_level(line: &str, lone_numerals: bool, framed: bool) -> Option<u8> {
    let t = line.trim();
    let words = t.split_whitespace().count();
    if t.is_empty() || chars(t) > 80 || words > 12 || t.ends_with([',', ';']) {
        return None;
    }
    if let Some(level) = cjk_heading(t) {
        return Some(level);
    }
    let lower = t.to_lowercase();
    let mut tokens = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty());
    let first = tokens.next()?;
    let numbered = |mut tokens: std::iter::Peekable<_>| {
        if tokens.peek() == Some(&"the") {
            tokens.next();
        }
        tokens.next().is_some_and(|w: &str| number(w, true))
    };
    if PARTS.contains(&first) && numbered(tokens.clone().peekable()) {
        return Some(1);
    }
    if CHAPTERS.contains(&first) && numbered(tokens.clone().peekable()) {
        return Some(2);
    }
    if ALONE.contains(&first) && (words == 1 || after_word(&lower, first)) {
        return Some(2);
    }
    let bare = t.trim_end_matches('.');
    if lone_numerals && words == 1 && number(bare, false) {
        return Some(2);
    }
    (framed && words <= 8 && capitals(t)).then_some(2)
}

/// Letters, all of them capitals: `COMMENT ON THE POEM.`
fn capitals(t: &str) -> bool {
    let letters = t.chars().filter(|c| c.is_alphabetic());
    let (mut upper, mut lower) = (0, 0);
    for c in letters {
        if c.is_uppercase() {
            upper += 1;
        } else if c.is_lowercase() {
            lower += 1;
        }
    }
    upper >= 2 && lower == 0
}

/// Whether `first` is followed by a title mark, as in `Preface: …`.
fn after_word(lower: &str, first: &str) -> bool {
    lower
        .strip_prefix(first)
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| matches!(c, ':' | '.' | '-' | '\u{2014}' | '\u{2013}'))
}

/// Digits, a Roman numeral or, after a keyword, a number word.
fn number(w: &str, words: bool) -> bool {
    let digits = !w.is_empty() && w.len() <= 4 && w.bytes().all(|b| b.is_ascii_digit());
    let roman = |w: &str| {
        !w.is_empty()
            && w.len() <= 8
            && w.chars().all(|c| {
                let c = if words { c.to_ascii_uppercase() } else { c };
                matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M')
            })
    };
    digits || roman(w) || (words && NUMBER_WORDS.contains(&w))
}

/// Chinese numerals: plain, traditional and the forms cheques use.
const CJK_NUMERALS: &str = "零〇一二三四五六七八九十百千万萬两兩壹贰貳叁叄參肆伍陆陸柒捌玖拾佰仟";
/// Units a title may follow straight on: `第一章天地初开`.
const CJK_CHAPTERS: &str = "章节節回篇幕讲講话話";
/// Units that are also measure words, so `第一本书` stays prose.
const CJK_PARTS: &str = "卷部集本册冊";
/// Marks that may part a unit from its title.
const CJK_GAPS: &str = " \u{3000}:：、()（）";
/// The longest title after the unit, in characters.
const CJK_TITLE: usize = 36;

/// `第十二章` and kin, as Chinese text novels head chapters.
fn cjk_heading(t: &str) -> Option<u8> {
    let rest = t.strip_prefix('第')?;
    let numeral =
        |c: char| c.is_ascii_digit() || ('０'..='９').contains(&c) || CJK_NUMERALS.contains(c);
    let count = rest.chars().take_while(|&c| numeral(c)).count();
    let mut after = rest.chars().skip(count);
    let mark = after.next()?;
    let title: String = after.collect();
    let sentence = title.ends_with(['。', '，', '；']);
    if count == 0 || chars(&title) > CJK_TITLE || sentence {
        return None;
    }
    if CJK_CHAPTERS.contains(mark) {
        return Some(2);
    }
    let parted = title.chars().next().is_none_or(|c| CJK_GAPS.contains(c));
    (CJK_PARTS.contains(mark) && parted).then_some(1)
}
