//! Blocks set by parley at their frame's width, then cut into pages.
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use hypher::Lang;
use parley::{
    Affinity, Alignment, AlignmentOptions, Cursor as Caret, FontFamily, FontFamilyName, FontStyle,
    FontWeight, GenericFamily, IndentOptions, Layout, LineHeight, OverflowWrap, Selection,
    StyleProperty,
};

use crate::block::{Align, Block, Content, Emphasis, Frame, Link, Role, Table, Text};
use crate::fonts::{Faces, Family, Fonts, Ink, Kind, SANS, Tone};
use crate::hyphen::{self, SHY};
use crate::picture::{Decoded, Picture};

/// mareader's page: an A4 card at 96 dpi, in layout units.
pub const PAGE_WIDTH: f32 = 794.0;
pub const PAGE_HEIGHT: f32 = 1123.0;
/// The card's inner margin on every side.
pub const PAD: f32 = 72.0;

/// Book layout's inner margin, toward the spine, and its outer one.
const GUTTER: f32 = 92.0;
const EDGE: f32 = 56.0;

/// More than this many pages means a broken or hostile book.
pub const MAX_PAGES: usize = 100_000;

/// How a book is set: the reader's typography plus the text's language.
#[derive(Debug, Clone, PartialEq)]
pub struct Typography {
    /// Body size in layout units (CSS px).
    pub font_size: f32,
    /// A multiple of the font size.
    pub line_height: f32,
    /// Space between blocks, in body ems.
    pub block_gap: f32,
    /// Body weight, 100 to 900; headings keep their own.
    pub weight: f32,
    /// Extra space between words, in layout units.
    pub word_spacing: f32,
    /// Extra space between letters, in ems.
    pub letter_spacing: f32,
    /// A paragraph's first-line indent, in ems.
    pub indent: f32,
    pub justify: bool,
    pub hyphenate: bool,
    /// Each column moves toward the spine: right on even pages.
    pub book_layout: bool,
    /// The column's width as a multiple of the page's own; the page follows.
    pub column_scale: f32,
    /// A BCP 47 tag; it picks the hyphenation patterns.
    pub language: String,
    pub faces: Faces,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            font_size: 17.0,
            line_height: 1.7,
            block_gap: 1.0,
            weight: 400.0,
            word_spacing: 0.0,
            letter_spacing: 0.0,
            indent: 0.0,
            justify: false,
            hyphenate: false,
            book_layout: false,
            column_scale: 1.0,
            language: "en".to_owned(),
            faces: Faces::default(),
        }
    }
}

const HEADING_LINE_HEIGHT: f32 = 1.3;
/// Inter's heaviest bundled weight stands in for mareader's 700.
const HEADING_WEIGHT: f32 = 600.0;
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

/// CSS `bolder`: what bold text weighs inside text of `weight`.
fn bolder(weight: f32) -> f32 {
    if weight < 350.0 {
        400.0
    } else if weight < 550.0 {
        700.0
    } else {
        900.0
    }
}

type Stack = Vec<FontFamilyName<'static>>;

/// Each stack as parley takes it, with Inter last for symbols.
struct Stacks {
    body: Stack,
    heading: Stack,
    mono: Stack,
}

impl Stacks {
    fn new(faces: &Faces) -> Self {
        Self {
            body: stack(&faces.body),
            heading: stack(&faces.heading),
            mono: stack(&faces.mono),
        }
    }
}

fn stack(families: &[Family]) -> Stack {
    let mut names: Stack = families
        .iter()
        .map(|f| match f {
            Family::Named(name) => FontFamilyName::Named(Cow::Owned(name.clone())),
            Family::Generic(kind) => FontFamilyName::Generic(match kind {
                Kind::Serif => GenericFamily::Serif,
                Kind::Sans => GenericFamily::SansSerif,
                Kind::Mono => GenericFamily::Monospace,
            }),
        })
        .collect();
    let symbols = FontFamilyName::named(SANS);
    if !names.contains(&symbols) {
        names.push(symbols);
    }
    names
}

/// How one text layout is set.
#[derive(Clone, Copy)]
struct Style<'a> {
    faces: &'a [FontFamilyName<'static>],
    /// Inline code's stack.
    mono: &'a [FontFamilyName<'static>],
    size: f32,
    line_height: f32,
    weight: f32,
    tone: Tone,
    align: Align,
    /// Preformatted: long lines wrap anywhere, code spans aren't resized.
    pre: bool,
    justify: bool,
    hyphens: Option<Lang>,
    /// The first line's indent, in layout units.
    indent: f32,
    /// Extra advance per letter and per word, in layout units.
    letter: f32,
    word: f32,
}

/// A text layout, its soft-hyphen lines, and line starts in source bytes.
pub(crate) struct Para {
    pub(crate) layout: Layout<Ink>,
    /// The text as set, soft hyphens included.
    pub(crate) text: String,
    pub(crate) hyphens: Vec<usize>,
    /// Where soft hyphens were put in, ascending; see `Hyphenated`.
    pub(crate) marks: Vec<usize>,
    starts: Vec<usize>,
    /// Linked runs, in bytes of the text as set.
    links: Vec<Link>,
}

/// What a set block draws; positions are relative to the block.
pub(crate) enum Draw {
    Text {
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
    /// Which rows are head rows.
    pub(crate) heads: Vec<bool>,
}

/// A block's text layouts: the bulk of a set book, so laid on demand.
pub(crate) enum Laid {
    Text {
        para: Box<Para>,
        /// A list marker, hung left of the first line.
        marker: Option<Box<Para>>,
    },
    /// Each row's cells.
    Table(Vec<Vec<Para>>),
    None,
}

/// The blocks laid most recently, up to [`MAX_LAID`] estimated bytes.
#[derive(Default)]
struct Recent {
    /// Each block's layouts, when last used, and estimated bytes.
    laid: HashMap<usize, (Arc<Laid>, u64, usize)>,
    tick: u64,
    bytes: usize,
}

impl Recent {
    fn get(&mut self, block: usize) -> Option<Arc<Laid>> {
        self.tick += 1;
        let entry = self.laid.get_mut(&block)?;
        entry.1 = self.tick;
        Some(Arc::clone(&entry.0))
    }

