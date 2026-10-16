//! The characters on a page and where they sit, for selecting and copying.

use std::ops::Range;

use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Device, DrawMode, DrawProps, Image, ImageDrawProps, SoftMask,
};
use hayro::kurbo::{Affine, BezPath, Point};

use crate::links::Link;

/// Glyph boxes span the em: a fifth below the baseline, the rest above.
const DESCENT: f64 = -200.0;
const ASCENT: f64 = 800.0;
/// The width assumed for a glyph whose font gives none, in em thousandths.
const FALLBACK_ADVANCE: f64 = 500.0;
/// Characters kept per page; past it a page is not text but noise.
pub const MAX_CHARS: usize = 200_000;

/// An upright box in page points, origin top left, y down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Bounds {
    pub(crate) fn around(points: impl IntoIterator<Item = Point>) -> Option<Self> {
        let mut out: Option<Self> = None;
        for p in points {
            let (x, y) = (p.x as f32, p.y as f32);
            if !(x.is_finite() && y.is_finite()) {
                return None;
            }
            let b = Self {
                x0: x,
                y0: y,
                x1: x,
                y1: y,
            };
            out = Some(out.map_or(b, |o| o.union(b)));
        }
        out
    }

    pub fn union(self, o: Self) -> Self {
        Self {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        (self.x0..=self.x1).contains(&x) && (self.y0..=self.y1).contains(&y)
    }

    pub fn height(self) -> f32 {
        self.y1 - self.y0
    }

    fn centre_x(self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }

    /// How far `v` lies outside `lo..=hi`; zero inside.
    fn gap(v: f32, lo: f32, hi: f32) -> f32 {
        (lo - v).max(v - hi).max(0.0)
    }
}

/// What a page offers besides its pixels: its text and its links.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextLayer {
    pub text: PageText,
    pub links: Vec<Link>,
    /// A book's: each character's block and byte; a PDF has none.
    pub places: Vec<Option<(u32, u32)>>,
}

impl TextLayer {
    pub fn bytes(&self) -> usize {
        let places = self.places.len() * size_of::<Option<(u32, u32)>>();
        self.text.bytes() + self.links.len() * size_of::<Link>() + places
    }

    /// Character `i`'s block and byte in a book's text.
    pub fn place(&self, i: usize) -> Option<(usize, usize)> {
        let (block, byte) = self.places.get(i).copied().flatten()?;
        Some((block as usize, byte as usize))
    }

    /// The last link listed under a point: later ones are drawn on top.
    pub fn link_at(&self, x: f32, y: f32) -> Option<&Link> {
        self.links.iter().rev().find(|l| l.bounds.contains(x, y))
    }
}

/// One character, its box, its line, and whether a gap precedes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextChar {
    pub ch: char,
    pub bounds: Bounds,
    pub line: u32,
    /// The page shows a space here without drawing one.
    pub space_before: bool,
}

/// A run of characters on one baseline.
#[derive(Debug, Clone, PartialEq)]
struct Line {
    chars: Range<usize>,
    bounds: Bounds,
    /// It goes on from the line before: a copy joins them unbroken.
    wraps: bool,
}

/// A page's characters in content order, grouped into lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageText {
    chars: Vec<TextChar>,
    lines: Vec<Line>,
}

