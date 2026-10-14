//! Blocks set by parley at their frame's width, then cut into pages.
use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;

use parley::{
    Alignment, AlignmentOptions, FontFamily, FontFamilyName, FontStyle, FontWeight, Layout,
    LineHeight, OverflowWrap, StyleProperty,
};

use crate::block::{Align, Block, Content, Emphasis, Frame, Role, Table, Text};
use crate::fonts::{Fonts, Ink, MONO, SANS, SERIF, Tone};
use crate::picture::{Decoded, Picture};

/// mareader's page: an A4 card at 96 dpi, in layout units.
pub const PAGE_WIDTH: f32 = 794.0;
pub const PAGE_HEIGHT: f32 = 1123.0;
/// The card's inner margin on every side.
pub const PAD: f32 = 72.0;

/// More than this many pages means a broken or hostile book.
pub const MAX_PAGES: usize = 100_000;

/// mareader's defaults; the knobs that change them come later.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Typography {
    /// Body size in layout units (CSS px).
    pub font_size: f32,
    /// A multiple of the font size.
    pub line_height: f32,
    /// Space between blocks, in body ems.
    pub block_gap: f32,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            font_size: 17.0,
            line_height: 1.7,
            block_gap: 1.0,
        }
    }
}

const HEADING_LINE_HEIGHT: f32 = 1.3;
/// Inter's heaviest bundled weight stands in for mareader's 700.
const HEADING_WEIGHT: f32 = 600.0;
const BOLD_WEIGHT: f32 = 700.0;
const HEAD_CELL_WEIGHT: f32 = 600.0;

// Sizes in ems, after mareader's text stylesheet.
const PRE_SCALE: f32 = 0.85;
const PRE_LINE_HEIGHT: f32 = 1.55;
const PRE_PAD: (f32, f32) = (1.1, 0.9);
const CODE_SCALE: f32 = 0.88;
const CELL_SCALE: f32 = 0.95;
const CELL_PAD: (f32, f32) = (0.7, 0.35);
/// Between list items, instead of the paragraph gap.
const TIGHT_GAP: f32 = 0.3;
const RULE_HEIGHT: f32 = 0.5;
/// From a list marker's right edge to the text.
pub(crate) const MARKER_GAP: f32 = 0.5;

/// Each face falls back to Inter for symbols it lacks, like "◦".
const SERIF_FACES: &[FontFamilyName<'static>] =
    &[FontFamilyName::named(SERIF), FontFamilyName::named(SANS)];
const SANS_FACES: &[FontFamilyName<'static>] = &[FontFamilyName::named(SANS)];
const MONO_FACES: &[FontFamilyName<'static>] =
    &[FontFamilyName::named(MONO), FontFamilyName::named(SANS)];

/// A heading's size in body ems, as mareader's stylesheet has it.
fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.7,
        2 => 1.45,
        3 => 1.22,
        4 => 1.08,
        5 => 1.0,
        _ => 0.92,
    }
}

/// How one text layout is set.
#[derive(Clone, Copy)]
struct Style {
    faces: &'static [FontFamilyName<'static>],
    size: f32,
    line_height: f32,
    weight: f32,
    tone: Tone,
    align: Align,
    /// Preformatted: long lines wrap anywhere, code spans aren't resized.
    pre: bool,
}

impl Style {
    fn body(typo: &Typography, frame: &Frame) -> Self {
        Self {
            faces: SERIF_FACES,
            size: typo.font_size,
            line_height: typo.line_height,
            weight: 400.0,
            tone: if frame.soft { Tone::Soft } else { Tone::Body },
            align: frame.align,
            pre: false,
        }
    }