    fn put(&mut self, block: usize, laid: Arc<Laid>, bytes: usize) {
        self.tick += 1;
        if let Some(old) = self.laid.insert(block, (laid, self.tick, bytes)) {
            self.bytes -= old.2;
        }
        self.bytes += bytes;
        if self.bytes <= MAX_LAID {
            return;
        }
        // Over: drop the oldest down to half, so eviction is rare.
        let mut ages: Vec<(u64, usize)> = self.laid.iter().map(|(&b, e)| (e.1, b)).collect();
        ages.sort_unstable();
        for (_, old) in ages {
            if self.bytes <= MAX_LAID / 2 || old == block {
                break;
            }
            if let Some(gone) = self.laid.remove(&old) {
                self.bytes -= gone.2;
            }
        }
    }
}

/// Estimated bytes of layouts a book keeps; the rest are laid again.
pub const MAX_LAID: usize = 32 << 20;

/// One block set: `x` and `width` within the column.
pub(crate) struct Set {
    /// The block's index in the book, blank ones counted.
    pub(crate) source: usize,
    pub(crate) x: f32,
    pub(crate) width: f32,
    /// Quote bars' left edges within the column.
    pub(crate) bars: Vec<f32>,
    pub(crate) draw: Draw,
    /// Each unit's top and bottom: lines, rows or the whole image.
    pub(crate) units: Vec<(f32, f32)>,
    /// Each unit's place in the block: a text offset or a row.
    starts: Vec<usize>,
    keep_with_next: bool,
    break_before: bool,
    tight: bool,
    /// Text: avoid lone lines at a split.
    lines: bool,
    /// A table's leading head rows, never left alone at a page's foot.
    heads: usize,
}

/// Each unit's top and bottom within its block.
type Units = Vec<(f32, f32)>;

/// A run of one block's units on a page; `dy` maps block to page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Piece {
    pub(crate) block: usize,
    pub(crate) units: Range<usize>,
    pub(crate) dy: f32,
}

/// A place in the book that survives re-setting it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Anchor {
    /// The block's index in the book.
    pub block: usize,
    /// A byte offset into its text, or a table row.
    pub offset: usize,
}

/// A link's box on one line of a page, in page units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageLink {
    pub target: u32,
    /// Left, top, width, height.
    pub rect: [f32; 4],
}

/// A page's part in a stream: its column, cut to text and gap above.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A book paginated, keeping line boxes; `Send + Sync` for the workers.
pub struct Flow {
    pub(crate) blocks: Vec<Set>,
    pub(crate) pages: Vec<Vec<Piece>>,
    pub(crate) bands: Vec<Band>,
    pub(crate) book_layout: bool,
    /// Pages before its first, which decide each page's spread side.
    shift: usize,
    /// The text column's width.
    width: f32,
    pub(crate) decoded: Arc<Decoded>,
    /// Boxed: fonts and type are kilobytes, and books move.
    relay: Box<Relay>,
}

/// The blocks, type and fonts a book was set in, to lay it again.
struct Relay {
    source: Arc<Vec<Block>>,
    typo: Typography,
    /// The page's height as set, so blocks are laid again alike.
    height: f32,
    stacks: Stacks,
    fonts: Mutex<Fonts>,
    recent: Mutex<Recent>,
}

impl std::fmt::Debug for Flow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Flow({} pages)", self.pages.len())
    }
}

impl Flow {
    /// Sets `blocks` on mareader's A4 page; always at least one page.
    pub fn new(blocks: Arc<Vec<Block>>, typo: &Typography, fonts: Fonts) -> Self {
        let room = (column_width(typo), PAGE_HEIGHT - 2.0 * PAD);
        Self::set(blocks, typo, fonts, room, room.1)
    }

    /// Sets `blocks` `width` wide, cut every `height`; tall never cuts.
    pub fn sized(
        blocks: Arc<Vec<Block>>,
        typo: &Typography,
        fonts: Fonts,
        (width, height): (f32, f32),
        tall: bool,
    ) -> Self {
        let typo = Typography {
            book_layout: false,
            ..typo.clone()
        };
        let room = (width.max(MIN_COLUMN), height.max(1.0));
        let cut = if tall { f32::INFINITY } else { room.1 };
        Self::set(blocks, &typo, fonts, room, cut)
    }

    fn set(
        blocks: Arc<Vec<Block>>,
        typo: &Typography,
        mut fonts: Fonts,
        (width, height): (f32, f32),
        cut: f32,
    ) -> Self {
        let stacks = Stacks::new(&typo.faces);
        // Each block is laid to measure its lines, then let go.
        let set: Vec<Set> = {
            let mut setter = Setter::new(typo, &stacks, &mut fonts);
            (setter.column, setter.height) = (width, height);
            blocks
                .iter()
                .enumerate()
                .filter(|(_, b)| !b.is_blank())
                .map(|(i, b)| setter.block(i, b).0)
                .collect()
        };
        let pages = paginate(&set, cut, typo);
        let mut flow = Self {
            blocks: set,
            pages,
            bands: Vec::new(),
            book_layout: typo.book_layout,
            shift: 0,
            width,
            decoded: Arc::default(),
            relay: Box::new(Relay {
                source: blocks,
                typo: typo.clone(),
                height,
                stacks,
                fonts: Mutex::new(fonts),
                recent: Mutex::default(),
            }),
        };
        flow.bands = (0..flow.pages.len()).map(|n| flow.band(n, typo)).collect();
        flow
    }