impl PageText {
    pub fn chars(&self) -> &[TextChar] {
        &self.chars
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Roughly what the page's text holds in memory.
    pub fn bytes(&self) -> usize {
        self.chars.len() * size_of::<TextChar>() + self.lines.len() * size_of::<Line>()
    }

    /// The character drawn at a point, a little slack included.
    pub fn char_at(&self, x: f32, y: f32) -> Option<usize> {
        let slack = |b: Bounds| Bounds {
            x0: b.x0 - 1.0,
            y0: b.y0 - 1.0,
            x1: b.x1 + 1.0,
            y1: b.y1 + 1.0,
        };
        self.chars
            .iter()
            .position(|c| slack(c.bounds).contains(x, y))
    }

    /// The caret nearest a point, from 0 to `len`, as a drag ends there.
    pub fn caret_at(&self, x: f32, y: f32) -> usize {
        let top = self
            .lines
            .iter()
            .map(|l| l.bounds.y0)
            .fold(f32::MAX, f32::min);
        let bottom = self
            .lines
            .iter()
            .map(|l| l.bounds.y1)
            .fold(f32::MIN, f32::max);
        if self.lines.is_empty() || y < top {
            return 0;
        }
        if y > bottom {
            return self.len();
        }
        let line = self.nearest_line(x, y);
        let Some(line) = self.lines.get(line) else {
            return 0;
        };
        let chars = self.chars.get(line.chars.clone()).unwrap_or_default();
        let after = chars.iter().position(|c| x < c.bounds.centre_x());
        line.chars.start + after.unwrap_or(chars.len())
    }

    /// Lines containing `y` win, then the closest by height, then by width.
    fn nearest_line(&self, x: f32, y: f32) -> usize {
        let score = |l: &Line| {
            let b = l.bounds;
            let dy = Bounds::gap(y, b.y0, b.y1);
            let dx = Bounds::gap(x, b.x0, b.x1);
            (dy, dx)
        };
        let mut best = (0, (f32::MAX, f32::MAX));
        for (i, l) in self.lines.iter().enumerate() {
            let s = score(l);
            if s.0 < best.1.0 || (s.0 == best.1.0 && s.1 < best.1.1) {
                best = (i, s);
            }
        }
        best.0
    }

    /// The word holding character `at`: a run of one kind on one line.
    pub fn word_at(&self, at: usize) -> Range<usize> {
        let Some(c) = self.chars.get(at) else {
            return at..at;
        };
        let kind = Kind::of(c.ch);
        let same = |i: usize| {
            self.chars
                .get(i)
                .is_some_and(|o| o.line == c.line && Kind::of(o.ch) == kind)
        };
        let mut start = at;
        while start > 0 && same(start - 1) && !self.space_at(start) {
            start -= 1;
        }
        let mut end = at + 1;
        while same(end) && !self.space_at(end) {
            end += 1;
        }
        start..end
    }

    /// The line holding `at` and those it wraps into: a book's paragraph.
    pub fn line_at(&self, at: usize) -> Range<usize> {
        let line = self.chars.get(at).map_or(0, |c| c.line as usize);
        let wraps = |n: usize| self.lines.get(n).is_some_and(|l| l.wraps);
        let mut first = line;
        while first > 0 && wraps(first) {
            first -= 1;
        }
        let mut last = line;
        while wraps(last + 1) {
            last += 1;
        }
        match (self.lines.get(first), self.lines.get(last)) {
            (Some(a), Some(b)) => a.chars.start..b.chars.end,
            _ => at..at,
        }
    }

    /// The page opens mid-paragraph, going on from the page before.
    pub fn continues(&self) -> bool {
        self.lines.first().is_some_and(|l| l.wraps)
    }

    fn wraps(&self, line: u32) -> bool {
        let line = self.lines.get(line as usize);
        line.is_some_and(|l| l.wraps)
    }

    fn space_at(&self, i: usize) -> bool {
        self.chars.get(i).is_some_and(|c| c.space_before)
    }

    /// The text of `range`, gaps as spaces and line ends as newlines.
    pub fn text(&self, range: Range<usize>) -> String {
        let mut out = String::new();
        let mut prev: Option<&TextChar> = None;
        for c in self.chars.get(range).unwrap_or_default() {
            match prev {
                Some(p) if p.line != c.line => {
                    if !self.wraps(c.line) {
                        out.push('\n');
                    }
                }
                Some(_) if c.space_before => out.push(' '),
                _ => {}
            }
            out.push(c.ch);
            prev = Some(c);
        }
        out
    }

    /// One box per line touched by `range`, for painting a selection.
    pub fn boxes(&self, range: Range<usize>) -> Vec<Bounds> {
        let mut out: Vec<(u32, Bounds)> = Vec::new();
        for c in self.chars.get(range).unwrap_or_default() {
            match out.last_mut() {
                Some((line, b)) if *line == c.line => *b = b.union(c.bounds),
                _ => out.push((c.line, c.bounds)),
            }
        }
        out.into_iter().map(|(_, b)| b).collect()
    }

    /// A page from `(wraps, characters)` lines in reading order.
    pub fn from_lines(lines: impl IntoIterator<Item = (bool, Vec<(char, Bounds)>)>) -> Self {
        let mut out = Self::default();
        for (wraps, line) in lines {
            let room = MAX_CHARS.saturating_sub(out.chars.len());
            let Some(bounds) = line.iter().map(|c| c.1).reduce(Bounds::union) else {
                continue;
            };
            if room == 0 {
                break;
            }
            let index = u32::try_from(out.lines.len()).unwrap_or(u32::MAX);
            let start = out.chars.len();
            out.chars
                .extend(line.into_iter().take(room).map(|(ch, bounds)| TextChar {
                    ch,
                    bounds,
                    line: index,
                    space_before: false,
                }));
            out.lines.push(Line {
                chars: start..out.chars.len(),
                bounds,
                wraps,
            });
        }
        out
    }

    /// Groups glyphs into lines and marks the gaps between words.
    pub(crate) fn from_glyphs(glyphs: Vec<Placed>) -> Self {
        let mut chars: Vec<TextChar> = Vec::with_capacity(glyphs.len());
        let mut lines: Vec<Line> = Vec::new();
        let mut prev: Option<Placed> = None;
        for g in glyphs {
            let (new_line, gap) = match &prev {
                None => (true, false),
                Some(p) => p.follow(&g),
            };
            if new_line {
                lines.push(Line {
                    chars: chars.len()..chars.len(),
                    bounds: g.bounds,
                    wraps: false,
                });
            }
            let line = lines.len().saturating_sub(1);
            if let Some(l) = lines.last_mut() {
                l.bounds = l.bounds.union(g.bounds);
                l.chars.end = chars.len() + 1;
            }
            chars.push(TextChar {
                ch: g.ch,
                bounds: g.bounds,
                line: u32::try_from(line).unwrap_or(u32::MAX),
                space_before: gap,
            });
            prev = Some(g);
        }
        Self { chars, lines }
    }
}

/// Characters a double click keeps together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Mark,
}