    fn text(role: Role, typo: &Typography, frame: &Frame) -> Self {
        let body = Self::body(typo, frame);
        match role {
            Role::Heading(level) => Self {
                faces: SANS_FACES,
                size: body.size * heading_scale(level),
                line_height: HEADING_LINE_HEIGHT,
                weight: HEADING_WEIGHT,
                tone: if level >= 6 { Tone::Muted } else { body.tone },
                ..body
            },
            Role::Paragraph => body,
            Role::Preformatted => Self {
                faces: MONO_FACES,
                size: body.size * PRE_SCALE,
                line_height: PRE_LINE_HEIGHT,
                align: Align::Start,
                pre: true,
                ..body
            },
        }
    }

    fn cell(typo: &Typography, frame: &Frame, head: bool) -> Self {
        let body = Self::body(typo, frame);
        Self {
            size: body.size * CELL_SCALE,
            weight: if head { HEAD_CELL_WEIGHT } else { body.weight },
            align: Align::Start,
            ..body
        }
    }
}

/// What a set block draws; positions are relative to the block.
pub(crate) enum Draw {
    Text {
        layout: Box<Layout<Ink>>,
        /// A list marker, hung left of the first line.
        marker: Option<Box<Layout<Ink>>>,
        /// Inner padding left of the text, for shaded blocks.
        pad: f32,
        shaded: bool,
    },
    Rule,
    Image {
        picture: Arc<Picture>,
        rect: [f32; 4],
    },
    Table(Grid),
}

/// A table set in equal columns; row `i` spans `units[i]`.
pub(crate) struct Grid {
    pub(crate) column: f32,
    /// Cell padding: across, down.
    pub(crate) pad: [f32; 2],
    pub(crate) rows: Vec<(Vec<Layout<Ink>>, bool)>,
}

/// One block set: `x` and `width` within the column.
pub(crate) struct Set {
    pub(crate) x: f32,
    pub(crate) width: f32,
    /// Quote bars' left edges within the column.
    pub(crate) bars: Vec<f32>,
    pub(crate) draw: Draw,
    /// Each unit's top and bottom: lines, rows or the whole image.
    pub(crate) units: Vec<(f32, f32)>,
    keep_with_next: bool,
    break_before: bool,
    tight: bool,
    /// Text: avoid lone lines at a split.
    lines: bool,
    /// A table's leading head rows, never left alone at a page's foot.
    heads: usize,
}

/// A run of one block's units on a page; `dy` maps block to page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Piece {
    pub(crate) block: usize,
    pub(crate) units: Range<usize>,
    pub(crate) dy: f32,
}

/// A whole book set and paginated; `Send + Sync` for the workers.
pub struct Flow {
    pub(crate) blocks: Vec<Set>,
    pub(crate) pages: Vec<Vec<Piece>>,
    pub(crate) decoded: Decoded,
}

impl std::fmt::Debug for Flow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Flow({} pages)", self.pages.len())
    }
}