    /// `blocks` in this type for `replaced` pages, ending a spread evenly.
    pub fn region(
        &self,
        blocks: Arc<Vec<Block>>,
        language: Option<&str>,
        fonts: Fonts,
        (first, replaced): (usize, usize),
    ) -> Self {
        let relay = &*self.relay;
        let mut typo = relay.typo.clone();
        if let Some(tag) = language {
            tag.clone_into(&mut typo.language);
        }
        let room = (self.width, relay.height);
        let mut flow = Self::set(blocks, &typo, fonts, room, room.1);
        flow.shift = first % 2;
        // An odd change in count would swap every later page's side.
        if flow.book_layout && (flow.pages.len() + replaced) % 2 == 1 {
            flow.pages.push(Vec::new());
        }
        flow.bands = (0..flow.pages.len()).map(|n| flow.band(n, &typo)).collect();
        flow.keep_pictures(self)
    }

    /// Set block `block`'s layouts, laid again if not kept.
    pub(crate) fn laid(&self, block: usize) -> Arc<Laid> {
        let relay = &*self.relay;
        let recent = || relay.recent.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(laid) = recent().get(block) {
            return laid;
        }
        let set = self.blocks.get(block);
        let set = set.filter(|s| matches!(s.draw, Draw::Text { .. } | Draw::Table(_)));
        let Some((set, source)) = set.and_then(|s| Some((s, relay.source.get(s.source)?))) else {
            return Arc::new(Laid::None);
        };
        let laid = {
            let mut fonts = relay.fonts.lock().unwrap_or_else(PoisonError::into_inner);
            let mut setter = Setter::new(&relay.typo, &relay.stacks, &mut fonts);
            // The column it was measured in, not the typography's own.
            (setter.column, setter.height) = (self.width, relay.height);
            Arc::new(setter.block(set.source, source).1)
        };
        recent().put(block, Arc::clone(&laid), source.layout_cost());
        laid
    }

    /// Page `page`'s column: its left edge and width.
    pub fn column(&self, page: usize) -> (f32, f32) {
        let x = match (self.book_layout, (page + self.shift) % 2) {
            (false, _) => PAD,
            // Spreads pair pages from the first: even ones sit left.
            (true, 0) => EDGE,
            (true, _) => GUTTER,
        };
        (x, self.width)
    }

    /// Every page's band, for a stream that reads like one long page.
    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    fn band(&self, page: usize, typo: &Typography) -> Band {
        let pieces = self.pages.get(page).map_or(&[][..], Vec::as_slice);
        let used = pieces
            .iter()
            .filter_map(|p| {
                let set = self.blocks.get(p.block)?;
                let last = set.units.get(p.units.end.checked_sub(1)?)?;
                Some(p.dy + last.1)
            })
            .fold(0.0, f32::max);
        // The gap a page cut swallowed; a chapter gets a pad's worth.
        let lead = match pieces.first() {
            _ if page == 0 => PAD,
            Some(p) if p.units.start == 0 => self
                .blocks
                .get(p.block)
                .map_or(0.0, |b| if b.break_before { PAD } else { gap(b, typo) }),
            _ => 0.0,
        }
        .min(PAD);
        let foot = if page + 1 == self.pages.len() {
            PAD
        } else {
            0.0
        };
        let (x, width) = self.column(page);
        let y = PAD - lead;
        Band {
            x,
            y,
            width,
            height: (lead + used + foot).min(PAGE_HEIGHT - y),
        }
    }

