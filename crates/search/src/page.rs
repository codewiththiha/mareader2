//! A page's text made searchable, and where a query turns up in it.

use std::ops::Range;

use crate::fold::{fold, invisible, spell};

/// Characters of context each side of a hit in its snippet.
pub const SNIPPET_RADIUS: usize = 32;

/// One character of a page, as its text layer reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub ch: char,
    pub line: u32,
    /// A gap the layer saw before it, though no space was written.
    pub space_before: bool,
}

/// A space put in for a gap: no layer character stands behind it.
const GAP: u32 = u32::MAX;

/// A page's text, gaps as single spaces, with a folded twin to match.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    /// As written, for snippets.
    shown: String,
    /// Folded char for char, so positions agree with `shown`.
    folded: String,
    /// Each char's layer character, or `GAP`.
    layer: Vec<u32>,
    /// Hyphens that split a word across lines; a match may skip them.
    soft: Vec<u32>,
}

impl Page {
    pub fn new(glyphs: impl IntoIterator<Item = Glyph>) -> Self {
        let mut page = Self::default();
        let (mut line, mut gap) = (None, false);
        for (i, g) in glyphs.into_iter().enumerate() {
            let at = u32::try_from(i).unwrap_or(GAP);
            let broke = line.is_some_and(|l| l != g.line);
            line = Some(g.line);
            if invisible(g.ch) {
                continue;
            }
            if g.ch.is_whitespace() {
                gap = true;
                continue;
            }
            if broke && !gap && g.ch.is_lowercase() && page.ends_in_a_split_word() {
                let hyphen = page.len().saturating_sub(1);
                page.soft.push(u32::try_from(hyphen).unwrap_or(GAP));
            } else if (gap || broke || g.space_before) && !page.layer.is_empty() {
                page.push(' ', GAP);
            }
            gap = false;
            match spell(g.ch) {
                Some(letters) => letters.chars().for_each(|c| page.push(c, at)),
                None => page.push(g.ch, at),
            }
        }
        page
    }

    fn push(&mut self, c: char, at: u32) {
        self.shown.push(c);
        self.folded.push(fold(c));
        self.layer.push(at);
    }

    /// Characters, gaps included.
    pub fn len(&self) -> usize {
        self.layer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layer.is_empty()
    }

    /// A letter then a hyphen: a word the line end may have split.
    fn ends_in_a_split_word(&self) -> bool {
        let mut back = self.folded.chars().rev();
        back.next() == Some('-') && back.next().is_some_and(char::is_alphabetic)
    }

    /// Where `needle` turns up, in order and never overlapping.
    pub fn find(&self, needle: &Needle) -> Vec<Range<usize>> {
        let chars = Chars::new(&self.folded);
        let n = needle.chars;
        let mut hits: Vec<Range<usize>> = (self.folded.match_indices(&needle.folded))
            .map(|(b, _)| chars.at(b))
            .map(|c| c..c + n)
            .collect();
        if !self.soft.is_empty() {
            hits.extend(self.across_lines(needle));
            hits.sort_unstable_by_key(|h| h.start);
            let mut end = 0;
            hits.retain(|h| {
                let keep = h.start >= end;
                end = if keep { h.end } else { end };
                keep
            });
        }
        hits
    }

    /// Matches that run through a split word's hyphen as if it weren't there.
    fn across_lines(&self, needle: &Needle) -> Vec<Range<usize>> {
        let soft = |i: usize| self.soft.binary_search(&(i as u32)).is_ok();
        // Each joined char's place in `folded`.
        let (mut joined, mut from) = (String::new(), Vec::new());
        for (i, c) in self.folded.chars().enumerate() {
            if !soft(i) {
                joined.push(c);
                from.push(i);
            }
        }
        let chars = Chars::new(&joined);
        let n = needle.chars;
        (joined.match_indices(&needle.folded))
            .filter_map(|(b, _)| {
                let start = chars.at(b);
                let (first, last) = (*from.get(start)?, *from.get(start + n - 1)?);
                // Only hyphen-split matches; the plain pass found the rest.
                (last - first >= n).then_some(first..last + 1)
            })
            .collect()
    }

    /// The layer characters behind `chars`, gaps at either end dropped.
    pub fn layer_range(&self, chars: Range<usize>) -> Option<Range<usize>> {
        let span = self.layer.get(chars)?;
        let first = *span.iter().find(|&&i| i != GAP)?;
        let last = *span.iter().rev().find(|&&i| i != GAP)?;
        Some(first as usize..last as usize + 1)
    }

    /// The hit with up to `SNIPPET_RADIUS` characters each side.
    pub fn snippet(&self, chars: Range<usize>) -> String {
        let from = chars.start.saturating_sub(SNIPPET_RADIUS);
        let to = (chars.end + SNIPPET_RADIUS).min(self.len());
        let mut out = String::new();
        if from > 0 {
            out.push('…');
        }
        out.extend(self.shown.chars().skip(from).take(to.saturating_sub(from)));
        if to < self.len() {
            out.push('…');
        }
        out
    }
}