impl Flow {
    /// Sets `blocks` on mareader's A4 page; always at least one page.
    pub fn new(blocks: &[Block], typo: &Typography, fonts: &mut Fonts) -> Self {
        let column = PAGE_WIDTH - 2.0 * PAD;
        let height = PAGE_HEIGHT - 2.0 * PAD;
        let set: Vec<Set> = blocks
            .iter()
            .filter(|b| !b.is_blank())
            .map(|b| set_block(b, typo, column, height, fonts))
            .collect();
        let pages = paginate(&set, height, typo);
        Self {
            blocks: set,
            pages,
            decoded: Decoded::default(),
        }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Every page is the same size: width, height in layout units.
    pub fn page_size(&self) -> (f32, f32) {
        (PAGE_WIDTH, PAGE_HEIGHT)
    }
}

fn alignment(align: Align) -> Alignment {
    match align {
        Align::Start => Alignment::Start,
        Align::Center => Alignment::Center,
        Align::End => Alignment::End,
    }
}

fn layout_text(fonts: &mut Fonts, text: &Text, style: Style, width: Option<f32>) -> Layout<Ink> {
    let Fonts { fonts, layout } = fonts;
    let ink = Ink {
        tone: style.tone,
        code: false,
    };
    let mut b = layout.ranged_builder(fonts, &text.text, 1.0, false);
    b.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Borrowed(
        style.faces,
    ))));
    b.push_default(StyleProperty::FontSize(style.size));
    b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
        style.line_height,
    )));
    b.push_default(StyleProperty::FontWeight(FontWeight::new(style.weight)));
    b.push_default(StyleProperty::Brush(ink));
    if style.pre {
        b.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
    }
    for span in &text.spans {
        let range = span.range.clone();
        let Emphasis { italic, bold, code } = span.emphasis;
        if italic {
            b.push(StyleProperty::FontStyle(FontStyle::Italic), range.clone());
        }
        if bold {
            let weight = FontWeight::new(BOLD_WEIGHT);
            b.push(StyleProperty::FontWeight(weight), range.clone());
        }
        if code && !style.pre {
            let mono = FontFamily::List(Cow::Borrowed(MONO_FACES));
            b.push(StyleProperty::FontFamily(mono), range.clone());
            b.push(
                StyleProperty::FontSize(style.size * CODE_SCALE),
                range.clone(),
            );
            b.push(StyleProperty::Brush(Ink { code: true, ..ink }), range);
        }
    }
    let mut layout = b.build(&text.text);
    layout.break_all_lines(width);
    layout.align(alignment(style.align), AlignmentOptions::default());
    layout
}

fn line_units(layout: &Layout<Ink>) -> Vec<(f32, f32)> {
    layout
        .lines()
        .map(|l| (l.metrics().block_min_coord, l.metrics().block_max_coord))
        .collect()
}

fn set_block(block: &Block, typo: &Typography, column: f32, height: f32, f: &mut Fonts) -> Set {
    let em = typo.font_size;
    let frame = &block.frame;
    // Deep nesting stops indenting at half the column.
    let x = (frame.inset * em).clamp(0.0, column / 2.0);
    let width = column - x;
    let (draw, units, lines) = match &block.content {
        Content::Text(text) => {
            let (draw, units) = set_text(text, frame, typo, width, f);
            (draw, units, true)
        }
        Content::Rule => (Draw::Rule, vec![(0.0, RULE_HEIGHT * em)], false),
        Content::Image { picture, cover } => {
            let (rect, unit) = if *cover {
                cover_rect(picture, column, height)
            } else {
                image_rect(picture, frame.align, width, height)
            };
            let picture = picture.clone();
            (Draw::Image { picture, rect }, vec![unit], false)
        }
        Content::Table(table) => {
            let (grid, units) = set_table(table, frame, typo, width, f);
            (Draw::Table(grid), units, false)
        }
    };
    let cover = matches!(block.content, Content::Image { cover: true, .. });
    let heads = match &block.content {
        Content::Table(t) => t.rows.iter().take_while(|r| r.head).count(),
        _ => 0,
    };
    Set {
        x: if cover { 0.0 } else { x },
        width: if cover { column } else { width },
        bars: frame
            .bars
            .iter()
            .map(|b| (b * em).min(column / 2.0))
            .collect(),
        draw,
        units,
        keep_with_next: block.is_heading(),
        break_before: block.break_before,
        tight: frame.tight,
        lines,
        heads,
    }
}

fn set_text(
    text: &Text,
    frame: &Frame,
    typo: &Typography,
    width: f32,
    fonts: &mut Fonts,
) -> (Draw, Vec<(f32, f32)>) {
    let em = typo.font_size;
    let style = Style::text(text.role, typo, frame);
    let shaded = text.role == Role::Preformatted;
    let (pad, pad_y) = if shaded {
        (PRE_PAD.0 * em, PRE_PAD.1 * em)
    } else {
        (0.0, 0.0)
    };
    let inner = (width - 2.0 * pad).max(em);
    let layout = layout_text(fonts, text, style, Some(inner));
    let mut units = line_units(&layout);
    // The padding rides on the first and last lines.
    if let Some(first) = units.first_mut() {
        first.0 -= pad_y;
    }
    if let Some(last) = units.last_mut() {
        last.1 += pad_y;
    }
    let marker = frame.marker.as_deref().map(|m| {
        let mut t = Text::new(Role::Paragraph);
        t.push(m, Emphasis::default());
        let style = Style {
            align: Align::Start,
            pre: false,
            ..style
        };
        Box::new(layout_text(fonts, &t, style, None))
    });
    let draw = Draw::Text {
        layout: Box::new(layout),
        marker,
        pad,
        shaded,
    };
    (draw, units)
}