    /// Shares `other`'s decoded pictures: a re-set book shows the same.
    pub fn keep_pictures(mut self, other: &Flow) -> Self {
        self.decoded = Arc::clone(&other.decoded);
        self
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// How far down page `page`'s column its last line reaches.
    pub fn depth(&self, page: usize) -> f32 {
        let pieces = self.pages.get(page).map_or(&[][..], Vec::as_slice);
        let bottom = |p: &Piece| {
            let set = self.blocks.get(p.block)?;
            let last = set.units.get(p.units.end.checked_sub(1)?)?;
            Some(p.dy + last.1)
        };
        pieces.iter().filter_map(bottom).fold(0.0, f32::max)
    }

    /// The blocks as given, blank ones counted.
    pub fn source(&self) -> &[Block] {
        &self.relay.source
    }

    /// Page `page`'s blocks and their bytes or rows; `usize::MAX` ends one.
    pub fn excerpts(&self, page: usize) -> Vec<(usize, Range<usize>)> {
        let pieces = self.pages.get(page).map_or(&[][..], Vec::as_slice);
        let excerpt = |p: &Piece| {
            let set = self.blocks.get(p.block)?;
            let from = set.starts.get(p.units.start).copied().unwrap_or(0);
            let to = set.starts.get(p.units.end).copied().unwrap_or(usize::MAX);
            Some((set.source, from..to))
        };
        pieces.iter().filter_map(excerpt).collect()
    }

    /// Every page is the same size: width, height in layout units.
    pub fn page_size(&self) -> (f32, f32) {
        let margins = if self.book_layout {
            GUTTER + EDGE
        } else {
            2.0 * PAD
        };
        (margins + self.width, PAGE_HEIGHT)
    }

    /// Where page `page` starts.
    pub fn anchor(&self, page: usize) -> Option<Anchor> {
        let piece = self.pages.get(page)?.first()?;
        let set = self.blocks.get(piece.block)?;
        Some(Anchor {
            block: set.source,
            offset: set.starts.get(piece.units.start).copied().unwrap_or(0),
        })
    }

    /// Page `page`'s links: a box for each line of each.
    pub fn links(&self, page: usize) -> Vec<PageLink> {
        let mut out = Vec::new();
        let left = self.column(page).0;
        for piece in self.pages.get(page).map_or(&[][..], Vec::as_slice) {
            let Some(set) = self.blocks.get(piece.block) else {
                continue;
            };
            let x = left + set.x;
            let y = PAD + piece.dy;
            match (&set.draw, &*self.laid(piece.block)) {
                (Draw::Text { pad, .. }, Laid::Text { para, .. }) => {
                    link_boxes(para, piece.units.clone(), (x + pad, y), &mut out);
                }
                (Draw::Table(grid), Laid::Table(rows)) => {
                    let [pad_x, pad_y] = grid.pad;
                    for r in piece.units.clone() {
                        let (Some(&(top, _)), Some(cells)) = (set.units.get(r), rows.get(r)) else {
                            continue;
                        };
                        for (c, cell) in cells.iter().enumerate() {
                            let at = (x + c as f32 * grid.column + pad_x, y + top + pad_y);
                            link_boxes(cell, 0..cell.layout.len(), at, &mut out);
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The page showing `anchor`, or the first after it.
    pub fn locate(&self, anchor: Anchor) -> usize {
        let at = (anchor.block, anchor.offset);
        let reaches_past = |page: &Vec<Piece>| {
            page.iter().any(|piece| {
                self.blocks.get(piece.block).is_some_and(|set| {
                    // Where the block goes on after this piece.
                    let next = set.starts.get(piece.units.end).copied();
                    (set.source, next.unwrap_or(usize::MAX)) > at
                })
            })
        };
        // Pages run in book order, so the first that does is found by halving.
        let n = self.pages.partition_point(|page| !reaches_past(page));
        n.min(self.pages.len().saturating_sub(1))
    }
}

/// One job's view of the book's typography and fonts.
struct Setter<'a> {
    typo: &'a Typography,
    stacks: &'a Stacks,
    lang: Lang,
    fonts: &'a mut Fonts,
    column: f32,
    height: f32,
}

impl<'a> Setter<'a> {
    fn new(typo: &'a Typography, stacks: &'a Stacks, fonts: &'a mut Fonts) -> Self {
        Self {
            typo,
            stacks,
            lang: hyphen::lang(&typo.language),
            fonts,
            column: column_width(typo),
            height: PAGE_HEIGHT - 2.0 * PAD,
        }
    }

    fn body(&self, frame: &Frame) -> Style<'a> {
        let typo = self.typo;
        Style {
            faces: &self.stacks.body,
            mono: &self.stacks.mono,
            size: typo.font_size,
            line_height: typo.line_height,
            weight: typo.weight,
            tone: if frame.soft { Tone::Soft } else { Tone::Body },
            align: frame.align,
            pre: false,
            justify: typo.justify && frame.align == Align::Start,
            hyphens: typo.hyphenate.then_some(self.lang),
            indent: 0.0,
            letter: typo.letter_spacing * typo.font_size,
            word: typo.word_spacing,
        }
    }

    fn text_style(&self, role: Role, frame: &Frame) -> Style<'a> {
        let body = self.body(frame);
        match role {
            Role::Heading(level) => Style {
                faces: &self.stacks.heading,
                size: body.size * heading_scale(level),
                line_height: HEADING_LINE_HEIGHT,
                weight: HEADING_WEIGHT,
                tone: if level >= 6 { Tone::Muted } else { body.tone },
                justify: false,
                hyphens: None,
                ..body
            },
            // Quotes and list items start flush, as mareader's do.
            Role::Paragraph if !frame.soft && frame.marker.is_none() => Style {
                indent: self.typo.indent * body.size,
                ..body
            },
            Role::Paragraph => body,
            Role::Preformatted => Style {
                faces: &self.stacks.mono,
                size: body.size * PRE_SCALE,
                line_height: PRE_LINE_HEIGHT,
                align: Align::Start,
                pre: true,
                justify: false,
                hyphens: None,
                letter: 0.0,
                word: 0.0,
                ..body
            },
        }
    }

    fn cell_style(&self, frame: &Frame, head: bool) -> Style<'a> {
        let body = self.body(frame);
        Style {
            size: body.size * CELL_SCALE,
            weight: if head { HEAD_CELL_WEIGHT } else { body.weight },
            align: Align::Start,
            justify: false,
            ..body
        }
    }

    fn block(&mut self, source: usize, block: &Block) -> (Set, Laid) {
        let (column, height) = (self.column, self.height);
        let em = self.typo.font_size;
        let frame = &block.frame;
        // Deep nesting stops indenting at half the column.
        let x = (frame.inset * em).clamp(0.0, column / 2.0);
        let width = column - x;
        let (draw, laid, units, starts) = match &block.content {
            Content::Text(text) => self.text(text, frame, width),
            Content::Rule => {
                let unit = (0.0, RULE_HEIGHT * em);
                (Draw::Rule, Laid::None, vec![unit], vec![0])
            }
            Content::Image { picture, cover } => {
                let (rect, unit) = if *cover {
                    cover_rect(picture, column, height)
                } else {
                    image_rect(picture, frame.align, width, height)
                };
                let picture = picture.clone();
                let draw = Draw::Image { picture, rect };
                (draw, Laid::None, vec![unit], vec![0])
            }
            Content::Table(table) => {
                let (grid, cells, units) = self.table(table, frame, width);
                let rows = (0..units.len()).collect();
                (Draw::Table(grid), Laid::Table(cells), units, rows)
            }
        };
        let cover = matches!(block.content, Content::Image { cover: true, .. });
        let heads = match &block.content {
            Content::Table(t) => t.rows.iter().take_while(|r| r.head).count(),
            _ => 0,
        };
        let set = Set {
            source,
            x: if cover { 0.0 } else { x },
            width: if cover { column } else { width },
            bars: frame
                .bars
                .iter()
                .map(|b| (b * em).min(column / 2.0))
                .collect(),
            draw,
            units,
            starts,
            keep_with_next: block.is_heading(),
            break_before: block.break_before,
            tight: frame.tight,
            lines: matches!(block.content, Content::Text(_)),
            heads,
        };
        (set, laid)
    }

    fn text(
        &mut self,
        text: &Text,
        frame: &Frame,
        width: f32,
    ) -> (Draw, Laid, Vec<(f32, f32)>, Vec<usize>) {
        let em = self.typo.font_size;
        let style = self.text_style(text.role, frame);
        let shaded = text.role == Role::Preformatted;
        let (pad, pad_y) = if shaded {
            (PRE_PAD.0 * em, PRE_PAD.1 * em)
        } else {
            (0.0, 0.0)
        };
        let inner = (width - 2.0 * pad).max(em);
        let mut para = layout_text(self.fonts, text, style, Some(inner));
        let mut units = line_units(&para.layout);
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
                justify: false,
                hyphens: None,
                indent: 0.0,
                ..style
            };
            Box::new(layout_text(self.fonts, &t, style, None))
        });
        let starts = std::mem::take(&mut para.starts);
        let para = Box::new(para);
        let laid = Laid::Text { para, marker };
        (Draw::Text { pad, shaded }, laid, units, starts)
    }

