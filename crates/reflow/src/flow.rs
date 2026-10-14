//! Blocks set into lines by parley, then cut into pages line by line.
use std::ops::Range;

use parley::{
    Alignment, AlignmentOptions, FontFamily, FontStyle, FontWeight, Layout, LineHeight,
    StyleProperty,
};

use crate::block::{Block, Emphasis, Role};
use crate::fonts::{Fonts, SANS, SERIF};

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
    /// Space after each block, in body ems.
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

/// One block set at the column width.
pub(crate) struct Set {
    pub(crate) layout: Layout<()>,
    /// Each line's top and bottom in the layout.
    lines: Vec<(f32, f32)>,
    keep_with_next: bool,
    break_before: bool,
}

/// A run of one block's lines on a page; `dy` maps layout to page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Piece {
    pub(crate) block: usize,
    pub(crate) lines: Range<usize>,
    pub(crate) dy: f32,
}

/// A whole book set and paginated; `Send + Sync` for the workers.
pub struct Flow {
    pub(crate) blocks: Vec<Set>,
    pub(crate) pages: Vec<Vec<Piece>>,
}

impl std::fmt::Debug for Flow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Flow({} pages)", self.pages.len())
    }
}

impl Flow {
    /// Sets `blocks` on mareader's A4 page; always at least one page.
    pub fn new(blocks: &[Block], typo: &Typography, fonts: &mut Fonts) -> Self {
        let width = PAGE_WIDTH - 2.0 * PAD;
        let set: Vec<Set> = blocks
            .iter()
            .filter(|b| !b.is_blank())
            .map(|b| set_block(b, typo, width, fonts))
            .collect();
        let gap = typo.block_gap * typo.font_size;
        let pages = paginate(&set, PAGE_HEIGHT - 2.0 * PAD, gap);
        Self { blocks: set, pages }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Every page is the same size: width, height in layout units.
    pub fn page_size(&self) -> (f32, f32) {
        (PAGE_WIDTH, PAGE_HEIGHT)
    }
}

fn set_block(block: &Block, typo: &Typography, width: f32, fonts: &mut Fonts) -> Set {
    let (family, size, line_height, weight) = match block.role {
        Role::Heading(level) => (
            SANS,
            typo.font_size * heading_scale(level),
            HEADING_LINE_HEIGHT,
            HEADING_WEIGHT,
        ),
        Role::Paragraph => (SERIF, typo.font_size, typo.line_height, 400.0),
    };
    let Fonts { fonts, layout } = fonts;
    let mut b = layout.ranged_builder(fonts, &block.text, 1.0, false);
    b.push_default(StyleProperty::FontFamily(FontFamily::named(family)));
    b.push_default(StyleProperty::FontSize(size));
    b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
        line_height,
    )));
    b.push_default(StyleProperty::FontWeight(FontWeight::new(weight)));
    for span in &block.spans {
        let Emphasis { italic, bold } = span.emphasis;
        if italic {
            b.push(
                StyleProperty::FontStyle(FontStyle::Italic),
                span.range.clone(),
            );
        }
        if bold {
            b.push(
                StyleProperty::FontWeight(FontWeight::new(BOLD_WEIGHT)),
                span.range.clone(),
            );
        }
    }
    let mut layout = b.build(&block.text);
    layout.break_all_lines(Some(width));
    layout.align(Alignment::Start, AlignmentOptions::default());
    let lines = layout
        .lines()
        .map(|l| (l.metrics().block_min_coord, l.metrics().block_max_coord))
        .collect();
    Set {
        layout,
        lines,
        keep_with_next: block.role.is_heading(),
        break_before: block.break_before,
    }
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

    /// Places `lines` of block `block` at the cursor.
    fn place(&mut self, block: &Set, index: usize, lines: Range<usize>) {
        let (Some(&(top, _)), Some(&(_, bottom))) = (
            block.lines.get(lines.start),
            lines.end.checked_sub(1).and_then(|l| block.lines.get(l)),
        ) else {
            return;
        };
        self.page.push(Piece {
            block: index,
            lines,
            dy: self.y - top,
        });
        self.y += bottom - top;
    }
}

/// Fills pages line by line; headings never end one, no lone lines.
fn paginate(blocks: &[Set], height: f32, gap: f32) -> Vec<Vec<Piece>> {
    let mut c = Cursor::default();
    for (i, block) in blocks.iter().enumerate() {
        if c.pages.len() >= MAX_PAGES {
            break;
        }
        let total = block.lines.len();
        if total == 0 {
            continue;
        }
        if block.break_before {
            c.break_page();
        }
        if !c.page.is_empty() {
            c.y += gap;
        }
        if block.keep_with_next && !c.page.is_empty() {
            // Room for the heading and the first two lines after it.
            let next = blocks
                .get(i + 1)
                .map_or(0.0, |n| gap + span_height(n, 0..n.lines.len().min(2)));
            if c.y + span_height(block, 0..total) + next > height {
                c.break_page();
            }
        }
        let mut start = 0;
        while start < total {
            let fit = fitting_lines(block, start, height - c.y);
            let mut fit = balance(fit, start, total, c.page.is_empty());
            if fit == 0 && !c.page.is_empty() {
                c.break_page();
                continue;
            }
            // A line taller than a whole page overflows it.
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

fn span_height(block: &Set, lines: Range<usize>) -> f32 {
    let top = block.lines.get(lines.start).map_or(0.0, |l| l.0);
    let bottom = lines
        .end
        .checked_sub(1)
        .and_then(|l| block.lines.get(l))
        .map_or(top, |l| l.1);
    bottom - top
}

/// How many lines from `start` fit in `room`.
fn fitting_lines(block: &Set, start: usize, room: f32) -> usize {
    let Some(&(top, _)) = block.lines.get(start) else {
        return 0;
    };
    block
        .lines
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

    fn para(words: usize) -> Block {
        let mut b = Block::new(Role::Paragraph);
        let text = vec!["word"; words].join(" ");
        b.push(&text, Emphasis::default());
        b
    }

    fn heading(text: &str) -> Block {
        let mut b = Block::new(Role::Heading(1));
        b.push(text, Emphasis::default());
        b
    }

    fn flow(blocks: &[Block]) -> Flow {
        Flow::new(blocks, &Typography::default(), &mut Fonts::bundled())
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
            .flat_map(|p| p.lines.clone())
            .collect();
        let total = f.blocks.first().map_or(0, |b| b.lines.len());
        assert_eq!(lines, (0..total).collect::<Vec<_>>());
    }

    #[test]
    fn pages_never_overflow_the_column() {
        let blocks: Vec<Block> = (0..60).map(|i| para(20 + i * 7)).collect();
        let f = flow(&blocks);
        let room = PAGE_HEIGHT - 2.0 * PAD;
        for page in &f.pages {
            for piece in page {
                let set = f.blocks.get(piece.block);
                let bottom = set
                    .and_then(|s| s.lines.get(piece.lines.end - 1))
                    .map_or(0.0, |l| l.1);
                assert!(bottom + piece.dy <= room + 0.1, "{}", bottom + piece.dy);
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
                let total = f.blocks.get(piece.block).map_or(0, |s| s.lines.len());
                let split = piece.lines.len() < total;
                assert!(
                    !(split && piece.lines.len() == 1),
                    "page {n}: {:?} of {total}",
                    piece.lines
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
}