/// At its own pixel size, shrunk to the column and the page.
fn image_rect(picture: &Picture, align: Align, width: f32, height: f32) -> ([f32; 4], (f32, f32)) {
    let aspect = picture.aspect();
    let mut w = (picture.width as f32).min(width);
    let mut h = w * aspect;
    if h > height {
        h = height;
        w = h / aspect;
    }
    let x = match align {
        Align::Start => 0.0,
        Align::Center => (width - w) / 2.0,
        Align::End => width - w,
    };
    ([x, 0.0, w, h], (0.0, h))
}

/// A cover fills the page box, centred, scaled up if need be.
fn cover_rect(picture: &Picture, width: f32, height: f32) -> ([f32; 4], (f32, f32)) {
    let aspect = picture.aspect();
    let w = width.min(height / aspect);
    let h = w * aspect;
    let rect = [(width - w) / 2.0, (height - h) / 2.0, w, h];
    (rect, (0.0, height))
}

fn set_table(
    table: &Table,
    frame: &Frame,
    typo: &Typography,
    width: f32,
    fonts: &mut Fonts,
) -> (Grid, Vec<(f32, f32)>) {
    let em = typo.font_size;
    let columns = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(1);
    let column = width / columns.max(1) as f32;
    let (pad_x, pad_y) = (CELL_PAD.0 * em, CELL_PAD.1 * em);
    let inner = (column - 2.0 * pad_x).max(em);
    let mut units = Vec::with_capacity(table.rows.len());
    let mut rows = Vec::with_capacity(table.rows.len());
    let mut y = 0.0;
    for row in &table.rows {
        let style = Style::cell(typo, frame, row.head);
        let cells: Vec<Layout<Ink>> = row
            .cells
            .iter()
            .map(|c| layout_text(fonts, c, style, Some(inner)))
            .collect();
        let h = cells.iter().map(Layout::height).fold(0.0, f32::max) + 2.0 * pad_y;
        units.push((y, y + h));
        y += h;
        rows.push((cells, row.head));
    }
    let pad = [pad_x, pad_y];
    (Grid { column, pad, rows }, units)
}

/// The page being filled: its pieces and how far down it is.
#[derive(Default)]
struct Cursor {
    pages: Vec<Vec<Piece>>,
    page: Vec<Piece>,
    y: f32,
}

impl Cursor {
    fn break_page(&mut self) {
        if !self.page.is_empty() {
            self.pages.push(std::mem::take(&mut self.page));
        }
        self.y = 0.0;
    }

    /// Places `units` of block `block` at the cursor.
    fn place(&mut self, block: &Set, index: usize, units: Range<usize>) {
        let (Some(&(top, _)), Some(&(_, bottom))) = (
            block.units.get(units.start),
            units.end.checked_sub(1).and_then(|u| block.units.get(u)),
        ) else {
            return;
        };
        self.page.push(Piece {
            block: index,
            units,
            dy: self.y - top,
        });
        self.y += bottom - top;
    }
}

fn gap(block: &Set, typo: &Typography) -> f32 {
    let ems = if block.tight {
        TIGHT_GAP
    } else {
        typo.block_gap
    };
    ems * typo.font_size
}