    fn table(&mut self, table: &Table, frame: &Frame, width: f32) -> (Grid, Vec<Vec<Para>>, Units) {
        let em = self.typo.font_size;
        let columns = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(1);
        let column = width / columns.max(1) as f32;
        let (pad_x, pad_y) = (CELL_PAD.0 * em, CELL_PAD.1 * em);
        let inner = (column - 2.0 * pad_x).max(em);
        let mut units = Vec::with_capacity(table.rows.len());
        let mut rows = Vec::with_capacity(table.rows.len());
        let mut heads = Vec::with_capacity(table.rows.len());
        let mut y = 0.0;
        for row in &table.rows {
            let style = self.cell_style(frame, row.head);
            let cells: Vec<Para> = row
                .cells
                .iter()
                .map(|c| layout_text(self.fonts, c, style, Some(inner)))
                .collect();
            let tallest = cells.iter().map(|c| c.layout.height()).fold(0.0, f32::max);
            let h = tallest + 2.0 * pad_y;
            units.push((y, y + h));
            y += h;
            rows.push(cells);
            heads.push(row.head);
        }
        let pad = [pad_x, pad_y];
        (Grid { column, pad, heads }, rows, units)
    }
}

fn alignment(style: &Style<'_>) -> Alignment {
    if style.justify {
        return Alignment::Justify;
    }
    match style.align {
        Align::Start => Alignment::Start,
        Align::Center => Alignment::Center,
        Align::End => Alignment::End,
    }
}

fn layout_text(fonts: &mut Fonts, text: &Text, style: Style<'_>, width: Option<f32>) -> Para {
    let hyphenated = style.hyphens.map(|lang| hyphen::hyphenate(text, lang));
    let text = hyphenated.as_ref().map_or(text, |h| &h.text);
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
    if style.letter != 0.0 {
        b.push_default(StyleProperty::LetterSpacing(style.letter));
    }
    if style.word != 0.0 {
        b.push_default(StyleProperty::WordSpacing(style.word));
    }
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
            let weight = FontWeight::new(bolder(style.weight));
            b.push(StyleProperty::FontWeight(weight), range.clone());
        }
        if code && !style.pre {
            let mono = FontFamily::List(Cow::Borrowed(style.mono));
            b.push(StyleProperty::FontFamily(mono), range.clone());
            b.push(
                StyleProperty::FontSize(style.size * CODE_SCALE),
                range.clone(),
            );
            b.push(StyleProperty::Brush(Ink { code: true, ..ink }), range);
        }
    }
    for link in &text.links {
        b.push(StyleProperty::Underline(true), link.range.clone());
    }
    let mut layout = b.build(&text.text);
    if style.indent > 0.0 {
        layout.set_text_indent(style.indent, IndentOptions::default());
    }
    layout.break_all_lines(width);
    layout.align(alignment(&style), AlignmentOptions::default());
    let ends_soft = |end: usize| text.text.get(..end).is_some_and(|s| s.ends_with(SHY));
    let hyphens = layout
        .lines()
        .enumerate()
        .filter(|(_, l)| hyphenated.is_some() && ends_soft(l.text_range().end))
        .map(|(i, _)| i)
        .collect();
    let starts = layout
        .lines()
        .map(|l| {
            let start = l.text_range().start;
            hyphenated.as_ref().map_or(start, |h| h.original(start))
        })
        .collect();
    Para {
        layout,
        text: text.text.clone(),
        hyphens,
        marks: hyphenated
            .as_ref()
            .map(|h| h.marks.clone())
            .unwrap_or_default(),
        starts,
        links: text.links.clone(),
    }
}

/// Boxes of `para`'s links on lines `lines`, its origin at `x, y`.
fn link_boxes(para: &Para, lines: Range<usize>, (x, y): (f32, f32), out: &mut Vec<PageLink>) {
    let layout = &para.layout;
    for link in &para.links {
        let caret = |i, affinity| Caret::from_byte_index(layout, i, affinity);
        let start = caret(link.range.start, Affinity::Downstream);
        let end = caret(link.range.end, Affinity::Upstream);
        Selection::new(start, end).geometry_with(layout, |b, line| {
            if lines.contains(&line) {
                let (w, h) = ((b.x1 - b.x0) as f32, (b.y1 - b.y0) as f32);
                out.push(PageLink {
                    target: link.target,
                    rect: [x + b.x0 as f32, y + b.y0 as f32, w, h],
                });
            }
        });
    }
}

