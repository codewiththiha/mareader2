//! A book's pages as sentences to say, each knowing where it is drawn.

use crate::sentences;

/// A place in a book's text: a page and a character on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Spot {
    pub page: usize,
    pub char: usize,
}

/// One sentence to say, from `start` up to `end`, which it leaves out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sentence {
    pub text: String,
    pub start: Spot,
    pub end: Spot,
}

/// One character of a page's text, as a page's text layer holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub ch: char,
    pub line: u32,
    pub space_before: bool,
    /// Its line goes on from the one before, unbroken by a paragraph.
    pub wraps: bool,
}

/// Turns pages into sentences, carrying one that runs onto the next.
#[derive(Debug, Default)]
pub struct Script {
    text: String,
    /// Each character's byte in `text` and its spot; joins have none.
    spots: Vec<(usize, Spot)>,
    /// The last character added, to join the next to it.
    last: Option<(char, Spot, u32)>,
    /// Drops lone page numbers at a page's top or foot.
    skip_folios: bool,
}

impl Script {
    /// A script that drops lone page numbers if `skip_folios`.
    pub fn new(skip_folios: bool) -> Self {
        Self {
            skip_folios,
            ..Self::default()
        }
    }

    /// Adds `page` from char `from`; returns sentences ended, holding the last.
    pub fn page(&mut self, page: usize, glyphs: &[Glyph], from: usize) -> Vec<Sentence> {
        let lines = lines(glyphs, from);
        let count = lines.len();
        for (n, line) in lines.into_iter().enumerate() {
            let edge = n == 0 || n + 1 == count;
            if self.skip_folios && edge && folio(glyphs, &line) {
                continue;
            }
            for i in line {
                if let Some(g) = glyphs.get(i) {
                    self.add(*g, Spot { page, char: i });
                }
            }
        }
        self.take(false)
    }

    /// The sentences still waiting: the book ended.
    pub fn finish(&mut self) -> Vec<Sentence> {
        self.take(true)
    }

    fn add(&mut self, g: Glyph, spot: Spot) {
        if let Some((prev, at, line)) = self.last {
            let turned = at.page != spot.page || line != g.line;
            if turned && !g.wraps {
                self.text.push('\n');
            } else if turned {
                self.join(prev, g.ch);
            } else if g.space_before {
                self.text.push(' ');
            }
        }
        self.spots.push((self.text.len(), spot));
        self.text.push(g.ch);
        self.last = Some((g.ch, spot, g.line));
    }

    /// Joins a wrapped line to the last: a split word mends at its hyphen.
    fn join(&mut self, prev: char, next: char) {
        if prev.is_whitespace() || (spaceless(prev) && spaceless(next)) {
            return;
        }
        let hyphen = matches!(prev, '-' | '\u{ad}' | '‐');
        if hyphen && next.is_lowercase() {
            self.text.pop();
            self.spots.pop();
            return;
        }
        self.text.push(' ');
    }

    /// Cuts the finished sentences out, keeping a last unfinished one.
    fn take(&mut self, all: bool) -> Vec<Sentence> {
        let mut ranges = sentences::split(&self.text);
        let keep = match all {
            true => None,
            false => ranges.pop(),
        };
        let out = ranges
            .into_iter()
            .filter_map(|r| self.sentence(r))
            .collect();
        match keep {
            Some(rest) => self.keep_from(rest.start),
            None => {
                self.text.clear();
                self.spots.clear();
            }
        }
        out
    }

    fn sentence(&self, range: std::ops::Range<usize>) -> Option<Sentence> {
        let first = self.spots.partition_point(|(b, _)| *b < range.start);
        let last = self.spots.partition_point(|(b, _)| *b < range.end);
        let start = self.spots.get(first)?.1;
        let end = self.spots.get(last.checked_sub(1)?)?.1;
        Some(Sentence {
            text: self.text.get(range)?.to_owned(),
            start,
            end: Spot {
                page: end.page,
                char: end.char + 1,
            },
        })
    }

    fn keep_from(&mut self, byte: usize) {
        self.text = self.text.get(byte..).unwrap_or_default().to_owned();
        self.spots.retain(|(b, _)| *b >= byte);
        for (b, _) in &mut self.spots {
            *b -= byte;
        }
    }
}