impl Kind {
    fn of(c: char) -> Self {
        if c.is_alphanumeric() || c == '_' || c == '\'' || c == '\u{2019}' {
            Self::Word
        } else if c.is_whitespace() {
            Self::Space
        } else {
            Self::Mark
        }
    }
}

/// A character with its box, baseline origin and em size, in page points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
    pub ch: char,
    pub bounds: Bounds,
    pub origin: (f32, f32),
    pub size: f32,
}

impl Placed {
    /// Whether `next` starts a new line, and if a space falls between.
    fn follow(&self, next: &Self) -> (bool, bool) {
        let size = self.size.min(next.size).max(0.1);
        let dy = (next.origin.1 - self.origin.1).abs();
        let back = next.origin.0 < self.bounds.x0 - size;
        if dy > size * 0.5 || back {
            return (true, false);
        }
        let gap = next.origin.0 - self.bounds.x1;
        let blank = self.ch.is_whitespace() || next.ch.is_whitespace();
        (false, gap > size * 0.15 && !blank)
    }
}

/// Collects glyphs as the interpreter draws them; drawing is ignored.
#[derive(Default)]
pub(crate) struct Collector {
    pub glyphs: Vec<Placed>,
}

impl Collector {
    fn add(&mut self, transform: Affine, advance: f64, text: BfString) {
        let chars: Vec<char> = match text {
            BfString::Char(c) => vec![c],
            BfString::String(s) => s.chars().collect(),
        };
        let chars: Vec<char> = chars
            .into_iter()
            .flat_map(unligate)
            .filter(|c| !c.is_control())
            .collect();
        let n = chars.len().max(1) as f64;
        let size = (transform * Point::new(0.0, 1000.0)).distance(transform * Point::ORIGIN);
        // A ligature's box is shared evenly by its letters.
        for (i, ch) in chars.into_iter().enumerate() {
            if self.glyphs.len() >= MAX_CHARS {
                return;
            }
            let (a, b) = (advance * i as f64 / n, advance * (i + 1) as f64 / n);
            let corners = [(a, DESCENT), (b, DESCENT), (b, ASCENT), (a, ASCENT)];
            let corners = corners.map(|(x, y)| transform * Point::new(x, y));
            let origin = transform * Point::new(a, 0.0);
            if let Some(bounds) = Bounds::around(corners) {
                self.glyphs.push(Placed {
                    ch,
                    bounds,
                    origin: (origin.x as f32, origin.y as f32),
                    size: size as f32,
                });
            }
        }
    }
}

/// Typeset ligatures as the letters they join, so copies paste plainly.
fn unligate(c: char) -> Vec<char> {
    let letters = match c {
        '\u{FB00}' => "ff",
        '\u{FB01}' => "fi",
        '\u{FB02}' => "fl",
        '\u{FB03}' => "ffi",
        '\u{FB04}' => "ffl",
        '\u{FB05}' | '\u{FB06}' => "st",
        _ => return vec![c],
    };
    letters.chars().collect()
}