fn line_units(layout: &Layout<Ink>) -> Vec<(f32, f32)> {
    layout
        .lines()
        .map(|l| (l.metrics().block_min_coord, l.metrics().block_max_coord))
        .collect()
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

/// The text column's width; A4's own at a scale of 1.
fn column_width(typo: &Typography) -> f32 {
    let page = if typo.book_layout {
        PAGE_WIDTH - GUTTER - EDGE
    } else {
        PAGE_WIDTH - 2.0 * PAD
    };
    let (lo, hi) = COLUMN_SCALE;
    let scale = if typo.column_scale.is_finite() {
        typo.column_scale.clamp(lo, hi)
    } else {
        1.0
    };
    (page * scale).max(MIN_COLUMN)
}

/// The column scale's range, as mareader's Column Width dial.
pub const COLUMN_SCALE: (f32, f32) = (0.6, 1.4);
/// The narrowest column, as mareader's.
const MIN_COLUMN: f32 = 160.0;

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
        let blocks = Arc::new(blocks.to_vec());
        Flow::new(blocks, &Typography::default(), Fonts::bundled())
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
    fn a_narrow_room_is_drawn_as_narrow_as_it_was_measured() {
        let blocks = Arc::new(vec![para(120)]);
        let typo = Typography::default();
        let f = Flow::sized(blocks, &typo, Fonts::bundled(), (300.0, 2000.0), false);
        let width = match &*f.laid(0) {
            Laid::Text { para, .. } => para.layout.width(),
            _ => f32::INFINITY,
        };
        assert!(width <= 300.5, "{width}");
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
        let marked = matches!(
            &*f.laid(0),
            Laid::Text {
                marker: Some(_),
                ..
            }
        );
        assert!(marked);
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
    fn with(blocks: &[Block], typo: Typography) -> Flow {
        Flow::new(Arc::new(blocks.to_vec()), &typo, Fonts::bundled())
    }

    fn book() -> Vec<Block> {
        let mut blocks: Vec<Block> = (0..60).map(|i| para(40 + i * 9)).collect();
        blocks.insert(20, heading("Middle"));
        blocks
    }

    /// `see` of the first text block's layout.
    fn first_para<R>(f: &Flow, see: impl FnOnce(&Para) -> R) -> Option<R> {
        let n = f
            .blocks
            .iter()
            .position(|s| matches!(s.draw, Draw::Text { .. }))?;
        match &*f.laid(n) {
            Laid::Text { para, .. } => Some(see(para)),
            _ => None,
        }
    }

    /// Each line's right edge: the end of its last glyph run.
    fn right_edges(p: &Para) -> Vec<f32> {
        p.layout
            .lines()
            .map(|l| {
                l.items()
                    .filter_map(|i| match i {
                        parley::PositionedLayoutItem::GlyphRun(g) => Some(g.offset() + g.advance()),
                        parley::PositionedLayoutItem::InlineBox(_) => None,
                    })
                    .fold(0.0, f32::max)
            })
            .collect()
    }

    /// The leftmost column with ink in it, in pixels.
    fn left_ink(px: &crate::Pixels) -> Option<u32> {
        let width = px.width.max(1) as usize;
        px.rgba
            .iter()
            .skip(3)
            .step_by(4)
            .enumerate()
            .filter(|(_, a)| **a > 128)
            .map(|(i, _)| (i % width) as u32)
            .min()
    }

    #[test]
    fn the_column_scale_widens_the_column_and_page_within_its_range() {
        let blocks = book();
        let scaled = |column_scale| {
            let typo = Typography {
                column_scale,
                ..Typography::default()
            };
            with(&blocks, typo)
        };
        let (plain, wide, narrow) = (scaled(1.0), scaled(1.4), scaled(0.6));
        let width = PAGE_WIDTH - 2.0 * PAD;
        assert_eq!(plain.page_size(), (PAGE_WIDTH, PAGE_HEIGHT));
        assert_eq!(wide.column(0), (PAD, width * 1.4));
        assert_eq!(wide.page_size().0, 2.0 * PAD + width * 1.4);
        assert!(wide.page_count() < plain.page_count());
        assert!(narrow.page_count() > plain.page_count());
        // Past the range, or not a number, it clamps or falls back.
        assert_eq!(scaled(9.0).column(0), wide.column(0));
        assert_eq!(scaled(f32::NAN).column(0), plain.column(0));
    }

    #[test]
    fn a_region_is_set_like_its_book_and_keeps_spread_sides() {
        let typo = Typography {
            book_layout: true,
            ..Typography::default()
        };
        let base = with(&book(), typo.clone());
        let blocks = Arc::new(vec![para(30), para(500), para(30)]);
        let region = |first, replaced| {
            let fonts = Fonts::bundled();
            base.region(Arc::clone(&blocks), Some("fr"), fonts, (first, replaced))
        };
        let one = region(3, 1);
        let laid = Flow::new(Arc::clone(&blocks), &typo, Fonts::bundled());
        assert_eq!(one.page_size(), base.page_size());
        // An odd change would swap later sides, so a blank page evens it.
        let extra = usize::from(laid.page_count().is_multiple_of(2));
        assert_eq!(one.page_count(), laid.page_count() + extra);
        let last = one.excerpts(one.page_count() - 1);
        assert_eq!(last.is_empty(), extra == 1, "{last:?}");
        // Its first page sits on the side of the page it replaces.
        assert_eq!(one.column(0), base.column(3));
        assert_eq!(region(4, 1).column(0), base.column(4));
        assert_eq!(one.bands().len(), one.page_count());
        let band = one.bands().first().map(|b| (b.x, b.width));
        assert_eq!(band, Some(one.column(0)));
        let plain = with(&book(), Typography::default());
        let free = plain.region(Arc::clone(&blocks), None, Fonts::bundled(), (3, 1));
        assert_eq!(free.page_count(), plain_count(&blocks));
    }

    fn plain_count(blocks: &Arc<Vec<Block>>) -> usize {
        let typo = Typography::default();
        Flow::new(Arc::clone(blocks), &typo, Fonts::bundled()).page_count()
    }

    #[test]
    fn book_layout_moves_each_column_toward_the_spine() {
        let blocks = book();
        let plain = with(&blocks, Typography::default());
        let typo = Typography {
            book_layout: true,
            ..Typography::default()
        };
        let bound = with(&blocks, typo);
        assert_eq!(plain.column(0), (PAD, PAGE_WIDTH - 2.0 * PAD));
        assert_eq!(plain.column(1), plain.column(0));
        let width = PAGE_WIDTH - GUTTER - EDGE;
        // Spreads pair pages from the first: the left one's spine is right.
        assert_eq!(bound.column(0), (EDGE, width));
        assert_eq!(bound.column(1), (GUTTER, width));
        let region = [0, 0, PAGE_WIDTH as u32, PAGE_HEIGHT as u32];
        let ink = |f: &Flow, page| f.render(page, 1.0, region).as_ref().and_then(left_ink);
        let (even, odd) = (ink(&bound, 2), ink(&bound, 3));
        assert!(
            even.is_some_and(|x| x.abs_diff(EDGE as u32) < 4),
            "{even:?}"
        );
        assert!(
            odd.is_some_and(|x| x.abs_diff(GUTTER as u32) < 4),
            "{odd:?}"
        );
        assert!(ink(&plain, 3).is_some_and(|x| x.abs_diff(PAD as u32) < 4));
    }

    #[test]
    fn bands_hold_each_page_s_text_and_the_gap_its_cut_swallowed() {
        let f = with(&book(), Typography::default());
        let bands = f.bands();
        assert_eq!(bands.len(), f.page_count());
        let first = bands.first().copied();
        assert!(first.is_some_and(|b| b.y == 0.0), "the first keeps its pad");
        let last = bands.last().copied();
        let foot = last.map(|b| b.y + b.height);
        assert!(foot.is_some_and(|y| y <= PAGE_HEIGHT), "{last:?}");
        for (n, band) in bands.iter().enumerate() {
            assert_eq!((band.x, band.width), f.column(n));
            assert!(band.height > 0.0 && band.y + band.height <= PAGE_HEIGHT);
            let Some(piece) = f.pages.get(n).and_then(|p| p.first()) else {
                continue;
            };
            // Lines cut mid-paragraph butt; a cut between blocks keeps
            // the block gap.
            let lead = PAD - band.y;
            match (n, piece.units.start) {
                (0, _) => assert_eq!(lead, PAD),
                (_, 0) => assert!((lead - 17.0).abs() < 0.01, "{n}: {lead}"),
                _ => assert_eq!(lead, 0.0, "{n}"),
            }
        }
    }

    #[test]
    fn bigger_type_takes_more_pages_and_anchors_keep_the_place() {
        let blocks = book();
        let small = with(&blocks, Typography::default());
        let big = with(
            &blocks,
            Typography {
                font_size: 24.0,
                ..Typography::default()
            },
        );
        assert!(big.page_count() > small.page_count());
        for page in [1, 3, small.page_count() / 2, small.page_count() - 1] {
            let Some(anchor) = small.anchor(page) else {
                continue;
            };
            let found = big.locate(anchor);
            // The page found starts at or before the place...
            assert!(big.anchor(found).is_some_and(|a| a <= anchor), "{page}");
            // ...and the next one after it.
            let next = big.anchor(found + 1);
            assert!(next.is_none_or(|a| a > anchor), "{page}: {next:?}");
        }
        assert_eq!(
            small.locate(Anchor {
                block: 9999,
                offset: 0
            }),
            small.page_count() - 1
        );
    }

    #[test]
    fn justified_lines_reach_the_column_edge_but_the_last() {
        let column = PAGE_WIDTH - 2.0 * PAD;
        let blocks = [para(120)];
        let ragged = with(&blocks, Typography::default());
        let justified = with(
            &blocks,
            Typography {
                justify: true,
                ..Typography::default()
            },
        );
        let edges = first_para(&justified, right_edges).unwrap_or_default();
        let (last, full) = edges.split_last().unwrap_or((&0.0, &[]));
        assert!(full.len() > 2, "{edges:?}");
        // A trailing space may hang past the edge; never short of it.
        let reach = |e: &f32| *e > column - 0.5 && *e < column + 17.0 * 0.5;
        assert!(full.iter().all(reach), "{edges:?}");
        assert!(*last < column - 1.0, "{edges:?}");
        let ragged = first_para(&ragged, right_edges).unwrap_or_default();
        assert!(ragged.iter().any(|e| *e < column - 1.0), "{ragged:?}");
    }

    #[test]
    fn hyphenation_breaks_long_words_and_marks_those_lines() {
        let long = "Incomprehensibilities characteristically internationalisation ";
        let blocks = [Block::text(Role::Paragraph, &long.repeat(12))];
        let marks = |hyphenate| {
            let f = with(
                &blocks,
                Typography {
                    hyphenate,
                    ..Typography::default()
                },
            );
            first_para(&f, |p| p.hyphens.len()).unwrap_or(0)
        };
        assert_eq!(marks(false), 0);
        assert!(marks(true) > 0);
        // Anchors stay in the unhyphenated text's bytes.
        let f = with(
            &blocks,
            Typography {
                hyphenate: true,
                ..Typography::default()
            },
        );
        let starts = f
            .blocks
            .first()
            .map(|s| s.starts.clone())
            .unwrap_or_default();
        let text = long.repeat(12);
        assert!(
            starts
                .iter()
                .all(|&s| text.is_char_boundary(s) && s < text.len())
        );
    }

    #[test]
    fn spacing_indent_and_weight_take_effect() {
        let blocks = [para(200)];
        let lines = |typo: Typography| {
            let f = with(&blocks, typo);
            first_para(&f, |p| p.layout.len()).unwrap_or(0)
        };
        let base = lines(Typography::default());
        let letters = lines(Typography {
            letter_spacing: 0.15,
            ..Typography::default()
        });
        let words = lines(Typography {
            word_spacing: 8.0,
            ..Typography::default()
        });
        assert!(letters > base && words > base, "{base} {letters} {words}");
        let f = with(
            &blocks,
            Typography {
                indent: 2.0,
                ..Typography::default()
            },
        );
        let first = first_para(&f, |p| {
            p.layout.get(0)?.items().find_map(|i| match i {
                parley::PositionedLayoutItem::GlyphRun(g) => Some(g.offset()),
                parley::PositionedLayoutItem::InlineBox(_) => None,
            })
        })
        .flatten();
        assert!(first.is_some_and(|x| (x - 34.0).abs() < 0.5), "{first:?}");
        assert_eq!(bolder(400.0), 700.0);
        assert_eq!(bolder(600.0), 900.0);
        assert_eq!(bolder(200.0), 400.0);
    }

    fn linked(before: &str, link: &str, target: u32) -> Block {
        let mut t = Text::new(Role::Paragraph);
        t.push(before, Emphasis::default());
        t.push_link(link, Emphasis::default(), target);
        t.push(" after.", Emphasis::default());
        Block::new(Content::Text(t))
    }

    #[test]
    fn links_box_their_words_on_every_line_they_cross() {
        let words = vec!["linked"; 60].join(" ");
        let mut second = linked("Next ", "chapter", 9);
        second.break_before = true;
        let f = flow(&[linked("Before the link ", &words, 7), second]);
        let links = f.links(0);
        assert!(links.len() > 2, "{links:?}");
        assert!(links.iter().all(|l| l.target == 7));
        let right = PAGE_WIDTH - PAD + 1.0;
        for l in &links {
            let [x, y, w, h] = l.rect;
            assert!(x >= PAD - 1.0 && x + w <= right && w > 0.0, "{l:?}");
            assert!(y >= PAD - 1.0 && h > 0.0, "{l:?}");
        }
        // The first box starts after the unlinked words, on line one.
        let first = links.first().map(|l| l.rect);
        assert!(first.is_some_and(|[x, ..]| x > PAD + 40.0), "{first:?}");
        let tops: Vec<f32> = links.iter().map(|l| l.rect[1]).collect();
        assert!(tops.windows(2).all(|w| w.first() < w.get(1)), "{tops:?}");
        let next = f.links(1);
        assert_eq!(next.iter().map(|l| l.target).collect::<Vec<_>>(), [9]);
    }

    #[test]
    fn hyphenated_links_keep_their_words() {
        let typo = Typography {
            hyphenate: true,
            ..Typography::default()
        };
        let words = "extraordinarily incomprehensible ".repeat(30);
        let blocks = [linked("Some text ", &words, 3)];
        let plain = with(&blocks, Typography::default()).links(0);
        let split = with(&blocks, typo).links(0);
        // The link starts after the same unlinked words either way.
        let first = |l: &[PageLink]| l.first().map(|l| l.rect[0]);
        assert!(first(&split).is_some() && first(&split) == first(&plain));
        assert!(split.len() + 1 >= plain.len(), "{split:?}");
        let right = PAGE_WIDTH - PAD + 1.0;
        let inside = |l: &PageLink| l.rect[0] >= PAD - 1.0 && l.rect[0] + l.rect[2] <= right;
        assert!(split.iter().all(inside), "{split:?}");
    }

    fn kept(f: &Flow) -> Vec<usize> {
        let recent = f
            .relay
            .recent
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut kept: Vec<usize> = recent.laid.keys().copied().collect();
        kept.sort_unstable();
        kept
    }

    #[test]
    fn layouts_are_laid_for_the_pages_drawn_and_no_others() {
        let f = flow(&book());
        assert!(kept(&f).is_empty(), "nothing is kept at open");
        let region = [0, 0, PAGE_WIDTH as u32, PAGE_HEIGHT as u32];
        assert!(f.render(2, 1.0, region).is_some());
        let on_page: Vec<usize> = f
            .pages
            .get(2)
            .into_iter()
            .flatten()
            .map(|p| p.block)
            .collect();
        assert!(!on_page.is_empty());
        assert_eq!(kept(&f), on_page);
    }

    #[test]
    fn a_page_laid_again_draws_and_reads_the_same() {
        let f = flow(&book());
        let region = [0, 0, PAGE_WIDTH as u32, PAGE_HEIGHT as u32];
        let see = |f: &Flow| (f.render(1, 1.0, region), f.text(1), f.links(1));
        let first = see(&f);
        *f.relay
            .recent
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Recent::default();
        assert_eq!(see(&f), first);
        assert!(first.1.len() > 10);
    }

    #[test]
    fn kept_layouts_stay_under_the_cap_and_the_used_stay() {
        let mut recent = Recent::default();
        let laid = || Arc::new(Laid::None);
        let quarter = MAX_LAID / 4;
        for block in 0..4 {
            recent.put(block, laid(), quarter);
        }
        assert!(recent.get(0).is_some());
        // Past the cap, the oldest go until half is left: 1 and 2.
        recent.put(4, laid(), quarter);
        assert!(recent.bytes <= MAX_LAID / 2);
        let mut kept: Vec<usize> = recent.laid.keys().copied().collect();
        kept.sort_unstable();
        assert_eq!(kept, [0, 4]);
        // One block past the cap alone is still kept while used.
        recent.put(5, laid(), 2 * MAX_LAID);
        assert!(recent.get(5).is_some());
    }

    #[test]
    fn a_missing_face_falls_back_down_its_stack() {
        let blocks = book();
        let mut faces = Faces::default();
        faces
            .body
            .insert(0, Family::Named("No Such Face 7f3a".into()));
        let typo = Typography {
            faces,
            ..Typography::default()
        };
        let missing = Flow::new(Arc::new(blocks.clone()), &typo, Fonts::system());
        let plain = with(&blocks, Typography::default());
        assert_eq!(missing.page_count(), plain.page_count());
        let pages = |f: &Flow| f.pages.clone();
        assert_eq!(pages(&missing), pages(&plain));
    }
}