/// The page's lines from character `from`, as runs of indices.
fn lines(glyphs: &[Glyph], from: usize) -> Vec<std::ops::Range<usize>> {
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    for (i, g) in glyphs.iter().enumerate().skip(from) {
        let same = out.last().is_some_and(|r| {
            let prev = r.end.checked_sub(1).and_then(|p| glyphs.get(p));
            r.end == i && prev.is_some_and(|p| p.line == g.line)
        });
        match (same, out.last_mut()) {
            (true, Some(r)) => r.end = i + 1,
            _ => out.push(i..i + 1),
        }
    }
    out
}

/// True when a line holds only a page number: digits or roman numerals.
fn folio(glyphs: &[Glyph], line: &std::ops::Range<usize>) -> bool {
    let text: String = (glyphs.get(line.clone()).unwrap_or_default().iter())
        .map(|g| g.ch)
        .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '–' | '—' | '.'))
        .collect();
    let digits = !text.is_empty() && text.len() <= 5 && text.chars().all(|c| c.is_ascii_digit());
    let roman = (1..=6).contains(&text.len())
        && (text.chars()).all(|c| matches!(c.to_ascii_lowercase(), 'i' | 'v' | 'x' | 'l'));
    digits || roman
}

/// Scripts written without spaces between words.
fn spaceless(c: char) -> bool {
    matches!(c as u32,
        0x0E00..=0x0EFF | 0x1000..=0x109F | 0x1780..=0x17FF
        | 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page from lines of text; a line starting `+` goes on from the last.
    fn page(lines: &[&str]) -> Vec<Glyph> {
        let mut out = Vec::new();
        for (n, line) in lines.iter().enumerate() {
            let (wraps, line) = match line.strip_prefix('+') {
                Some(rest) => (true, rest),
                None => (false, *line),
            };
            let mut space = false;
            for ch in line.chars() {
                if ch == ' ' {
                    space = true;
                    continue;
                }
                out.push(Glyph {
                    ch,
                    line: n as u32,
                    space_before: space,
                    wraps,
                });
                space = false;
            }
        }
        out
    }

    fn texts(sentences: &[Sentence]) -> Vec<&str> {
        sentences.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn wrapped_lines_join_and_split_words_mend() {
        let mut s = Script::default();
        let glyphs = page(&["The river ran past the mill-", "+stone. It was", "+late."]);
        let mut out = s.page(0, &glyphs, 0);
        out.extend(s.finish());
        assert_eq!(
            texts(&out),
            ["The river ran past the millstone.", "It was late."]
        );
    }

    #[test]
    fn a_sentence_runs_onto_the_next_page_and_knows_both_ends() {
        let mut s = Script::default();
        let first = s.page(3, &page(&["One. Two runs"]), 0);
        assert_eq!(texts(&first), ["One."]);
        let second = s.page(4, &page(&["+on. Three."]), 0);
        assert_eq!(texts(&second), ["Two runs on."]);
        let two = second.first().map(|s| (s.start, s.end));
        let spots = (Spot { page: 3, char: 4 }, Spot { page: 4, char: 3 });
        assert_eq!(two, Some(spots));
        assert_eq!(texts(&s.finish()), ["Three."]);
    }

    #[test]
    fn reading_starts_mid_page_and_a_paragraph_break_ends_a_heading() {
        let mut s = Script::default();
        let glyphs = page(&["Skipped words. Chapter Two", "It began."]);
        let from = "Skippedwords.".chars().count();
        let mut out = s.page(0, &glyphs, from);
        out.extend(s.finish());
        assert_eq!(texts(&out), ["Chapter Two", "It began."]);
        assert_eq!(out.first().map(|s| s.start.char), Some(from));
    }

    #[test]
    fn page_numbers_at_the_edges_are_skipped_when_asked() {
        let glyphs = page(&["12", "Text here.", "xiv"]);
        let mut s = Script::new(true);
        let mut out = s.page(0, &glyphs, 0);
        out.extend(s.finish());
        assert_eq!(texts(&out), ["Text here."]);
    }

    #[test]
    fn unspaced_scripts_join_lines_without_a_space() {
        let mut s = Script::default();
        let mut out = s.page(0, &page(&["雨が降", "+った。"]), 0);
        out.extend(s.finish());
        assert_eq!(texts(&out), ["雨が降った。"]);
    }
}