/// Fills pages unit by unit; headings never end one, no lone lines.
fn paginate(blocks: &[Set], height: f32, typo: &Typography) -> Vec<Vec<Piece>> {
    let mut c = Cursor::default();
    for (i, block) in blocks.iter().enumerate() {
        if c.pages.len() >= MAX_PAGES {
            break;
        }
        let total = block.units.len();
        if total == 0 {
            continue;
        }
        if block.break_before {
            c.break_page();
        }
        if !c.page.is_empty() {
            c.y += gap(block, typo);
        }
        if block.keep_with_next && !c.page.is_empty() {
            // Room for the heading and the first two units after it.
            let next = blocks.get(i + 1).map_or(0.0, |n| {
                gap(n, typo) + span_height(n, 0..n.units.len().min(2))
            });
            if c.y + span_height(block, 0..total) + next > height {
                c.break_page();
            }
        }
        let mut start = 0;
        while start < total {
            let fit = fitting_units(block, start, height - c.y);
            let mut fit = if block.lines {
                balance(fit, start, total, c.page.is_empty())
            } else if start + fit <= block.heads && fit < total && !c.page.is_empty() {
                // Head rows go on with the first body row.
                0
            } else {
                fit
            };
            if fit == 0 && !c.page.is_empty() {
                c.break_page();
                continue;
            }
            // A unit taller than a whole page overflows it.
            fit = fit.max(1);
            c.place(block, i, start..start + fit);
            start += fit;
            if start < total {
                c.break_page();
            }
        }
    }
    c.break_page();
    if c.pages.is_empty() {
        c.pages.push(Vec::new());
    }
    c.pages
}

pub(crate) fn span_height(block: &Set, units: Range<usize>) -> f32 {
    let top = block.units.get(units.start).map_or(0.0, |u| u.0);
    let bottom = units
        .end
        .checked_sub(1)
        .and_then(|u| block.units.get(u))
        .map_or(top, |u| u.1);
    bottom - top
}

/// How many units from `start` fit in `room`.
fn fitting_units(block: &Set, start: usize, room: f32) -> usize {
    let Some(&(top, _)) = block.units.get(start) else {
        return 0;
    };
    block
        .units
        .iter()
        .skip(start)
        .take_while(|(_, bottom)| bottom - top <= room + 0.01)
        .count()
}