impl<'a> Device<'a> for Collector {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}

    // Invisible text is a scan's OCR layer: selectable, so kept.
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        let glyphs = run.glyphs();
        for (i, g) in glyphs.iter().enumerate() {
            let Some(text) = g.as_unicode() else {
                continue;
            };
            let transform = props.transform * g.transform();
            let advance = match &**g {
                Glyph::Outline(o) => o.advance_width().map(f64::from),
                Glyph::Type3(_) => None,
            };
            // Without a width, the next glyph's start bounds this one.
            let next = glyphs.get(i + 1).map(|n| {
                let at = props.transform * n.transform() * Point::ORIGIN;
                (transform.inverse() * at).x
            });
            let advance = advance
                .filter(|w| w.is_finite() && *w > 0.0)
                .or(next.filter(|w| w.is_finite() && *w > 0.0 && *w < 4000.0))
                .unwrap_or(FALLBACK_ADVANCE);
            self.add(transform, advance, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line of text at `y` from `x`, 10 points per letter, size 12.
    fn row(text: &str, x: f32, y: f32) -> Vec<Placed> {
        text.chars()
            .enumerate()
            .map(|(i, ch)| {
                let x0 = x + 10.0 * i as f32;
                Placed {
                    ch,
                    bounds: Bounds {
                        x0,
                        y0: y - 10.0,
                        x1: x0 + 10.0,
                        y1: y + 2.0,
                    },
                    origin: (x0, y),
                    size: 12.0,
                }
            })
            .collect()
    }

    /// `(wraps, text)` lines 20 points apart, as a book sets them.
    fn set(lines: &[(bool, &str)]) -> PageText {
        PageText::from_lines(lines.iter().enumerate().map(|(n, (wraps, text))| {
            let y = 20.0 * n as f32;
            let row = row(text, 0.0, y).into_iter().map(|g| (g.ch, g.bounds));
            (*wraps, row.collect())
        }))
    }

    #[test]
    fn set_lines_copy_wrapped_ones_unbroken() {
        let p = set(&[
            (false, "A long para"),
            (true, "graph goes "),
            (true, "on."),
            (false, "Next"),
        ]);
        assert_eq!(p.text(0..p.len()), "A long paragraph goes on.\nNext");
        assert!(!p.continues());
        // Character 11 starts line two; a word, then the paragraph.
        assert_eq!(p.text(p.word_at(11)), "graph");
        assert_eq!(p.text(p.line_at(13)), "A long paragraph goes on.");
        assert_eq!(p.text(p.line_at(p.len() - 1)), "Next");
        let next = set(&[(true, "going on"), (false, "")]);
        assert!(next.continues());
        assert_eq!(next.len(), 8);
    }

    fn page() -> PageText {
        // Spaces drawn as glyphs in the first line, as gaps in the second.
        let mut glyphs = row("Hello world", 0.0, 20.0);
        glyphs.extend(row("two", 0.0, 40.0));
        glyphs.extend(row("words", 50.0, 40.0));
        PageText::from_glyphs(glyphs)
    }

    #[test]
    fn lines_break_on_new_baselines_and_gaps_become_spaces() {
        let p = page();
        assert_eq!(p.text(0..p.len()), "Hello world\ntwo words");
        assert_eq!(p.lines.len(), 2);
        assert!(p.chars().get(14).is_some_and(|c| c.space_before));
    }

    #[test]
    fn a_caret_lands_by_the_nearest_half_letter() {
        let p = page();
        assert_eq!(p.caret_at(-5.0, 15.0), 0);
        assert_eq!(p.caret_at(13.0, 15.0), 1);
        assert_eq!(p.caret_at(17.0, 15.0), 2);
        // Past a line's end, the caret takes the end of that line.
        assert_eq!(p.caret_at(500.0, 15.0), 11);
        // Above all text is the start; below it, the end.
        assert_eq!(p.caret_at(10.0, -50.0), 0);
        assert_eq!(p.caret_at(10.0, 900.0), p.len());
        // Between lines, the nearer one.
        assert_eq!(p.caret_at(0.0, 27.0), 11);
    }

    #[test]
    fn words_stop_at_spaces_gaps_and_punctuation_and_lines_are_whole() {
        let p = PageText::from_glyphs(row("say: don't", 0.0, 10.0));
        assert_eq!(p.text(p.word_at(1)), "say");
        assert_eq!(p.text(p.word_at(3)), ":");
        assert_eq!(p.text(p.word_at(7)), "don't");
        let p = page();
        assert_eq!(p.text(p.word_at(15)), "words");
        assert_eq!(p.text(p.line_at(13)), "two words");
        assert_eq!(p.word_at(999), 999..999);
    }

    #[test]
    fn a_selection_paints_one_box_per_line() {
        let p = page();
        let boxes = p.boxes(6..14);
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes.first().map(|b| (b.x0, b.x1)), Some((60.0, 110.0)));
        assert!(p.char_at(1.0, 15.0) == Some(0) && p.char_at(300.0, 300.0).is_none());
    }

    #[test]
    fn a_ligature_splits_its_box_among_its_letters() {
        let mut c = Collector::default();
        let t = Affine::scale(0.012);
        c.add(t, 900.0, BfString::String("ffi".into()));
        c.add(t, 500.0, BfString::Char('\u{7}'));
        c.add(t, 600.0, BfString::Char('\u{FB02}'));
        let xs: Vec<_> = c.glyphs.iter().map(|g| (g.ch, g.bounds.x0)).collect();
        assert_eq!(xs.len(), 5);
        let letters: String = c.glyphs.iter().map(|g| g.ch).collect();
        assert_eq!(letters, "ffifl");
        assert!(
            xs.iter()
                .zip([0.0, 3.6, 7.2])
                .all(|(a, b)| (a.1 - b).abs() < 1e-4)
        );
    }
}