/// Byte offsets to char positions, quick when the text is ASCII.
struct Chars<'a> {
    text: &'a str,
    ascii: bool,
}

impl<'a> Chars<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            ascii: text.is_ascii(),
        }
    }

    fn at(&self, byte: usize) -> usize {
        if self.ascii {
            return byte;
        }
        self.text.get(..byte).map_or(0, |s| s.chars().count())
    }
}

/// What the reader typed, made to match a page's folded text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Needle {
    folded: String,
    chars: usize,
}

impl Needle {
    /// None for a query of nothing but spaces.
    pub fn new(query: &str) -> Option<Self> {
        let mut folded = String::new();
        for word in query.split_whitespace() {
            if !folded.is_empty() {
                folded.push(' ');
            }
            for c in word.chars().filter(|c| !invisible(*c)) {
                match spell(c) {
                    Some(letters) => folded.extend(letters.chars().map(fold)),
                    None => folded.push(fold(c)),
                }
            }
        }
        let chars = folded.chars().count();
        (chars > 0).then_some(Self { folded, chars })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Lines of words; a word's leading `^` marks a gap the layer saw.
    fn page(lines: &[&str]) -> Page {
        let glyphs = lines.iter().enumerate().flat_map(|(n, l)| {
            let mut gap = false;
            l.chars()
                .filter_map(move |ch| {
                    if ch == '^' {
                        gap = true;
                        return None;
                    }
                    let g = Glyph {
                        ch,
                        line: n as u32,
                        space_before: std::mem::take(&mut gap),
                    };
                    Some(g)
                })
                .collect::<Vec<_>>()
        });
        Page::new(glyphs)
    }

    fn found(p: &Page, q: &str) -> Vec<String> {
        let needle = Needle::new(q).unwrap();
        let shown: Vec<char> = p.shown.chars().collect();
        let hits = p.find(&needle);
        hits.into_iter()
            .map(|h| shown[h].iter().collect())
            .collect()
    }

    #[test]
    fn gaps_and_line_ends_read_as_one_space() {
        let p = page(&["The  quick", "brown^fox"]);
        assert_eq!(p.shown, "The quick brown fox");
        assert_eq!(found(&p, "QUICK   brown"), ["quick brown"]);
        assert_eq!(found(&p, "brown fox"), ["brown fox"]);
        assert_eq!(Needle::new("  \t "), None);
    }

    #[test]
    fn matches_fold_and_never_overlap() {
        let p = page(&["Café CAFE cafe aaa"]);
        assert_eq!(found(&p, "cafe"), ["Café", "CAFE", "cafe"]);
        assert_eq!(found(&p, "aa"), ["aa"]);
        // Curly and straight quotes are one.
        assert_eq!(found(&page(&["it\u{2019}s"]), "it's"), ["it\u{2019}s"]);
    }

    #[test]
    fn a_ligature_is_spelled_out_and_maps_to_its_glyph() {
        let p = page(&["e\u{fb03}cient"]);
        assert_eq!(found(&p, "efficient").len(), 1);
        let hit = p.find(&Needle::new("ffi").unwrap())[0].clone();
        assert_eq!(p.layer_range(hit), Some(1..2), "the one ligature glyph");
    }

    #[test]
    fn a_word_split_at_a_line_end_is_found_both_ways() {
        let p = page(&["a docu-", "ment and well-", "Known"]);
        assert_eq!(found(&p, "document"), ["docu-ment"]);
        assert_eq!(found(&p, "docu-ment"), ["docu-ment"]);
        let hit = p.find(&Needle::new("document").unwrap())[0].clone();
        assert_eq!(p.layer_range(hit), Some(2..11), "the hyphen highlights too");
        // A capital after the break is a new word, not a split one.
        assert_eq!(found(&p, "well- known"), ["well- Known"]);
        assert!(found_none(&p, "wellknown"));
    }

    fn found_none(p: &Page, q: &str) -> bool {
        p.find(&Needle::new(q).unwrap()).is_empty()
    }

    #[test]
    fn layer_ranges_skip_gaps_and_snippets_cut_both_ends() {
        let p = page(&["one two"]);
        assert_eq!(p.layer_range(3..7), Some(4..7), "the gap starts nothing");
        let long = format!("{}target{}", "x".repeat(50), "y".repeat(50));
        let p = page(&[long.as_str()]);
        let hit = p.find(&Needle::new("target").unwrap())[0].clone();
        let s = p.snippet(hit);
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert_eq!(s.chars().count(), SNIPPET_RADIUS * 2 + 6 + 2);
        let p = page(&["target"]);
        assert_eq!(p.snippet(0..6), "target");
    }

    #[test]
    fn soft_hyphens_and_zero_widths_vanish() {
        let p = page(&["hy\u{00ad}phen\u{200b}ated"]);
        assert_eq!(found(&p, "hyphenated").len(), 1);
    }
}