/// Trims a fit to avoid a lone line; zero moves the block on.
fn balance(fit: usize, start: usize, total: usize, page_empty: bool) -> usize {
    let rest = total - start;
    if fit >= rest {
        return rest;
    }
    // Its last line alone atop the next page: take one more along.
    if rest - fit == 1 && fit >= 3 {
        return fit - 1;
    }
    if page_empty {
        return fit;
    }
    // A lone first line at a page's foot, or no good split: move on.
    if fit < 2 || rest - fit == 1 {
        return 0;
    }
    fit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::Row;
    use crate::picture::tests::png;

    fn para(words: usize) -> Block {
        Block::text(Role::Paragraph, &vec!["word"; words].join(" "))
    }

    fn heading(text: &str) -> Block {
        Block::text(Role::Heading(1), text)
    }

    fn picture(w: u32, h: u32) -> Arc<Picture> {
        let p = Picture::probe(png(w, h)).map(Arc::new);
        assert!(p.is_ok(), "probe");
        p.unwrap_or_else(|_| Arc::new(Picture::empty()))
    }

    fn image(w: u32, h: u32, cover: bool) -> Block {
        let picture = picture(w, h);
        Block::new(Content::Image { picture, cover })
    }

    fn flow(blocks: &[Block]) -> Flow {
        Flow::new(blocks, &Typography::default(), &mut Fonts::bundled())
    }

    fn room() -> f32 {
        PAGE_HEIGHT - 2.0 * PAD
    }

    #[test]
    fn an_empty_book_has_one_blank_page() {
        let f = flow(&[]);
        assert_eq!(f.page_count(), 1);
    }

    #[test]
    fn long_paragraphs_split_between_lines() {
        let f = flow(&[para(4000)]);
        assert!(f.page_count() > 3, "{}", f.page_count());
        // Every line placed once, in order.
        let lines: Vec<usize> = f
            .pages
            .iter()
            .flatten()
            .flat_map(|p| p.units.clone())
            .collect();
        let total = f.blocks.first().map_or(0, |b| b.units.len());
        assert_eq!(lines, (0..total).collect::<Vec<_>>());
    }

    #[test]
    fn pages_never_overflow_the_column() {
        let mut blocks: Vec<Block> = (0..60).map(|i| para(20 + i * 7)).collect();
        blocks.insert(9, image(1200, 3000, false));
        let f = flow(&blocks);
        for page in &f.pages {
            for piece in page {
                let set = f.blocks.get(piece.block);
                let bottom = set
                    .and_then(|s| s.units.get(piece.units.end - 1))
                    .map_or(0.0, |u| u.1);
                assert!(bottom + piece.dy <= room() + 0.1, "{}", bottom + piece.dy);
            }
        }
    }

    #[test]
    fn a_heading_never_ends_a_page() {
        let mut blocks = Vec::new();
        for i in 0..40 {
            blocks.push(heading(&format!("Chapter {i}")));
            blocks.push(para(90 + i * 13));
        }
        let f = flow(&blocks);
        for page in &f.pages {
            let last = page.last().and_then(|p| f.blocks.get(p.block));
            assert!(last.is_some_and(|s| !s.keep_with_next));
        }
    }

    #[test]
    fn no_page_holds_a_lone_line_of_a_split_paragraph() {
        let blocks: Vec<Block> = (0..50).map(|i| para(30 + i * 11)).collect();
        let f = flow(&blocks);
        for (n, page) in f.pages.iter().enumerate() {
            for piece in page {
                let total = f.blocks.get(piece.block).map_or(0, |s| s.units.len());
                let split = piece.units.len() < total;
                assert!(
                    !(split && piece.units.len() == 1),
                    "page {n}: {:?} of {total}",
                    piece.units
                );
            }
        }
    }

    #[test]
    fn a_chapter_starts_a_new_page() {
        let mut second = para(5);
        second.break_before = true;
        let f = flow(&[para(5), second, para(5)]);
        let blocks: Vec<Vec<usize>> = f
            .pages
            .iter()
            .map(|p| p.iter().map(|piece| piece.block).collect())
            .collect();
        assert_eq!(blocks, [vec![0], vec![1, 2]]);
    }

    #[test]
    fn lines_of_a_piece_sit_where_the_layout_put_them() {
        let f = flow(&[para(300)]);
        let first = f.pages.first().and_then(|p| p.first());
        assert_eq!(first.map(|p| p.dy), Some(0.0));
    }

    /// Each block's top on page one.
    fn tops(f: &Flow) -> Vec<f32> {
        let page = f.pages.first().cloned().unwrap_or_default();
        page.iter()
            .map(|p| {
                let top = f.blocks.get(p.block).and_then(|s| s.units.first());
                top.map_or(0.0, |u| u.0) + p.dy
            })
            .collect()
    }

    #[test]
    fn list_items_sit_closer_than_paragraphs() {
        let item = |tight| {
            let mut b = para(3);
            b.frame = Frame {
                inset: 1.6,
                marker: Some("•".into()),
                tight,
                ..Frame::default()
            };
            b
        };
        let loose = tops(&flow(&[para(3), para(3)]));
        let tight = tops(&flow(&[item(false), item(true)]));
        let step = |t: &[f32]| t.get(1).zip(t.first()).map(|(b, a)| b - a);
        assert!(step(&tight) < step(&loose), "{tight:?} {loose:?}");
        let f = flow(&[item(false)]);
        let set = f.blocks.first();
        assert_eq!(set.map(|s| s.x), Some(1.6 * 17.0));
        assert!(set.is_some_and(|s| matches!(
            &s.draw,
            Draw::Text {
                marker: Some(_),
                ..
            }
        )));
    }

    #[test]
    fn images_shrink_to_the_column_and_the_page_never_split() {
        let f = flow(&[
            image(100, 50, false),
            image(4000, 1000, false),
            image(500, 9000, false),
        ]);
        let rect = |i: usize| match f.blocks.get(i).map(|s| &s.draw) {
            Some(Draw::Image { rect, .. }) => *rect,
            _ => [0.0; 4],
        };
        // Small images keep their size; wide ones fit the column.
        assert_eq!(rect(0), [0.0, 0.0, 100.0, 50.0]);
        let column = PAGE_WIDTH - 2.0 * PAD;
        assert!((rect(1)[2] - column).abs() < 0.01, "{:?}", rect(1));
        // Tall ones fit the page, alone on it.
        assert!((rect(2)[3] - room()).abs() < 0.01, "{:?}", rect(2));
        assert_eq!(f.page_count(), 2);
    }

    #[test]
    fn a_cover_fills_its_own_page() {
        let mut next = para(10);
        next.break_before = true;
        let f = flow(&[image(400, 900, true), next]);
        assert_eq!(f.page_count(), 2);
        let rect = match f.blocks.first().map(|s| &s.draw) {
            Some(Draw::Image { rect, .. }) => *rect,
            _ => [0.0; 4],
        };
        // Scaled up to the page box's height, centred across.
        assert!((rect[3] - room()).abs() < 0.01, "{rect:?}");
        let column = PAGE_WIDTH - 2.0 * PAD;
        assert!((rect[0] * 2.0 + rect[2] - column).abs() < 0.01, "{rect:?}");
    }

    #[test]
    fn tables_split_between_rows() {
        let cell = |s: &str| {
            let mut t = Text::new(Role::Paragraph);
            t.push(s, Emphasis::default());
            t
        };
        let rows = (0..120)
            .map(|i| Row {
                cells: vec![cell(&format!("row {i}")), cell("a longer second cell")],
                head: i == 0,
            })
            .collect();
        let f = flow(&[Block::new(Content::Table(Table { rows }))]);
        assert!(f.page_count() >= 2, "{}", f.page_count());
        let rows: Vec<usize> = f
            .pages
            .iter()
            .flatten()
            .flat_map(|p| p.units.clone())
            .collect();
        assert_eq!(rows, (0..120).collect::<Vec<_>>());
    }

    #[test]
    fn a_table_head_never_ends_a_page_alone() {
        let cell = |s: &str| {
            let mut t = Text::new(Role::Paragraph);
            t.push(s, Emphasis::default());
            t
        };
        let row = |head| Row {
            cells: vec![cell("cell"), cell("cell")],
            head,
        };
        // Some lead-in length leaves room for only the head row.
        for words in (200..900).step_by(9) {
            let rows = vec![row(true), row(false), row(false)];
            let table = Block::new(Content::Table(Table { rows }));
            let f = flow(&[para(words), table]);
            for page in &f.pages {
                let last = page.last();
                let alone = last.is_some_and(|p| p.block == 1 && p.units == (0..1));
                assert!(!alone, "{words} words: {:?}", f.pages);
            }
        }
    }

    #[test]
    fn preformatted_text_keeps_its_lines_and_padding() {
        let mut t = Text::new(Role::Preformatted);
        t.push("fn main() {\n    run();\n}", Emphasis::default());
        let f = flow(&[Block::new(Content::Text(t))]);
        let set = f.blocks.first();
        assert_eq!(set.map(|s| s.units.len()), Some(3));
        // The first unit starts above the first line by the padding.
        let first = set.and_then(|s| s.units.first()).map_or(0.0, |u| u.0);
        assert!((first + PRE_PAD.1 * 17.0).abs() < 0.01, "{first}");
    }
}
