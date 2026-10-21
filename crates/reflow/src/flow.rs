//! Blocks set by parley at their frame's width, then cut into pages.
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use hypher::Lang;
use parley::{
    Affinity, Alignment, AlignmentOptions, BaseDirection, Cursor as Caret, FontFamily,
    FontFamilyName, FontFeatures, FontStyle, FontWeight, GenericFamily, IndentOptions, InlineBox,
    InlineBoxKind, Layout, LineHeight, OverflowWrap, Selection, StyleProperty, VerticalAlign,
};

use crate::block::{
    Align, Backdrop, Block, Boxed, Case, Cell, Content, Dir, Edge, Emphasis, Finish, Floated,
    Frame, GridRow, Inline, Justify, LastLine, Length, Link, Look, Rise, Role, Script, Table, Text,
    VAlign,
};
use crate::fonts::{BOOK_FACE, Faces, Family, Fonts, Ink, Kind, SANS, Tone};
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

/// When a book's own design shows (`docs/epub-layout.md`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Design {
    /// The book's type everywhere; its layout in the chapters it designs.
    #[default]
    Auto,
    On,
    Off,
}

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
    pub design: Design,
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
            design: Design::Auto,
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
/// Text sizes a book may ask for, in layout units.
const MIN_SIZE: f32 = 2.0;
const MAX_SIZE: f32 = 400.0;
/// The tightest line height a book may ask for.
const MIN_LINE: f32 = 0.5;
/// Reader defaults (size, line, gap, weight); book type moves off them too.
const READER: (f32, f32, f32, f32) = (17.0, 1.7, 1.0, 400.0);
const RULE_HEIGHT: f32 = 0.5;
/// From a list marker's right edge to the text.
pub(crate) const MARKER_GAP: f32 = 0.5;
/// A capital's height in ems, near enough for most faces.
const CAP_HEIGHT: f32 = 0.7;
/// Lines a drop cap may span, and its gap to the text in ems.
const MAX_DROP: usize = 6;
const DROP_GAP: f32 = 0.2;
/// A drop cap's letter in the text: there, but too small to see.
const HIDDEN_SIZE: f32 = 0.01;
const FIRST_LINE_PASSES: usize = 3;
/// Px of a marker picture per em of its text.
const MARKER_PX: f32 = 16.0;
/// Holds a picture-only marker's line open; draws nothing.
const WORD_JOINER: &str = "\u{2060}";

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
    /// Runs keep the sizes the book gives them.
    sized: bool,
    /// How the last line sits; none follows the rest.
    last: Option<LastLine>,
    dir: Dir,
    /// Stacks for runs' own families, by [`Emphasis::face`].
    runs: &'a [Stack],
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
    /// Pictures in its lines, by their boxes' ids.
    pub(crate) pictures: Vec<Arc<Picture>>,
    /// Ruby notes over its runs, bases in bytes of the text as set.
    pub(crate) ruby: Vec<Note>,
}

/// A ruby note: its base in the text as set, then the note laid small.
pub(crate) struct Note {
    pub(crate) range: Range<usize>,
    text: String,
    pub(crate) para: Option<Box<Para>>,
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
    /// Room only: a designed book's rule draws as its boxes.
    Space,
}

/// A book's box placed in the column, in layout units (`docs/epub-layout.md`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placed {
    /// The same for every block in the box.
    key: usize,
    /// The border box across.
    x: f32,
    width: f32,
    border: [f32; 4],
    edges: [Edge; 4],
    colours: [Option<[u8; 3]>; 4],
    /// From its top border to its first block, its last block to the bottom.
    top: f32,
    bottom: f32,
    centre: Option<f32>,
    /// Its top border and padding, and the least height of its content.
    inside_top: f32,
    height: Option<f32>,
    fill: Option<f32>,
    backdrop: Option<Arc<Backdrop>>,
    /// The body em its backdrop's lengths are in.
    em: f32,
    /// Its corners' radius in layout units, before fitting the box.
    radius: f32,
    finish: Option<Arc<Finish>>,
    /// Kept on one page when it fits.
    whole: bool,
}

/// A float placed in the column: its key, the boxes holding it, its place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FloatAt {
    key: usize,
    depth: usize,
    x: f32,
    width: f32,
}

/// A float beside a text: its far edge in the column, its reach down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Exclusion {
    right: bool,
    edge: f32,
    height: f32,
}

/// Room a float takes from a text's lines, from its own edges.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Beside {
    left: f32,
    right: f32,
    height: f32,
}

/// A float's wrap going on: its edge, reach, and what may wrap.
#[derive(Debug, Clone)]
struct Wrap {
    exclusion: Exclusion,
    height: f32,
    within: Holders,
}

/// A grid item's place in its row's room, as sized: left, width, line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CellPlace {
    left: f32,
    width: f32,
    /// A wrapped flex row's line.
    line: u16,
}

/// A grid item's least and most content widths, or its set width.
#[derive(Debug, Clone, Copy)]
struct Extent {
    row: usize,
    item: u16,
    least: f32,
    most: f32,
    set: bool,
}

/// A block's place in a grid row, in layout units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CellAt {
    /// The same for every block in the row.
    key: usize,
    column: usize,
    /// How many of the block's boxes hold the grid.
    depth: usize,
    valign: VAlign,
    /// The space above the row.
    before: f32,
    /// Its grid, and the rows its cell spans down.
    grid: usize,
    down: u16,
}

/// A box's borders on one page; a page's cut leaves that side open.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Deco {
    /// Left, top, width and height in column units.
    pub(crate) rect: [f32; 4],
    pub(crate) border: [f32; 4],
    pub(crate) edges: [Edge; 4],
    pub(crate) colours: [Option<[u8; 3]>; 4],
    pub(crate) fill: Option<f32>,
    pub(crate) backdrop: Option<Arc<Backdrop>>,
    pub(crate) em: f32,
    pub(crate) radius: f32,
    pub(crate) finish: Option<Arc<Finish>>,
    /// Cut at the page's top and bottom: no frame picture there.
    pub(crate) continued: [bool; 2],
}

/// A table set in equal columns; row `i` spans `units[i]`.
pub(crate) struct Grid {
    pub(crate) column: f32,
    /// Cell padding: across, down.
    pub(crate) pad: [f32; 2],
    /// Which rows are head rows.
    pub(crate) heads: Vec<bool>,
    /// The book's colour for the grid, if it sets one.
    pub(crate) rule: Option<[u8; 3]>,
}

/// A drop cap: its letter laid large, sitting on this line's baseline.
pub(crate) struct Initial {
    pub(crate) para: Para,
    pub(crate) line: usize,
}

/// A block's text layouts: the bulk of a set book, so laid on demand.
pub(crate) enum Laid {
    Text {
        para: Box<Para>,
        /// A list marker, hung left of the first line.
        marker: Option<Box<Para>>,
        initial: Option<Box<Initial>>,
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
    /// Right to left: its marker hangs right of its text.
    pub(crate) rtl: bool,
    pub(crate) draw: Draw,
    /// Each unit's top and bottom: lines, rows or the whole image.
    pub(crate) units: Vec<(f32, f32)>,
    /// Each unit's place in the block: a text offset or a row.
    starts: Vec<usize>,
    keep_with_next: bool,
    break_before: bool,
    tight: bool,
    /// A look's space above, in layout units, instead of the gap.
    before: Option<f32>,
    /// Text: avoid lone lines at a split.
    lines: bool,
    /// Lines kept together at a split's foot and head.
    orphans: usize,
    widows: usize,
    /// Kept on one page when it fits.
    whole: bool,
    /// A table's leading head rows, never left alone at a page's foot.
    heads: usize,
    /// Designed: the book's boxes around it, outermost first.
    boxes: Vec<Placed>,
    /// Designed: its place in a grid row.
    cell: Option<CellAt>,
    /// Designed: the float it is in.
    float: Option<FloatAt>,
    /// Designed: a float beside its first lines.
    beside: Option<Exclusion>,
    /// It starts below the float before it.
    clear: bool,
    /// Designed, in a grid: its item's place, as sized.
    place: Option<CellPlace>,
    /// Designed: a turn in degrees, drawn about its middle.
    pub(crate) rotate: f32,
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
    /// Each page's box borders, drawn under its pieces.
    pub(crate) decos: Vec<Vec<Deco>>,
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
            let items: Vec<(usize, &Block)> = blocks
                .iter()
                .enumerate()
                .filter(|(_, b)| !b.is_blank())
                .collect();
            setter.set_all(&items)
        };
        let (pages, decos) = paginate(&set, cut, typo);
        let mut flow = Self {
            blocks: set,
            pages,
            decos,
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
            flow.decos.push(Vec::new());
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
            // Laid again as first set: beside its float, or as one.
            setter.float_box = set.float.map(|f| (f.x, f.width));
            setter.exclusion = set.beside;
            setter.cell_place = set.place;
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
    /// A float's place, given its blocks are set.
    float_box: Option<(f32, f32)>,
    /// A float beside the next block's first lines.
    exclusion: Option<Exclusion>,
    /// The next block's grid item place, overriding the sized ones.
    cell_place: Option<CellPlace>,
    /// The grid being set: its items' places, as sized.
    places: HashMap<(usize, u16), CellPlace>,
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
            float_box: None,
            exclusion: None,
            cell_place: None,
            places: HashMap::new(),
        }
    }

    /// Block `frame`'s grid item place, as sized.
    fn place_of(&self, frame: &Frame) -> Option<CellPlace> {
        let sized = || {
            item_key(frame)
                .and_then(|key| self.places.get(&key))
                .copied()
        };
        self.cell_place.or_else(sized)
    }

    /// Designed text's em: the column as a phone page, scaled as the reader's.
    fn design_em(&self) -> f32 {
        let em = READER.0;
        let phone = (self.column / DESIGN_EMS).clamp(em, em * MAX_DESIGN_SCALE);
        phone * self.typo.font_size / em
    }

    /// The em `frame`'s book type is set in: the reader's in prose.
    fn em_of(&self, frame: &Frame) -> f32 {
        match self.look(frame) {
            Some(_) => self.design_em(),
            None => self.typo.font_size,
        }
    }

    /// Sets `items` in order; floats and the text wrapping past them
    /// need what came before.
    fn set_all(&mut self, items: &[(usize, &Block)]) -> Vec<Set> {
        let mut out = Vec::with_capacity(items.len());
        let mut wrap: Option<Wrap> = None;
        let mut sized = None;
        let mut k = 0;
        while let Some(&(i, block)) = items.get(k) {
            let grid = self.look(&block.frame).and(grid_key(&block.frame));
            if let Some(key) = grid.filter(|key| sized != Some(*key)) {
                // A grid's items are sized together, as it starts.
                let rest = items.get(k..).unwrap_or_default();
                let same = |(_, b): &&(usize, &Block)| grid_key(&b.frame) == Some(key);
                let end = k + rest.iter().take_while(same).count().max(1);
                let places = self.size_grid(items.get(k..end).unwrap_or_default());
                self.places = places;
                sized = Some(key);
            }
            let floated = self.look(&block.frame).and(block.frame.float.as_ref());
            if let Some(f) = floated {
                let key = Arc::as_ptr(f) as usize;
                let rest = items.get(k..).unwrap_or_default();
                let same = |(_, b): &(usize, &Block)| float_key(&b.frame) == Some(key);
                let end = k + rest.iter().take_while(|b| same(b)).count().max(1);
                let (sets, next) = self.float_group(items.get(k..end).unwrap_or_default(), f);
                out.extend(sets);
                wrap = next;
                k = end;
                continue;
            }
            self.exclusion = None;
            let mut clear = false;
            if let Some(w) = wrap.take() {
                let height = w.height - block.frame.before * self.design_em();
                let within = w.within.hold(&block.frame) && self.look(&block.frame).is_some();
                if within && matches!(block.content, Content::Text(_)) && height > 0.0 {
                    self.exclusion = Some(Exclusion {
                        height,
                        ..w.exclusion
                    });
                    let (set, _) = self.block(i, block);
                    let left = height - span_height(&set, 0..set.units.len());
                    if left > 0.0 {
                        wrap = Some(Wrap { height: left, ..w });
                    }
                    out.push(set);
                    k += 1;
                    continue;
                }
                clear = height > 0.0;
            }
            let (mut set, _) = self.block(i, block);
            set.clear = clear;
            out.push(set);
            k += 1;
        }
        self.exclusion = None;
        out
    }

    /// Sizes a grid's items to their content: table columns or flex lines.
    fn size_grid(&mut self, group: &[(usize, &Block)]) -> HashMap<(usize, u16), CellPlace> {
        let mut places = HashMap::new();
        let Some(&(_, first)) = group.first() else {
            return places;
        };
        let Some(cell) = first.frame.cell.as_ref() else {
            return places;
        };
        let em = self.design_em();
        let (gx, room) = self.grid_room(&first.frame);
        let mut extents: Vec<Extent> = Vec::new();
        for &(i, b) in group.iter().take(MAX_MEASURED) {
            let Some((row, item)) = item_key(&b.frame) else {
                continue;
            };
            let set = b.frame.cell.as_ref().and_then(|c| {
                let w = c.row.widths.get(usize::from(item)).copied().flatten()?;
                Some(resolve(w, room, em).max(0.0))
            });
            let at = extents.iter().position(|e| e.row == row && e.item == item);
            if at.is_some_and(|n| extents.get(n).is_some_and(|e| e.set)) {
                continue;
            }
            let (least, most) = match set {
                Some(w) => (w, w),
                None => self.measure(i, b, (gx, room)),
            };
            match at.and_then(|n| extents.get_mut(n)) {
                Some(e) => (e.least, e.most) = (e.least.max(least), e.most.max(most)),
                None => extents.push(Extent {
                    row,
                    item,
                    least,
                    most,
                    set: set.is_some(),
                }),
            }
        }
        let shared = &cell.row.columns;
        match shared.flex {
            Some(flex) => {
                let gap = flex.gap.max(0.0) * em;
                let mut rows: Vec<usize> = extents.iter().map(|e| e.row).collect();
                rows.dedup();
                for row in rows {
                    let grow = group.iter().find_map(|(_, b)| {
                        let c = b.frame.cell.as_ref()?;
                        (Arc::as_ptr(&c.row) as usize == row).then(|| c.row.grow.clone())
                    });
                    let items: Vec<&Extent> = extents.iter().filter(|e| e.row == row).collect();
                    flex_places(
                        &items,
                        (room, gap),
                        (flex, &grow.unwrap_or_default()),
                        &mut places,
                    );
                }
            }
            None => {
                let rows = group.iter().filter_map(|(_, b)| b.frame.cell.as_ref());
                table_places(
                    &extents,
                    rows,
                    (room, shared.fill, shared.centre),
                    &mut places,
                );
            }
        }
        places
    }

    /// A grid item's least and most content widths, from its block `b`.
    fn measure(&mut self, i: usize, b: &Block, (gx, room): (f32, f32)) -> (f32, f32) {
        let depth = b.frame.cell.as_ref().map_or(0, |c| usize::from(c.depth));
        let mut extent = |width: f32| {
            self.cell_place = Some(CellPlace {
                left: 0.0,
                width,
                line: 0,
            });
            let (set, laid) = self.block(i, b);
            // A box of set width inside the item holds its content.
            let held = b.frame.boxes.iter().zip(&set.boxes).skip(depth);
            let fixed = held.filter(|(b, _)| matches!(b.width, Some(Length::Em(_))));
            match fixed.map(|(_, p)| p.width).next() {
                Some(w) => w,
                None => natural(&set, &laid) + 2.0 * (set.x - gx).max(0.0),
            }
        };
        let least = extent(1.0);
        let most = extent(room * MEASURE_ROOM);
        self.cell_place = None;
        (least.min(most), most)
    }

    /// Where a block's grid sits: the left and width its boxes leave.
    fn grid_room(&self, frame: &Frame) -> (f32, f32) {
        let em = self.design_em();
        let (mut x, mut width) = (0.0, self.column);
        let depth = frame.cell.as_ref().map_or(0, |c| usize::from(c.depth));
        for b in frame.boxes.iter().take(depth) {
            let p = place(b, (x, width), em);
            let inside = (b.border[1] + b.border[3] + b.padding[1] + b.padding[3]) * em;
            x = p.x + (b.border[3] + b.padding[3]) * em;
            width = (p.width - inside).max(1.0);
        }
        (x, width)
    }

    /// Sets a float's blocks as wide as they need; gives its wrap.
    fn float_group(&mut self, group: &[(usize, &Block)], f: &Floated) -> (Vec<Set>, Option<Wrap>) {
        (self.exclusion, self.float_box) = (None, None);
        let mut room = None;
        let mut want: f32 = 0.0;
        for &(i, b) in group {
            let (set, laid) = self.block(i, b);
            room.get_or_insert((set.x, set.width));
            want = want.max(natural(&set, &laid));
        }
        let Some((x, width)) = room else {
            return (Vec::new(), None);
        };
        let want = want.clamp(1.0, width);
        let left = if f.right { x + width - want } else { x };
        self.float_box = Some((left, want));
        let sets: Vec<Set> = group.iter().map(|&(i, b)| self.block(i, b).0).collect();
        self.float_box = None;
        let em = self.design_em();
        let depth = usize::from(f.depth);
        let own = sets.first().and_then(|s| s.boxes.get(depth));
        let (near, far) = own.map_or((left, left + want), |p| (p.x, p.x + p.width));
        let edge = if f.right {
            near - FLOAT_GAP * em
        } else {
            far + FLOAT_GAP * em
        };
        let mut height = 0.0;
        for (n, set) in sets.iter().enumerate() {
            let gap = gap(set, self.typo);
            height += span_height(set, 0..set.units.len()) + gap;
            if n == 0 {
                height += own.map_or(0.0, |p| p.top);
            }
        }
        height += own.map_or(0.0, |p| p.bottom);
        let within = group
            .first()
            .map_or_else(Holders::default, |(_, b)| Holders::of(&b.frame, depth));
        let exclusion = Exclusion {
            right: f.right,
            edge,
            height: 0.0,
        };
        let wrap = Wrap {
            exclusion,
            height,
            within,
        };
        (sets, Some(wrap))
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
            sized: false,
            last: None,
            dir: frame.dir,
            runs: &[],
        }
    }

    /// The block's look for its type: the book's unless Book design is Off.
    fn typed<'f>(&self, frame: &'f Frame) -> Option<&'f Look> {
        let look = frame.look.as_deref()?;
        (self.typo.design != Design::Off).then_some(look)
    }

    /// The block's look for its layout, if the Book design setting uses it.
    fn look<'f>(&self, frame: &'f Frame) -> Option<&'f Look> {
        let look = frame.look.as_deref()?;
        match self.typo.design {
            Design::On => Some(look),
            Design::Auto if look.designed => Some(look),
            Design::Auto | Design::Off => None,
        }
    }

    /// Book type moved by the reader's settings; the caller adds faces.
    fn designed(&self, role: Role, frame: &Frame, look: &Look) -> Style<'a> {
        let (typo, em) = (self.typo, self.em_of(frame));
        let pre = role == Role::Preformatted;
        let line = match (look.line_height, role) {
            (Some(l), _) => l * typo.line_height / READER.1,
            (None, Role::Heading(_)) => HEADING_LINE_HEIGHT,
            (None, _) => typo.line_height,
        };
        let size = (look.size * em).clamp(MIN_SIZE, MAX_SIZE);
        // The reader's indent adds to the book's, as in prose.
        let body = role == Role::Paragraph && !frame.soft && frame.marker.is_none();
        let indent = if body { typo.indent * size } else { 0.0 };
        Style {
            faces: if pre {
                &self.stacks.mono
            } else {
                &self.stacks.body
            },
            size,
            line_height: line.max(MIN_LINE),
            weight: (look.weight + typo.weight - READER.3).clamp(100.0, 900.0),
            align: if pre { Align::Start } else { frame.align },
            pre,
            justify: (look.justify || typo.justify) && !pre && frame.align == Align::Start,
            hyphens: (typo.hyphenate && !pre).then_some(self.lang),
            indent: look.indent * em + indent,
            letter: look.letter * em + typo.letter_spacing * size,
            word: look.word * em + typo.word_spacing,
            sized: true,
            last: look.last.filter(|_| !pre),
            ..self.body(frame)
        }
    }

    /// A look's families, then `rest`; prose keeps only embedded fonts.
    fn look_faces(&self, look: &Look, laid: bool, rest: &[FontFamilyName<'static>]) -> Stack {
        let mut tail = rest;
        let mut names = Vec::new();
        for name in &look.faces {
            match name.as_str() {
                "sans-serif" | "system-ui" | "ui-sans-serif" => tail = &self.stacks.heading,
                "monospace" | "ui-monospace" => tail = &self.stacks.mono,
                _ if laid || name.starts_with(BOOK_FACE) => names.push(name),
                _ => {}
            }
        }
        look_stack(&names, tail)
    }

    fn text_style(&self, role: Role, frame: &Frame) -> Style<'a> {
        if let Some(look) = self.typed(frame) {
            return self.designed(role, frame, look);
        }
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
        let frame = &block.frame;
        let designed = self.look(frame).is_some();
        let typed = self.typed(frame).is_some();
        let em = if designed {
            self.design_em()
        } else {
            self.typo.font_size
        };
        let (left, room, mut boxes, cell) = match (designed, typed) {
            (true, _) => self.boxes(frame, column),
            (false, true) => self.painted(frame, column),
            (false, false) => (0.0, column, Vec::new(), None),
        };
        // Deep nesting stops indenting at half the column.
        let indent = (frame.inset * em).clamp(0.0, room / 2.0);
        // Right to left, lists and quotes indent from the right.
        let rtl = right_to_left(block);
        let mut x = if rtl { left } else { left + indent };
        let mut width = room - indent;
        let float = self.float_box.filter(|_| designed).map(|(fx, fw)| {
            // The float's own boxes wrap its narrower content.
            let depth = frame.float.as_ref().map_or(0, |f| usize::from(f.depth));
            for p in boxes.iter_mut().skip(depth) {
                let (near, far) = (x - p.x, p.x + p.width - x - width);
                (p.x, p.width) = (fx - near, fw + near + far);
            }
            (x, width) = (fx, fw);
            FloatAt {
                key: float_key(frame).unwrap_or(0),
                depth,
                x: fx,
                width: fw,
            }
        });
        let beside = self.exclusion.filter(|_| designed).map(|e| {
            let (l, r) = match e.right {
                true => (0.0, x + width - e.edge),
                false => (e.edge - x, 0.0),
            };
            let most = (width - 2.0 * em).max(0.0);
            Beside {
                left: l.clamp(0.0, most),
                right: r.clamp(0.0, most),
                height: e.height,
            }
        });
        let edged = || {
            frame
                .boxes
                .iter()
                .any(|b| b.rule || b.edges.iter().any(|e| e.draws()))
        };
        let (draw, laid, units, starts) = match &block.content {
            Content::Text(text) => self.text(text, frame, width, beside),
            Content::Rule => {
                let unit = (0.0, RULE_HEIGHT * em);
                (Draw::Rule, Laid::None, vec![unit], vec![0])
            }
            Content::Space(h) if typed => {
                let unit = (0.0, (h * em).max(0.0));
                (Draw::Space, Laid::None, vec![unit], vec![0])
            }
            // Read as prose, a bordered blank is a plain rule.
            Content::Space(_) if edged() => {
                let unit = (0.0, RULE_HEIGHT * em);
                (Draw::Rule, Laid::None, vec![unit], vec![0])
            }
            Content::Space(_) => (Draw::Space, Laid::None, vec![(0.0, 0.0)], vec![0]),
            Content::Image {
                picture,
                cover,
                width: asked,
            } => {
                let (rect, unit) = if *cover {
                    cover_rect(picture, column, height)
                } else {
                    let wanted = match asked {
                        Some(l) => resolve(*l, width, em),
                        None => picture.width as f32,
                    };
                    image_rect(picture, frame.align, (wanted, width), height)
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
                .map(|b| if rtl { column - b - BAR_ROOM } else { b })
                .collect(),
            rtl,
            draw,
            units,
            starts,
            keep_with_next: block.is_heading() || (designed && frame.keep.with_next),
            break_before: block.break_before,
            tight: frame.tight,
            // The book's margins, scaled as the reader's paragraph gap.
            before: (self.typed(frame)).map(|_| frame.before * em * self.typo.block_gap / READER.2),
            lines: matches!(block.content, Content::Text(_)),
            orphans: lines_kept(frame.keep.orphans, designed),
            widows: lines_kept(frame.keep.widows, designed),
            whole: designed && frame.keep.whole,
            heads,
            boxes,
            cell,
            float,
            beside: self.exclusion.filter(|_| designed),
            clear: false,
            place: self.place_of(frame).filter(|_| designed),
            rotate: if designed { frame.rotate } else { 0.0 },
        };
        (set, laid)
    }

    /// Prose: only painted boxes, at the reader's size, inside the column.
    fn painted(&self, frame: &Frame, column: f32) -> (f32, f32, Vec<Placed>, Option<CellAt>) {
        let em = self.typo.font_size;
        let (mut x, mut width) = (0.0, column);
        let mut placed = Vec::new();
        for b in frame.boxes.iter().filter(|b| painted(b)) {
            let mut p = place(b, (x, width), em);
            if b.width.is_none() && p.width < width / 2.0 {
                (p.x, p.width) = (x, width);
            }
            let end = (p.x + p.width).min(column);
            p.x = p.x.max(0.0);
            p.width = (end - p.x).max(1.0);
            p.centre = None;
            let inside = (b.border[1] + b.border[3] + b.padding[1] + b.padding[3]) * em;
            x = p.x + (b.border[3] + b.padding[3]) * em;
            width = (p.width - inside).max(1.0);
            placed.push(p);
        }
        (x, width, placed, None)
    }

    /// Places the boxes and cell in `column`: content left, width, both.
    fn boxes(&self, frame: &Frame, column: f32) -> (f32, f32, Vec<Placed>, Option<CellAt>) {
        let em = self.design_em();
        let (mut x, mut width) = (0.0, column);
        let mut placed = Vec::with_capacity(frame.boxes.len());
        let mut at = None;
        let depth = frame.cell.as_ref().map(|c| usize::from(c.depth));
        let mut bounds = None;
        for (i, b) in frame.boxes.iter().enumerate() {
            if depth == Some(i) {
                at = frame
                    .cell
                    .as_ref()
                    .map(|c| enter(c, (&mut x, &mut width), em, self.place_of(frame)));
                bounds = Some((x, x + width));
            }
            let mut p = place(b, (x, width), em);
            if let Some((left, right)) = bounds {
                // Inside a cell, negative margins stop at its edges.
                let end = (p.x + p.width).min(right);
                p.x = p.x.max(left);
                p.width = (end - p.x).max(1.0);
            }
            let inside = (b.border[1] + b.border[3] + b.padding[1] + b.padding[3]) * em;
            x = p.x + (b.border[3] + b.padding[3]) * em;
            width = (p.width - inside).max(1.0);
            placed.push(p);
        }
        if at.is_none() && depth.is_some_and(|d| d >= frame.boxes.len()) {
            at = frame
                .cell
                .as_ref()
                .map(|c| enter(c, (&mut x, &mut width), em, self.place_of(frame)));
        }
        (x, width, placed, at)
    }

    fn text(
        &mut self,
        text: &Text,
        frame: &Frame,
        width: f32,
        beside: Option<Beside>,
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
        // The book's families first, the reader's after.
        let laid = self.look(frame).is_some();
        let faces = (self.typed(frame))
            .filter(|l| !l.faces.is_empty())
            .map(|l| self.look_faces(l, laid, style.faces));
        let style = match &faces {
            Some(faces) => Style {
                faces: faces.as_slice(),
                ..style
            },
            None => style,
        };
        let runs: Vec<Stack> = match self.typed(frame) {
            Some(_) => (text.faces.iter())
                .map(|names| {
                    let look = Look {
                        faces: names.clone(),
                        ..Look::default()
                    };
                    self.look_faces(&look, laid, style.faces)
                })
                .collect(),
            None => Vec::new(),
        };
        let style = Style {
            runs: &runs,
            ..style
        };
        let initial = self.initial(text, style);
        let beside = match &initial {
            Some((_, room)) => Some(match beside {
                Some(b) => Beside {
                    left: b.left + room.left,
                    height: b.height.max(room.height),
                    ..b
                },
                None => *room,
            }),
            None => beside,
        };
        let mut para = layout_text(self.fonts, text, style, (Some(inner), beside));
        if let Some(extra) = text.first_line.filter(|_| style.sized) {
            para = self.first_line(text, (extra, para), style, (inner, beside));
        }
        self.notes(text, &mut para, style);
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
            let colour = frame.marker_colour;
            t.push(
                m,
                Emphasis {
                    colour,
                    ..Emphasis::default()
                },
            );
            let style = Style {
                align: Align::Start,
                pre: false,
                justify: false,
                hyphens: None,
                indent: 0.0,
                ..style
            };
            if let Some(picture) = &frame.marker_picture {
                t.inlines.push(marker_inline(picture, t.text.len()));
                t.push(WORD_JOINER, Emphasis::default());
            }
            Box::new(layout_text(self.fonts, &t, style, (None, None)))
        });
        let starts = std::mem::take(&mut para.starts);
        let para = Box::new(para);
        let initial = initial.map(|(i, _)| Box::new(i));
        let laid = Laid::Text {
            para,
            marker,
            initial,
        };
        (Draw::Text { pad, shaded }, laid, units, starts)
    }

    /// Lays `para`'s ruby notes small, each in its base's emphasis.
    fn notes(&mut self, text: &Text, para: &mut Para, style: Style<'_>) {
        let small = Style {
            size: (style.size * RUBY_SCALE).clamp(MIN_SIZE, MAX_SIZE),
            line_height: 1.0,
            align: Align::Start,
            pre: false,
            justify: false,
            hyphens: None,
            indent: 0.0,
            letter: 0.0,
            word: 0.0,
            last: None,
            dir: Dir::Auto,
            ..style
        };
        for (note, own) in para.ruby.iter_mut().zip(&text.ruby) {
            let at = own.range.start;
            let base = (text.spans.iter())
                .find(|s| s.range.start <= at && at < s.range.end)
                .map(|s| s.emphasis)
                .unwrap_or_default();
            let mut t = Text::new(Role::Paragraph);
            let plain = Emphasis {
                size: 0,
                script: Script::Normal,
                underline: false,
                strike: false,
                case: Case::Normal,
                ..base
            };
            t.push(&note.text, plain);
            t.faces.clone_from(&text.faces);
            note.para = Some(Box::new(layout_text(self.fonts, &t, small, (None, None))));
        }
    }

    /// The drop cap `text` starts with, and the room its lines leave it.
    fn initial(&mut self, text: &Text, style: Style<'_>) -> Option<(Initial, Beside)> {
        let letter = text
            .text
            .get(..text.initial)
            .filter(|l| style.sized && !l.is_empty())?;
        let own = (text.spans.first()).filter(|s| s.range.start == 0);
        let emphasis = own.map(|s| s.emphasis).unwrap_or_default();
        let size = style.size * f32::from(emphasis.size.max(100)) / 100.0;
        let mut t = Text::new(Role::Paragraph);
        let plain = Emphasis {
            size: 0,
            script: Script::Normal,
            underline: false,
            ..emphasis
        };
        t.push(letter, plain);
        let big = Style {
            size: size.clamp(MIN_SIZE, MAX_SIZE),
            line_height: 1.0,
            align: Align::Start,
            pre: false,
            justify: false,
            hyphens: None,
            indent: 0.0,
            ..style
        };
        let para = layout_text(self.fonts, &t, big, (None, None));
        let line = style.size * style.line_height;
        // It spans the lines from the first's capitals to its own baseline.
        let drop = ((size - style.size) * CAP_HEIGHT / line.max(1.0)).round();
        let lines = (drop as usize + 1).clamp(1, MAX_DROP);
        let room = Beside {
            left: para.layout.width() + DROP_GAP * style.size,
            right: 0.0,
            height: (lines - 1) as f32 * line + 1.0,
        };
        Some((Initial { para, line: lines }, room))
    }

    /// Lays `text` again with `extra` over what its first line holds.
    fn first_line(
        &mut self,
        text: &Text,
        (extra, mut para): (Emphasis, Para),
        style: Style<'_>,
        (inner, beside): (f32, Option<Beside>),
    ) -> Para {
        let mut end = para.starts.get(1).copied().unwrap_or(text.text.len());
        // A styled line may hold less; a few passes settle it.
        for _ in 0..FIRST_LINE_PASSES {
            let styled = over_first_line(text, end, extra);
            para = layout_text(self.fonts, &styled, style, (Some(inner), beside));
            let held = para.starts.get(1).copied().unwrap_or(text.text.len());
            if held >= end {
                break;
            }
            end = held;
        }
        para
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
                .map(|c| layout_text(self.fonts, c, style, (Some(inner), None)))
                .collect();
            let tallest = cells.iter().map(|c| c.layout.height()).fold(0.0, f32::max);
            let h = tallest + 2.0 * pad_y;
            units.push((y, y + h));
            y += h;
            rows.push(cells);
            heads.push(row.head);
        }
        let pad = [pad_x, pad_y];
        let rule = table.rule;
        (
            Grid {
                column,
                pad,
                heads,
                rule,
            },
            rows,
            units,
        )
    }
}

/// A book's length in layout units; percentages are of `width`.
pub(crate) fn resolve(length: Length, width: f32, em: f32) -> f32 {
    match length {
        Length::Em(e) => e * em,
        Length::Percent(p) => width * p / 100.0,
        Length::Mix { em: e, percent: p } => e * em + width * p / 100.0,
    }
}

/// Box `b` in the room at `x`, `width` across: margins, width, centring.
fn place(b: &Arc<Boxed>, (x, width): (f32, f32), em: f32) -> Placed {
    let inside = (b.border[1] + b.border[3] + b.padding[1] + b.padding[3]) * em;
    let side = |m: Option<Length>| m.map(|l| resolve(l, width, em));
    let (left, right) = (side(b.margin[0]), side(b.margin[1]));
    let content = match b.width {
        // A set width wider than the room shrinks to it.
        Some(l) => resolve(l, width, em).min(width - inside),
        None => width - left.unwrap_or(0.0) - right.unwrap_or(0.0) - inside,
    };
    let outer = content.max(0.0) + inside;
    let left = match (left, right, b.width) {
        (l, _, None) => l.unwrap_or(0.0),
        (None, None, Some(_)) => ((width - outer) / 2.0).max(0.0),
        (None, Some(r), Some(_)) => (width - outer - r).max(0.0),
        (Some(l), _, Some(_)) => l,
    };
    Placed {
        key: Arc::as_ptr(b) as usize,
        x: x + left,
        width: outer,
        border: b.border.map(|w| w * em),
        edges: b.edges,
        colours: b.colours,
        top: b.top * em,
        bottom: b.bottom * em,
        centre: b.centre,
        inside_top: (b.border[0] + b.padding[0]) * em,
        height: b.height.map(|h| h * em),
        fill: b.fill,
        backdrop: b.backdrop.clone(),
        em,
        radius: b.radius.map_or(0.0, |r| resolve(r, outer, em).max(0.0)),
        finish: b.finish.clone(),
        whole: b.whole,
    }
}

/// Whether a box shows: a drawn border, a background or its picture.
fn painted(b: &Boxed) -> bool {
    let edged = (b.edges.iter().zip(&b.border)).any(|(e, w)| e.draws() && *w > 0.0);
    let finished = b.finish.as_ref().is_some_and(|f| f.shows());
    edged || b.fill.is_some_and(|f| f > 0.02) || b.backdrop.is_some() || finished
}

/// The float block `frame` is in, by its address.
fn float_key(frame: &Frame) -> Option<usize> {
    frame.float.as_ref().map(|f| Arc::as_ptr(f) as usize)
}

/// What holds a float, by address: its outer boxes and its cell.
#[derive(Debug, Clone, Default, PartialEq)]
struct Holders {
    boxes: Vec<usize>,
    cell: Option<(usize, u16)>,
}

impl Holders {
    /// Block `frame`'s first `depth` boxes and its cell.
    fn of(frame: &Frame, depth: usize) -> Self {
        let boxes = frame.boxes.iter().take(depth);
        Self {
            boxes: boxes.map(|b| Arc::as_ptr(b) as usize).collect(),
            cell: cell_key(frame),
        }
    }

    /// Whether block `frame` sits in the same holders.
    fn hold(&self, frame: &Frame) -> bool {
        let mine = frame.boxes.iter().map(|b| Arc::as_ptr(b) as usize);
        let inside = frame.boxes.len() >= self.boxes.len();
        inside && self.boxes.iter().zip(mine).all(|(a, b)| *a == b) && self.cell == cell_key(frame)
    }
}

/// Block `frame`'s grid row, by address, and column.
fn cell_key(frame: &Frame) -> Option<(usize, u16)> {
    frame
        .cell
        .as_ref()
        .map(|c| (Arc::as_ptr(&c.row) as usize, c.column))
}

/// How wide a set block's content is, as a float shrinks to it.
fn natural(set: &Set, laid: &Laid) -> f32 {
    match (laid, &set.draw) {
        (Laid::Text { para, .. }, Draw::Text { pad, .. }) => para.layout.width() + 2.0 * pad,
        (_, Draw::Image { rect, .. }) => rect[2],
        _ => set.width,
    }
}

/// Narrows `x`, `width` to cell `c`'s column of its row.
fn enter(c: &Cell, (x, width): (&mut f32, &mut f32), em: f32, at: Option<CellPlace>) -> CellAt {
    let column = usize::from(c.column);
    let (left, span) = match at {
        Some(p) => (p.left, p.width),
        None => cell_span(&c.row, column, *width, em),
    };
    *x += left;
    *width = span.max(1.0);
    let line = at.map_or(0, |p| p.line);
    let gap = c.row.columns.flex.map_or(0.0, |f| f.gap.max(0.0));
    CellAt {
        // A wrapped line is its own row, keyed inside the row's allocation.
        key: Arc::as_ptr(&c.row) as usize + usize::from(line),
        column,
        depth: usize::from(c.depth),
        valign: c.row.valign.get(column).copied().unwrap_or_default(),
        before: if line == 0 { c.row.before } else { gap } * em,
        grid: Arc::as_ptr(&c.row.columns) as usize,
        // A wrapped flex line is a row of its own: nothing spans it.
        down: c.row.down.get(column).copied().unwrap_or(1).max(1),
    }
}

/// The grid block `frame` is in: its table's or flex row's columns.
fn grid_key(frame: &Frame) -> Option<usize> {
    frame
        .cell
        .as_ref()
        .map(|c| Arc::as_ptr(&c.row.columns) as usize)
}

/// Block `frame`'s grid item: its row, by address, and its index.
fn item_key(frame: &Frame) -> Option<(usize, u16)> {
    frame
        .cell
        .as_ref()
        .map(|c| (Arc::as_ptr(&c.row) as usize, c.column))
}

/// A table's columns from its items' extents, shared by its rows.
fn table_places<'a>(
    extents: &[Extent],
    rows: impl Iterator<Item = &'a Cell>,
    (room, fill, centre): (f32, bool, bool),
    places: &mut HashMap<(usize, u16), CellPlace>,
) {
    // Each item's columns: its first and how many.
    let mut spans: HashMap<(usize, u16), (usize, usize)> = HashMap::new();
    for c in rows {
        let item = usize::from(c.column);
        let start = c.row.starts.get(item).map_or(item, |s| usize::from(*s));
        let span = c.row.spans.get(item).map_or(1, |s| usize::from(*s)).max(1);
        let key = (Arc::as_ptr(&c.row) as usize, c.column);
        spans.insert(key, (start.min(MAX_COLUMNS - 1), span));
    }
    let n = spans
        .values()
        .map(|(s, w)| s + w)
        .max()
        .unwrap_or(1)
        .clamp(1, MAX_COLUMNS);
    let (mut least, mut most, mut set) = (vec![0.0f32; n], vec![0.0f32; n], vec![false; n]);
    let mut wide = Vec::new();
    for e in extents {
        let (start, span) = spans.get(&(e.row, e.item)).copied().unwrap_or((0, 1));
        if span > 1 {
            wide.push((start, span.min(n - start.min(n - 1)), e));
            continue;
        }
        if let (Some(l), Some(m), Some(s)) = (
            least.get_mut(start),
            most.get_mut(start),
            set.get_mut(start),
        ) {
            (*l, *m) = (l.max(e.least), m.max(e.most));
            *s |= e.set;
        }
    }
    // A spanning item's excess spreads over its columns evenly.
    for (start, span, e) in wide {
        let cols = start..(start + span).min(n);
        let count = cols.len().max(1) as f32;
        let have: (f32, f32) = cols.clone().fold((0.0, 0.0), |(l, m), c| {
            (
                l + least.get(c).unwrap_or(&0.0),
                m + most.get(c).unwrap_or(&0.0),
            )
        });
        for c in cols {
            if let (Some(l), Some(m)) = (least.get_mut(c), most.get_mut(c)) {
                *l += (e.least - have.0).max(0.0) / count;
                *m += (e.most - have.1).max(0.0) / count;
            }
        }
    }
    let widths = distribute(&least, &most, room, fill);
    let total: f32 = widths.iter().sum();
    let offset = if centre {
        ((room - total) / 2.0).max(0.0)
    } else {
        0.0
    };
    for (key, (start, span)) in spans {
        let left = offset + widths.iter().take(start).sum::<f32>();
        let width = widths.iter().skip(start).take(span).sum();
        places.insert(
            key,
            CellPlace {
                left,
                width,
                line: 0,
            },
        );
    }
}

/// Column widths in `room`: their most, else between least and most.
fn distribute(least: &[f32], most: &[f32], room: f32, fill: bool) -> Vec<f32> {
    let (low, high): (f32, f32) = (least.iter().sum(), most.iter().sum());
    let n = most.len().max(1) as f32;
    if high <= room {
        if fill && high > 0.0 {
            return most.iter().map(|m| m * room / high).collect();
        }
        if fill {
            return vec![room / n; most.len()];
        }
        return most.to_vec();
    }
    if low < room && high > low {
        let share = (room - low) / (high - low);
        return least
            .iter()
            .zip(most)
            .map(|(l, m)| l + (m - l) * share)
            .collect();
    }
    if low > 0.0 {
        return least.iter().map(|l| l * room / low).collect();
    }
    vec![room / n; most.len()]
}

/// A flex row's items placed: wrapped, grown or shrunk, then justified.
fn flex_places(
    items: &[&Extent],
    (room, gap): (f32, f32),
    (flex, grow): (crate::block::Flex, &[f32]),
    places: &mut HashMap<(usize, u16), CellPlace>,
) {
    let basis = |e: &Extent| e.most.clamp(e.least.min(room), room);
    let mut lines: Vec<Vec<&Extent>> = vec![Vec::new()];
    let mut used = 0.0;
    for e in items {
        let w = basis(e);
        let full = lines
            .last()
            .is_some_and(|l| !l.is_empty() && used + gap + w > room);
        if flex.wrap && full && lines.len() < MAX_FLEX_LINES {
            lines.push(Vec::new());
            used = 0.0;
        }
        used += if lines.last().is_some_and(Vec::is_empty) {
            w
        } else {
            gap + w
        };
        if let Some(line) = lines.last_mut() {
            line.push(e);
        }
    }
    for (n, line) in lines.iter().enumerate() {
        let k = line.len().max(1) as f32;
        let mut widths: Vec<f32> = line.iter().map(|e| basis(e)).collect();
        let gaps = gap * (k - 1.0);
        let mut free = room - widths.iter().sum::<f32>() - gaps;
        let grows: Vec<f32> = line
            .iter()
            .map(|e| {
                grow.get(usize::from(e.item))
                    .copied()
                    .unwrap_or(0.0)
                    .max(0.0)
            })
            .collect();
        let total_grow: f32 = grows.iter().sum();
        if free > 0.0 && total_grow > 0.0 {
            for (w, g) in widths.iter_mut().zip(&grows) {
                *w += free * g / total_grow;
            }
            free = 0.0;
        } else if free < 0.0 {
            let sum: f32 = widths.iter().sum::<f32>().max(1.0);
            let short = -free;
            for (w, e) in widths.iter_mut().zip(line) {
                *w = (*w - short * *w / sum).max(e.least.min(room / k));
            }
            free = 0.0;
        }
        let (start, extra) = match flex.justify {
            Justify::Start => (0.0, 0.0),
            Justify::Center => (free / 2.0, 0.0),
            Justify::End => (free, 0.0),
            Justify::Between if k > 1.0 => (0.0, free / (k - 1.0)),
            Justify::Between => (0.0, 0.0),
            Justify::Around => (free / (2.0 * k), free / k),
            Justify::Evenly => (free / (k + 1.0), free / (k + 1.0)),
        };
        let mut left = start.max(0.0);
        for (e, w) in line.iter().zip(widths) {
            let line = u16::try_from(n).unwrap_or(0);
            places.insert(
                (e.row, e.item),
                CellPlace {
                    left,
                    width: w,
                    line,
                },
            );
            left += w + gap + extra;
        }
    }
}

/// A column's left and width; set widths shrink, unset ones share the rest.
fn cell_span(row: &GridRow, column: usize, width: f32, em: f32) -> (f32, f32) {
    let n = row.widths.len().max(column + 1);
    let asked: Vec<Option<f32>> = (0..n)
        .map(|i| row.widths.get(i).copied().flatten())
        .map(|l| l.map(|l| resolve(l, width, em).max(0.0)))
        .collect();
    let fixed: f32 = asked.iter().flatten().sum();
    let free = asked.iter().filter(|a| a.is_none()).count();
    let scale = if fixed > width && fixed > 0.0 {
        width / fixed
    } else {
        1.0
    };
    let share = match free {
        0 => 0.0,
        free => (width - fixed * scale).max(0.0) / free as f32,
    };
    let widths = asked.iter().map(|a| a.map_or(share, |w| w * scale));
    let left = widths.clone().take(column).sum();
    (left, widths.clone().nth(column).unwrap_or(0.0))
}

/// Whether a block reads right to left: as set, or by its first strong letter.
fn right_to_left(block: &Block) -> bool {
    match (block.frame.dir, &block.content) {
        (Dir::Rtl, _) => true,
        (Dir::Auto, Content::Text(t)) => {
            let strong = t.text.chars().find(|&c| rtl_letter(c) || c.is_alphabetic());
            strong.is_some_and(rtl_letter)
        }
        _ => false,
    }
}

/// Hebrew, Arabic, Syriac, Thaana, N'Ko and their presentation forms.
fn rtl_letter(c: char) -> bool {
    matches!(
        u32::from(c),
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF
    )
}

/// A look's family names, then `rest`.
fn look_stack(names: &[&String], rest: &[FontFamilyName<'static>]) -> Stack {
    let named = names
        .iter()
        .map(|n| FontFamilyName::Named(Cow::Owned((*n).clone())));
    named.chain(rest.iter().cloned()).collect()
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

fn layout_text(
    fonts: &mut Fonts,
    text: &Text,
    style: Style<'_>,
    (width, beside): (Option<f32>, Option<Beside>),
) -> Para {
    let hyphenated = style.hyphens.map(|lang| hyphen::hyphenate(text, lang));
    let text = hyphenated.as_ref().map_or(text, |h| &h.text);
    let Fonts { fonts, layout } = fonts;
    let ink = Ink {
        tone: style.tone,
        code: false,
        colour: None,
        shadow: None,
        hidden: false,
    };
    let (shown, small) = cased(text);
    let mut b = layout.ranged_builder(fonts, &shown, 1.0, false);
    b.set_base_direction(match style.dir {
        Dir::Auto => BaseDirection::Auto,
        Dir::Ltr => BaseDirection::Ltr,
        Dir::Rtl => BaseDirection::Rtl,
    });
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
    for note in &text.ruby {
        // Room above the base for its note.
        let tall = LineHeight::FontSizeRelative(style.line_height + RUBY_ROOM);
        b.push(StyleProperty::LineHeight(tall), note.range.clone());
    }
    for span in &text.spans {
        let range = span.range.clone();
        let Emphasis {
            italic,
            bold,
            light,
            code,
            size,
            script,
            case: _,
            underline,
            strike,
            colour,
            shadow,
            hidden,
            features,
            face,
        } = span.emphasis;
        if !features.is_empty() {
            let source = FontFeatures::Source(Cow::Owned(features.source()));
            b.push(StyleProperty::FontFeatures(source), range.clone());
        }
        if script != Script::Normal {
            let shift = match script {
                Script::Sub => VerticalAlign::SUB,
                _ => VerticalAlign::SUPER,
            };
            b.push(StyleProperty::VerticalAlign(shift), range.clone());
            if !style.sized {
                let size = StyleProperty::FontSize(style.size * SCRIPT_SCALE);
                b.push(size, range.clone());
            }
        }
        if underline {
            b.push(StyleProperty::Underline(true), range.clone());
        }
        if strike {
            b.push(StyleProperty::Strikethrough(true), range.clone());
        }
        if size != 0 && style.sized {
            let size = style.size * f32::from(size) / 100.0;
            let size = StyleProperty::FontSize(size.clamp(MIN_SIZE, MAX_SIZE));
            b.push(size, range.clone());
        }
        if italic {
            b.push(StyleProperty::FontStyle(FontStyle::Italic), range.clone());
        }
        if bold {
            let weight = FontWeight::new(bolder(style.weight));
            b.push(StyleProperty::FontWeight(weight), range.clone());
        } else if light {
            let weight = FontWeight::new((style.weight - LIGHTER).max(100.0));
            b.push(StyleProperty::FontWeight(weight), range.clone());
        }
        let own = usize::from(face).checked_sub(1);
        let own = own.and_then(|i| style.runs.get(i)).map(Vec::as_slice);
        if let Some(stack) = own {
            let family = FontFamily::List(Cow::Borrowed(stack));
            b.push(StyleProperty::FontFamily(family), range.clone());
        }
        if code && !style.pre {
            let mono = FontFamily::List(Cow::Borrowed(own.unwrap_or(style.mono)));
            b.push(StyleProperty::FontFamily(mono), range.clone());
            b.push(
                StyleProperty::FontSize(style.size * CODE_SCALE),
                range.clone(),
            );
            b.push(
                StyleProperty::Brush(Ink {
                    code: true,
                    colour,
                    shadow,
                    hidden,
                    ..ink
                }),
                range,
            );
        } else if colour.is_some() || shadow.is_some() || hidden {
            let brush = Ink {
                colour,
                shadow,
                hidden,
                ..ink
            };
            b.push(StyleProperty::Brush(brush), range);
        }
    }
    for (range, scale) in small {
        let size = style.size * scale * SMALL_CAPS;
        b.push(
            StyleProperty::FontSize(size.clamp(MIN_SIZE, MAX_SIZE)),
            range,
        );
    }
    for link in &text.links {
        b.push(StyleProperty::Underline(true), link.range.clone());
    }
    // A drop cap's letter keeps its place in the text but shows apart.
    let initial = text.initial.min(shown.len());
    if style.sized && initial > 0 && shown.is_char_boundary(initial) {
        b.push(StyleProperty::FontSize(HIDDEN_SIZE), 0..initial);
    }
    for (n, inline) in text.inlines.iter().enumerate().take(MAX_INLINES) {
        let size = |ems: f32| (ems * style.size).clamp(1.0, MAX_SIZE * 8.0);
        b.push_inline_box(InlineBox {
            id: n as u64,
            kind: InlineBoxKind::InFlow,
            index: inline.at.min(shown.len()),
            width: size(inline.width),
            height: size(inline.height),
            baseline: None,
            vertical_align: rise(inline.rise, style.size),
        });
    }
    let mut layout = b.build(&shown);
    if style.indent > 0.0 {
        layout.set_text_indent(style.indent, IndentOptions::default());
    }
    match (width, beside) {
        (Some(width), Some(beside)) => break_beside(&mut layout, width, beside),
        _ => layout.break_all_lines(width),
    }
    let last = style.last.map(|last| match last {
        LastLine::Start => Alignment::Start,
        LastLine::Center => Alignment::Center,
        LastLine::End => Alignment::End,
        LastLine::Justify => Alignment::Justify,
    });
    let options = AlignmentOptions {
        last_line_alignment: last,
        ..AlignmentOptions::default()
    };
    layout.align(alignment(&style), options);
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
        ruby: (text.ruby.iter())
            .map(|a| Note {
                range: a.range.clone(),
                text: a.text.clone(),
                para: None,
            })
            .collect(),
        pictures: text
            .inlines
            .iter()
            .map(|i| Arc::clone(&i.picture))
            .collect(),
    }
}

/// A list marker's picture at `at`, at most a line of text tall.
fn marker_inline(picture: &Arc<Picture>, at: usize) -> Inline {
    let ems = |px: u32| px.max(1) as f32 / MARKER_PX;
    let (width, height) = (ems(picture.width), ems(picture.height));
    let scale = (1.0 / height).min(1.0);
    Inline {
        at,
        picture: Arc::clone(picture),
        width: width * scale,
        height: height * scale,
        rise: Rise::Baseline,
    }
}

/// `text` with `extra` added over its first `end` bytes.
fn over_first_line(text: &Text, end: usize, extra: Emphasis) -> Text {
    let len = text.text.len();
    let mut cuts: Vec<usize> = (text.spans.iter())
        .flat_map(|s| [s.range.start, s.range.end])
        .chain([0, end, len])
        .filter(|&c| c <= len && text.text.is_char_boundary(c))
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = Text {
        text: String::with_capacity(len),
        spans: Vec::new(),
        ..text.clone()
    };
    for pair in cuts.windows(2) {
        let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        let own = (text.spans.iter())
            .find(|s| s.range.start <= a && a < s.range.end)
            .map(|s| s.emphasis)
            .unwrap_or_default();
        let e = match b <= end {
            true => Emphasis {
                italic: own.italic || extra.italic,
                bold: own.bold || extra.bold,
                light: own.light && !extra.bold,
                size: if own.size == 0 { extra.size } else { own.size },
                case: if extra.case == Case::Normal {
                    own.case
                } else {
                    extra.case
                },
                underline: own.underline || extra.underline,
                strike: own.strike || extra.strike,
                colour: own.colour.or(extra.colour),
                shadow: own.shadow.or(extra.shadow),
                ..own
            },
            false => own,
        };
        out.push(text.text.get(a..b).unwrap_or_default(), e);
    }
    out
}

/// Breaks `layout`'s lines in `width`, those beside a float narrower.
fn break_beside(layout: &mut Layout<Ink>, width: f32, beside: Beside) {
    let mut lines = layout.break_lines();
    lines.state_mut().set_layout_max_advance(width);
    loop {
        let top = lines.state().line_y() as f32;
        let (x, room) = match top < beside.height {
            true => (beside.left, width - beside.left - beside.right),
            false => (0.0, width),
        };
        lines.state_mut().set_line_x(x);
        lines
            .state_mut()
            .set_line_max_advance(room.clamp(1.0, width));
        if lines.break_next().is_none() {
            break;
        }
    }
    lines.finish();
}

/// `text` in its shown cases, offsets kept; small capitals' runs.
fn cased(text: &Text) -> (Cow<'_, str>, Vec<(Range<usize>, f32)>) {
    let mut small: Vec<(Range<usize>, f32)> = Vec::new();
    if text.spans.iter().all(|s| s.emphasis.case == Case::Normal) {
        return (Cow::Borrowed(&text.text), small);
    }
    // A letter swapped only for one as long, keeping every offset.
    let swap = |c: char, upper: bool| {
        let mut to: Vec<char> = match upper {
            true => c.to_uppercase().collect(),
            false => c.to_lowercase().collect(),
        };
        let one = to.len() == 1;
        to.pop().filter(|t| one && t.len_utf8() == c.len_utf8())
    };
    let mut shown = String::with_capacity(text.text.len());
    let mut spans = text.spans.iter().peekable();
    for (i, c) in text.text.char_indices() {
        while spans.next_if(|s| s.range.end <= i).is_some() {}
        let span = spans.peek().filter(|s| s.range.start <= i);
        let case = span.map_or(Case::Normal, |s| s.emphasis.case);
        let out = match case {
            Case::Normal => None,
            Case::Upper => swap(c, true),
            Case::Lower => swap(c, false),
            Case::Small => swap(c, true).filter(|_| c.is_lowercase()),
            Case::AllSmall if c.is_uppercase() => Some(c),
            Case::AllSmall => swap(c, true).filter(|_| c.is_lowercase()),
        };
        shown.push(out.unwrap_or(c));
        if matches!(case, Case::Small | Case::AllSmall) && out.is_some() {
            let size = span.map_or(0, |s| s.emphasis.size);
            let scale = if size == 0 {
                1.0
            } else {
                f32::from(size) / 100.0
            };
            let end = i + c.len_utf8();
            match small.last_mut() {
                Some((r, s)) if r.end == i && *s == scale => r.end = end,
                _ => small.push((i..end, scale)),
            }
        }
    }
    (Cow::Owned(shown), small)
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

/// `wanted` wide where the column allows, shrunk to fit the page.
fn image_rect(
    picture: &Picture,
    align: Align,
    (wanted, width): (f32, f32),
    height: f32,
) -> ([f32; 4], (f32, f32)) {
    let aspect = picture.aspect();
    let mut w = wanted.max(1.0).min(width);
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

/// The page being filled: its pieces, borders and how far down it is.
#[derive(Default)]
struct Cursor {
    pages: Vec<Vec<Piece>>,
    page: Vec<Piece>,
    decos: Vec<Vec<Deco>>,
    deco: Vec<Deco>,
    /// The book's boxes open here, outermost first.
    open: Vec<OpenBox>,
    y: f32,
    /// The page's height, which centring boxes fill.
    height: f32,
    /// Where the last float on the page ends.
    float_bottom: f32,
    /// Cells spanning rows down, waiting for those rows.
    hangs: Vec<Hang>,
}

/// A cell spanning rows down: placed with its first, aligned past its last.
struct Hang {
    /// Its grid, and its rows still to come.
    grid: usize,
    rows: u16,
    /// Where its content ends, and how it sits in the rows it spans.
    bottom: f32,
    valign: VAlign,
    /// Its pieces on the page, and its borders.
    pieces: Vec<usize>,
    decos: Range<usize>,
}

/// A book's box open on the page being filled.
struct OpenBox {
    placed: Placed,
    top: f32,
    /// Begun on an earlier page: no top border here.
    continued: bool,
    /// Centring, begun on a fresh page: the pieces and borders before it.
    centre: Option<(usize, usize)>,
}

impl OpenBox {
    /// Its borders down to `bottom`; open there unless `closed`.
    fn deco(&self, bottom: f32, closed: bool) -> Option<Deco> {
        let (mut border, mut edges) = (self.placed.border, self.placed.edges);
        if self.continued {
            (border[0], edges[0]) = (0.0, Edge::None);
        }
        if !closed {
            (border[2], edges[2]) = (0.0, Edge::None);
        }
        let edged = border
            .iter()
            .zip(&edges)
            .any(|(w, e)| *w > 0.0 && e.draws());
        let p = &self.placed;
        let finished = p.finish.as_ref().is_some_and(|f| f.shows());
        let drawn = edged || p.fill.is_some() || p.backdrop.is_some() || finished;
        let rect = [p.x, self.top, p.width, bottom - self.top];
        drawn.then(|| Deco {
            rect: rect.map(|v| if v.is_finite() { v } else { 0.0 }),
            border,
            edges,
            colours: p.colours,
            fill: p.fill,
            backdrop: p.backdrop.clone(),
            em: p.em,
            radius: p.radius,
            finish: p.finish.clone(),
            continued: [self.continued, !closed],
        })
    }
}

impl Cursor {
    fn break_page(&mut self) {
        self.settle();
        if !self.page.is_empty() {
            // A box running on is cut here and goes on overleaf.
            for open in &mut self.open {
                self.deco.extend(open.deco(self.y, false));
                (open.top, open.continued, open.centre) = (0.0, true, None);
            }
            self.pages.push(std::mem::take(&mut self.page));
            self.decos.push(std::mem::take(&mut self.deco));
        }
        (self.y, self.float_bottom) = (0.0, 0.0);
    }

    /// Ends cells spanning down early: what follows starts below them.
    fn settle(&mut self) {
        for h in std::mem::take(&mut self.hangs) {
            self.y = self.y.max(h.bottom);
        }
    }

    /// A row of `grid` ended: the cells it ends sit across their rows.
    fn row_done(&mut self, grid: usize) {
        let mut y = self.y;
        for mut h in std::mem::take(&mut self.hangs) {
            h.rows = h.rows.saturating_sub(1);
            if h.grid == grid && h.rows > 0 {
                self.hangs.push(h);
                continue;
            }
            // Taller than its rows: the last of them grows.
            let room = self.y - h.bottom;
            y = y.max(h.bottom);
            let shift = match h.valign {
                _ if room <= 0.0 => continue,
                VAlign::Top => continue,
                VAlign::Middle => room / 2.0,
                VAlign::Bottom => room,
            };
            for i in &h.pieces {
                if let Some(p) = self.page.get_mut(*i) {
                    p.dy += shift;
                }
            }
            for d in self.deco.get_mut(h.decos).unwrap_or_default() {
                d.rect[1] += shift;
            }
        }
        self.y = y;
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

    /// How many of the open boxes `boxes` is still in.
    fn common(&self, boxes: &[Placed]) -> usize {
        let same = |(o, b): &(&OpenBox, &Placed)| o.placed.key == b.key;
        self.open.iter().zip(boxes).take_while(same).count()
    }

    /// Closes the open boxes after the first `keep`, below what is placed.
    fn close(&mut self, keep: usize) {
        while self.open.len() > keep {
            let Some(open) = self.open.pop() else {
                break;
            };
            let mut bottom = self.y + open.placed.bottom;
            if let (Some((pieces, decos)), Some(share)) = (open.centre, open.placed.centre) {
                // Its content sits mid-way down the height it fills.
                let fill = (share * self.height).min(self.height - open.top);
                let shift = ((fill - (bottom - open.top)) / 2.0).max(0.0);
                for piece in self.page.iter_mut().skip(pieces) {
                    piece.dy += shift;
                }
                for deco in self.deco.iter_mut().skip(decos) {
                    deco.rect[1] += shift;
                }
                self.y += shift;
                bottom += 2.0 * shift;
            }
            if let Some(least) = open.placed.height.filter(|_| !open.continued) {
                // A set height holds the box open below short content.
                let floor = open.top + open.placed.inside_top + least;
                self.y = self.y.max(floor.min(self.height));
                bottom = bottom.max(self.y + open.placed.bottom);
            }
            self.deco.extend(open.deco(bottom, true));
        }
    }

    /// Opens what `boxes` holds past the open ones; a block goes in next.
    fn open(&mut self, boxes: &[Placed]) {
        let fresh = boxes.get(self.open.len()..).unwrap_or_default();
        // A box opening atop a page keeps its border on the page.
        let need = fresh.iter().map(|b| b.top - self.y).fold(0.0, f32::max);
        self.y += need;
        let empty = self.page.is_empty() && self.height.is_finite();
        for b in fresh {
            let centre =
                (empty && b.centre.is_some()).then_some((self.page.len(), self.deco.len()));
            self.open.push(OpenBox {
                placed: b.clone(),
                top: self.y - b.top,
                continued: false,
                centre,
            });
        }
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
/// The most cells a grid row lays side by side.
const MAX_COLUMNS: usize = 64;
/// A ruby note's size per base size, and the line room it adds, in ems.
const RUBY_SCALE: f32 = 0.5;
const RUBY_ROOM: f32 = 0.6;
/// A quote bar's width and air, mirrored right to left.
const BAR_ROOM: f32 = 3.0;
/// The most lines `widows` and `orphans` keep together.
const MAX_KEPT_LINES: u8 = 8;
/// Pictures set in one block's lines, at most.
const MAX_INLINES: usize = 256;
/// Grid blocks measured for content widths; the rest share their sizes.
const MAX_MEASURED: usize = 512;
/// A grid item's most width is measured in this many rooms.
const MEASURE_ROOM: f32 = 8.0;
/// Lines a wrapping flex row makes, at most.
const MAX_FLEX_LINES: usize = 32;
/// A designed page is laid as if this many book ems wide, as a phone's.
const DESIGN_EMS: f32 = 26.0;
/// The most designed type grows past the reader's size.
const MAX_DESIGN_SCALE: f32 = 1.6;
/// Space between a float and the text past it, in ems.
const FLOAT_GAP: f32 = 0.4;
/// Small capitals' size against capitals.
const SMALL_CAPS: f32 = 0.8;
/// Raised and lowered text's size in reading type.
const SCRIPT_SCALE: f32 = 0.75;
/// How much lighter normal words in bold text are: bold to regular.
const LIGHTER: f32 = 300.0;

fn gap(block: &Set, typo: &Typography) -> f32 {
    if let Some(before) = block.before {
        return before.max(0.0);
    }
    let ems = if block.tight {
        TIGHT_GAP
    } else {
        typo.block_gap
    };
    ems * typo.font_size
}

/// Fills pages unit by unit; headings never end one, no lone lines.
fn paginate(blocks: &[Set], height: f32, typo: &Typography) -> (Vec<Vec<Piece>>, Vec<Vec<Deco>>) {
    let mut c = Cursor {
        height,
        ..Cursor::default()
    };
    let row = |i: usize| blocks.get(i).and_then(|b| b.cell).map(|c| c.key);
    let float = |i: usize| blocks.get(i).and_then(|b| b.float).map(|f| f.key);
    let mut i = 0;
    while i < blocks.len() && c.pages.len() < MAX_PAGES {
        if let (None, Some(key)) = (row(i), float(i)) {
            let end = (i..blocks.len())
                .find(|&j| float(j) != Some(key))
                .unwrap_or(blocks.len())
                .max(i + 1);
            place_float(&mut c, blocks, i..end, (height, typo));
            i = end;
            continue;
        }
        let Some(key) = row(i) else {
            place_block(&mut c, blocks, i, (height, typo));
            i += 1;
            continue;
        };
        let end = (i..blocks.len())
            .find(|&j| row(j) != Some(key))
            .unwrap_or(blocks.len());
        // A row taller than a page is read down the page instead.
        if !place_row(&mut c, blocks, i..end, (height, typo)) {
            for k in i..end {
                place_block(&mut c, blocks, k, (height, typo));
            }
        }
        i = end;
    }
    c.settle();
    c.close(0);
    c.break_page();
    if c.pages.is_empty() {
        c.pages.push(Vec::new());
        c.decos.push(Vec::new());
    }
    (c.pages, c.decos)
}

/// Places a float; what follows starts at its top, unless it ran over.
fn place_float(c: &mut Cursor, blocks: &[Set], float: Range<usize>, room: (f32, &Typography)) {
    c.settle();
    let (pages, top) = (c.pages.len(), c.y);
    for k in float.clone() {
        place_block(c, blocks, k, room);
    }
    let depth = blocks
        .get(float.start)
        .and_then(|b| b.float)
        .map_or(0, |f| f.depth);
    c.close(depth.min(c.open.len()));
    c.float_bottom = c.y;
    if c.pages.len() == pages {
        c.y = top;
    } else if c.page.first().is_some_and(|p| p.block == float.start) {
        // The whole float went overleaf: text starts beside it there.
        c.y = 0.0;
    }
}

/// Places block `i`, split between pages where it must be.
fn place_block(c: &mut Cursor, blocks: &[Set], i: usize, (height, typo): (f32, &Typography)) {
    let Some(block) = blocks.get(i) else {
        return;
    };
    let total = block.units.len();
    if total == 0 {
        return;
    }
    c.settle();
    c.close(c.common(&block.boxes));
    if block.break_before {
        c.break_page();
    }
    if block.clear {
        c.y = c.y.max(c.float_bottom);
    }
    if !c.page.is_empty() {
        c.y += gap(block, typo);
    } else if let (true, Some(before)) = (block.break_before, block.before) {
        // A designed chapter's opening keeps its space at the top.
        c.y += before.clamp(0.0, height / 2.0);
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
    if !c.page.is_empty() {
        // An unbroken box or block that fits a page starts one.
        let tall = whole_height(c, blocks, i, typo).filter(|&h| h <= height);
        if tall.is_some_and(|h| c.y + h > height) {
            c.break_page();
        }
    }
    let mut start = 0;
    while start < total {
        let fit = fitting_units(block, start, height - c.y);
        let mut fit = if block.lines {
            let kept = (block.orphans, block.widows);
            balance(fit, (start, total), c.page.is_empty(), kept)
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
        if start == 0 {
            c.open(&block.boxes);
        }
        c.place(block, i, start..start + fit);
        start += fit;
        if start < total {
            c.break_page();
        }
    }
}

/// Places grid row `row` whole, cells side by side; false if too tall.
fn place_row(
    c: &mut Cursor,
    blocks: &[Set],
    row: Range<usize>,
    (height, typo): (f32, &Typography),
) -> bool {
    let Some((first, cell)) = blocks.get(row.start).and_then(|b| Some((b, b.cell?))) else {
        return false;
    };
    let members = blocks.get(row.clone()).unwrap_or_default();
    let columns = members
        .iter()
        .filter_map(|b| b.cell)
        .map(|c| c.column + 1)
        .max();
    let columns = columns.unwrap_or(1).clamp(1, MAX_COLUMNS);
    let column = |b: &Set| b.cell.map_or(0, |c| c.column).min(columns - 1);
    // Each block's top in its column, then each column's height.
    let mut heights = vec![0.0; columns];
    let mut last: Vec<Option<&Set>> = vec![None; columns];
    let mut tops = Vec::with_capacity(members.len());
    // A float in a column: its key and top; then where floats end.
    let mut floats: Vec<Option<(usize, f32)>> = vec![None; columns];
    let mut float_ends = vec![0.0f32; columns];
    for b in members {
        let col = column(b);
        let (Some(h), Some(end)) = (heights.get_mut(col), last.get_mut(col)) else {
            continue;
        };
        let (Some(float), Some(float_end)) = (floats.get_mut(col), float_ends.get_mut(col)) else {
            continue;
        };
        if let Some((key, start)) = *float
            && b.float.map(|f| f.key) != Some(key)
        {
            // The float ends: what follows starts at its top, beside it.
            (*float_end, *h, *float) = (float_end.max(*h), start, None);
        }
        if b.clear {
            *h = h.max(*float_end);
        }
        if let (Some(f), None) = (b.float, *float) {
            *float = Some((f.key, *h));
        }
        let gap = match end {
            Some(_) => gap(b, typo),
            None => b.before.unwrap_or(0.0).max(0.0),
        };
        tops.push(*h + gap);
        *h += gap + span_height(b, 0..b.units.len());
        *end = Some(b);
    }
    // A column ends below its floats.
    for (h, end) in heights.iter_mut().zip(&float_ends) {
        *h = h.max(*end);
    }
    // A cell's last block closes the cell's boxes under it.
    for (h, b) in heights.iter_mut().zip(&last) {
        let inner = b.map_or(&[][..], |b| b.boxes.get(cell.depth..).unwrap_or_default());
        *h += inner.iter().map(|p| p.bottom).fold(0.0, f32::max);
    }
    // A cell spanning down leaves this row's height to the rest.
    let down = |col: usize| {
        let b = last.get(col).copied().flatten();
        b.and_then(|b| b.cell).map_or(1, |c| c.down)
    };
    let (mut tall, mut need) = (0.0f32, 0.0f32);
    for (col, h) in heights.iter().enumerate() {
        need = need.max(*h);
        if down(col) <= 1 {
            tall = tall.max(*h);
        }
    }
    if need > height {
        return false;
    }
    if c.hangs.iter().any(|h| h.grid != cell.grid) {
        c.settle();
    }
    let outer = first.boxes.get(..cell.depth).unwrap_or(&first.boxes);
    c.close(c.common(outer));
    if first.break_before {
        c.break_page();
    }
    if first.clear {
        c.y = c.y.max(c.float_bottom);
    }
    if !c.page.is_empty() {
        c.y += cell.before.max(0.0);
        if c.y + need > height {
            c.break_page();
        }
    }
    c.open(outer);
    let top = c.y;
    let mut placed = Vec::with_capacity(members.len());
    let mut hung: Vec<Vec<usize>> = vec![Vec::new(); columns];
    for ((k, b), at) in row.clone().zip(members).zip(&tops) {
        let col = column(b);
        let room = tall - heights.get(col).copied().unwrap_or(0.0);
        let shift = match b.cell.map(|c| c.valign) {
            _ if down(col) > 1 => 0.0,
            Some(VAlign::Top) | None => 0.0,
            Some(VAlign::Middle) => room / 2.0,
            Some(VAlign::Bottom) => room,
        };
        let y = top + shift + at;
        let unit = b.units.first().map_or(0.0, |u| u.0);
        if let Some(pieces) = hung.get_mut(col).filter(|_| down(col) > 1) {
            pieces.push(c.page.len());
        }
        c.page.push(Piece {
            block: k,
            units: 0..b.units.len(),
            dy: y - unit,
        });
        placed.push((col, b, y));
    }
    let mut decos = vec![0..0; columns];
    for col in 0..columns {
        let stack = placed.iter().filter(|(c, ..)| *c == col);
        let from = c.deco.len();
        cell_decos(stack.map(|(_, b, y)| (*b, *y)), cell.depth, &mut c.deco);
        if let Some(d) = decos.get_mut(col) {
            *d = from..c.deco.len();
        }
    }
    c.y = top + tall;
    c.row_done(cell.grid);
    for (col, (pieces, decos)) in hung.into_iter().zip(decos).enumerate() {
        if pieces.is_empty() {
            continue;
        }
        let valign = last.get(col).copied().flatten().and_then(|b| b.cell);
        c.hangs.push(Hang {
            grid: cell.grid,
            rows: down(col) - 1,
            bottom: top + heights.get(col).copied().unwrap_or(0.0),
            valign: valign.map_or(VAlign::Top, |c| c.valign),
            pieces,
            decos,
        });
    }
    true
}

/// The borders of a cell's own boxes, its blocks at their tops.
fn cell_decos<'a>(stack: impl Iterator<Item = (&'a Set, f32)>, depth: usize, out: &mut Vec<Deco>) {
    let close = |open: &mut Vec<OpenBox>, keep: usize, bottom: f32, out: &mut Vec<Deco>| {
        while open.len() > keep {
            if let Some(o) = open.pop() {
                out.extend(o.deco(bottom + o.placed.bottom, true));
            }
        }
    };
    let mut open: Vec<OpenBox> = Vec::new();
    let mut bottom = 0.0;
    for (b, y) in stack {
        let inner = b.boxes.get(depth..).unwrap_or_default();
        let same = |(o, p): &(&OpenBox, &Placed)| o.placed.key == p.key;
        let keep = open.iter().zip(inner).take_while(same).count();
        close(&mut open, keep, bottom, out);
        for p in inner.iter().skip(keep) {
            open.push(OpenBox {
                placed: p.clone(),
                top: y - p.top,
                continued: false,
                centre: None,
            });
        }
        bottom = y + span_height(b, 0..b.units.len());
    }
    close(&mut open, 0, bottom, out);
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

/// Trims a fit to keep lines together at a split; zero moves the block on.
fn balance(
    fit: usize,
    (start, total): (usize, usize),
    page_empty: bool,
    (orphans, widows): (usize, usize),
) -> usize {
    let rest = total - start;
    if fit >= rest {
        return rest;
    }
    // Too few lines atop the next page: take more along.
    let trimmed = if rest - fit < widows {
        rest.saturating_sub(widows)
    } else {
        fit
    };
    if trimmed >= orphans.max(1) {
        return trimmed;
    }
    // Too few at a page's foot, or no good split: move on, unless atop one.
    if page_empty { fit } else { 0 }
}

/// Lines a split keeps together; the book's count when designed.
fn lines_kept(count: u8, designed: bool) -> usize {
    match count {
        0 => 2,
        _ if !designed => 2,
        n => usize::from(n.min(MAX_KEPT_LINES)),
    }
}

/// The height block `i` needs on one page: its own, or its unbroken box's.
fn whole_height(c: &Cursor, blocks: &[Set], i: usize, typo: &Typography) -> Option<f32> {
    let block = blocks.get(i)?;
    let depth = c.open.len();
    let boxed = block.boxes.get(depth..).unwrap_or_default();
    let Some((at, outer)) = boxed.iter().enumerate().find(|(_, b)| b.whole) else {
        return block
            .whole
            .then(|| span_height(block, 0..block.units.len()));
    };
    let level = depth + at;
    let inside = |b: &Set| b.boxes.get(level).is_some_and(|p| p.key == outer.key);
    let mut total = outer.top + outer.bottom;
    let mut seen: Vec<usize> = Vec::new();
    for (k, b) in blocks
        .iter()
        .enumerate()
        .skip(i)
        .take_while(|(_, b)| inside(b))
    {
        if k > i {
            total += gap(b, typo);
        }
        total += span_height(b, 0..b.units.len());
        for nested in b.boxes.iter().skip(level + 1) {
            if !seen.contains(&nested.key) {
                seen.push(nested.key);
                total += nested.top + nested.bottom;
            }
        }
    }
    Some(total)
}

/// A line picture's place as parley aligns boxes; `size` is the text's.
fn rise(rise: Rise, size: f32) -> VerticalAlign {
    match rise {
        Rise::Baseline => VerticalAlign::BASELINE,
        Rise::Middle => VerticalAlign::MIDDLE,
        Rise::TextTop => VerticalAlign::TEXT_TOP,
        Rise::TextBottom => VerticalAlign::TEXT_BOTTOM,
        Rise::Top => VerticalAlign::TOP,
        Rise::Bottom => VerticalAlign::BOTTOM,
        Rise::Sub => VerticalAlign::SUB,
        Rise::Super => VerticalAlign::SUPER,
        Rise::By(em) if em.is_finite() => {
            VerticalAlign::length((em * size).clamp(-size * 8.0, size * 8.0))
        }
        Rise::By(_) => VerticalAlign::BASELINE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{Columns, Flex, Inline, Row};
    use crate::picture::tests::png;
    use parley::PositionedLayoutItem;

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
        Block::new(Content::Image {
            picture,
            cover,
            width: None,
        })
    }

    fn flow(blocks: &[Block]) -> Flow {
        let blocks = Arc::new(blocks.to_vec());
        Flow::new(blocks, &Typography::default(), Fonts::bundled())
    }

    fn room() -> f32 {
        PAGE_HEIGHT - 2.0 * PAD
    }

    /// `block` in a designed chapter, in `boxes`, at `cell` if any.
    fn designed(mut block: Block, boxes: &[Arc<Boxed>], cell: Option<Cell>) -> Block {
        let look = Look {
            designed: true,
            ..Look::default()
        };
        block.frame.look = Some(Arc::new(look));
        block.frame.boxes = boxes.to_vec();
        block.frame.cell = cell;
        block
    }

    /// The body em designed text in `f` is set at.
    fn design_em(f: &Flow) -> f32 {
        let em = Typography::default().font_size;
        (f.width / DESIGN_EMS).clamp(em, em * MAX_DESIGN_SCALE)
    }

    /// `block` floated at the left, or right if `right`.
    fn floated(block: Block, right: bool) -> Block {
        let mut block = designed(block, &[], None);
        block.frame.float = Some(Arc::new(Floated { right, depth: 0 }));
        block
    }

    /// Each line's left edge in block `block`'s text.
    fn line_lefts(f: &Flow, block: usize) -> Vec<f32> {
        match &*f.laid(block) {
            Laid::Text { para, .. } => {
                let lines = para.layout.lines();
                lines.map(|l| l.metrics().inline_min_coord).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Where block `block`'s first piece starts and ends on page `page`.
    fn span(f: &Flow, page: usize, block: usize) -> (f32, f32) {
        let piece = f
            .pages
            .get(page)
            .and_then(|p| p.iter().find(|p| p.block == block));
        let set = f.blocks.get(block);
        let (Some(piece), Some(set)) = (piece, set) else {
            return (f32::NAN, f32::NAN);
        };
        let top = set.units.get(piece.units.start).map_or(0.0, |u| u.0) + piece.dy;
        (top, top + span_height(set, piece.units.clone()))
    }

    #[test]
    fn a_book_box_centres_its_width_and_draws_its_border() {
        let b = Arc::new(Boxed {
            width: Some(Length::Em(10.0)),
            border: [0.2; 4],
            edges: [Edge::Double; 4],
            ..Boxed::default()
        });
        let boxed = || designed(para(3), &[Arc::clone(&b)], None);
        let f = flow(&[boxed(), boxed(), designed(para(3), &[], None)]);
        let em = design_em(&f);
        let set = f.blocks.first();
        let left = (f.width - 10.4 * em) / 2.0 + 0.2 * em;
        assert!(set.is_some_and(|s| (s.x - left).abs() < 0.5 && (s.width - 10.0 * em).abs() < 0.5));
        let decos = f.decos.first().map_or(&[][..], Vec::as_slice);
        assert_eq!(decos.len(), 1);
        let (top, _) = span(&f, 0, 0);
        let (_, bottom) = span(&f, 0, 1);
        let (after, _) = span(&f, 0, 2);
        let deco = decos.first().map_or([0.0; 4], |d| d.rect);
        assert!(deco[1] <= top && deco[1] + deco[3] >= bottom, "{deco:?}");
        assert!(deco[1] + deco[3] <= after, "{deco:?} {after}");
    }

    #[test]
    fn prose_draws_the_boxes_the_book_paints_at_the_readers_size() {
        let framed = Arc::new(Boxed {
            border: [0.1; 4],
            edges: [Edge::Solid; 4],
            padding: [0.0, 1.0, 0.0, 1.0],
            ..Boxed::default()
        });
        // Margins alone don't show, so prose keeps its column.
        let moved = Arc::new(Boxed {
            margin: [Some(Length::Em(6.0)), Some(Length::Em(6.0))],
            ..Boxed::default()
        });
        let prose = |boxes: &[Arc<Boxed>]| {
            let mut b = para(3);
            b.frame.look = Some(Arc::new(Look::default()));
            b.frame.boxes = boxes.to_vec();
            b
        };
        let f = flow(&[prose(&[Arc::clone(&moved), framed]), prose(&[moved])]);
        let em = Typography::default().font_size;
        let set = |i: usize| f.blocks.get(i).map(|s| (s.x, s.width, s.boxes.len()));
        let inset = 1.1 * em;
        let framed_at = set(0).unwrap_or_default();
        assert!((framed_at.0 - inset).abs() < 0.5, "{framed_at:?}");
        assert!(
            (framed_at.1 - (f.width - 2.0 * inset)).abs() < 0.5,
            "{framed_at:?}"
        );
        assert_eq!(framed_at.2, 1);
        assert_eq!(set(1), Some((0.0, f.width, 0)));
        let decos = f.decos.first().map_or(0, Vec::len);
        assert_eq!(decos, 1);
        // Read as the reader's own, nothing of the book's shows.
        let off = Flow::new(
            Arc::new(vec![prose(&[Arc::new(Boxed {
                border: [0.1; 4],
                edges: [Edge::Solid; 4],
                ..Boxed::default()
            })])]),
            &Typography {
                design: Design::Off,
                ..Typography::default()
            },
            Fonts::bundled(),
        );
        assert_eq!(off.decos.first().map_or(0, Vec::len), 0);
    }

    #[test]
    fn a_full_height_box_centres_its_content_down_the_page() {
        // Held a page tall as well, it still centres.
        let tall = Arc::new(Boxed {
            centre: Some(0.94),
            height: Some(80.0),
            ..Boxed::default()
        });
        let flex = Arc::new(Columns {
            flex: Some(Flex::default()),
            ..Columns::default()
        });
        let row = row_of(&flex, vec![None], vec![0]);
        let cell = |row: &Arc<GridRow>| {
            let mut c = at(row, 0);
            if let Some(c) = c.as_mut() {
                c.depth = 1;
            }
            c
        };
        for (name, cell) in [("plain", None), ("cell", cell(&row))] {
            let blocks = [
                designed(para(3), &[Arc::clone(&tall)], cell.clone()),
                designed(para(3), &[Arc::clone(&tall)], cell),
            ];
            let f = flow(&blocks);
            let (top, _) = span(&f, 0, 0);
            assert!(top > 300.0, "{name}: {top}");
        }
    }

    #[test]
    fn a_grid_rows_cells_sit_side_by_side() {
        let widths = [10.0, 2.0, 10.0].map(|w| Some(Length::Em(w))).to_vec();
        let row = Arc::new(GridRow {
            widths,
            valign: vec![VAlign::Top; 3],
            ..GridRow::default()
        });
        let cell = |column| {
            let row = Arc::clone(&row);
            Some(Cell {
                row,
                column,
                depth: 0,
            })
        };
        let f = flow(&[
            designed(para(5), &[], cell(0)),
            designed(para(5), &[], cell(2)),
            designed(para(5), &[], cell(2)),
            designed(para(3), &[], None),
        ]);
        let x = |i: usize| f.blocks.get(i).map_or((0.0, 0.0), |s| (s.x, s.width));
        assert!(
            x(1).0 >= x(0).0 + x(0).1 && x(1) == x(2),
            "{:?} {:?}",
            x(0),
            x(1)
        );
        let (a, b, c, d) = (
            span(&f, 0, 0),
            span(&f, 0, 1),
            span(&f, 0, 2),
            span(&f, 0, 3),
        );
        assert!((a.0 - b.0).abs() < 0.01, "{a:?} {b:?}");
        assert!(c.0 >= b.1 && d.0 >= c.1.max(a.1), "{c:?} {d:?}");
    }

    /// A grid row sharing `columns`, with cells' widths, places and growth.
    fn row_of(
        columns: &Arc<Columns>,
        widths: Vec<Option<Length>>,
        starts: Vec<u16>,
    ) -> Arc<GridRow> {
        let n = widths.len();
        let spans = (0..n)
            .map(|i| {
                starts
                    .get(i + 1)
                    .map_or(1, |s| s - starts.get(i).copied().unwrap_or(0))
            })
            .collect();
        Arc::new(GridRow {
            widths,
            valign: vec![VAlign::Top; n],
            starts,
            spans,
            columns: Arc::clone(columns),
            ..GridRow::default()
        })
    }

    fn at(row: &Arc<GridRow>, column: u16) -> Option<Cell> {
        let row = Arc::clone(row);
        Some(Cell {
            row,
            column,
            depth: 0,
        })
    }

    fn words(text: &str, cell: Option<Cell>) -> Block {
        designed(Block::text(Role::Paragraph, text), &[], cell)
    }

    /// Block `i`'s left edge and width.
    fn across(f: &Flow, i: usize) -> (f32, f32) {
        f.blocks
            .get(i)
            .map_or((f32::NAN, f32::NAN), |s| (s.x, s.width))
    }

    #[test]
    fn table_columns_take_their_contents_width() {
        let columns = Arc::new(Columns::default());
        let row = row_of(&columns, vec![None, None], vec![0, 1]);
        let f = flow(&[
            words("ab", at(&row, 0)),
            designed(para(60), &[], at(&row, 1)),
        ]);
        let ((x0, w0), (x1, w1)) = (across(&f, 0), across(&f, 1));
        assert!(w0 < w1 && w0 < f.width / 4.0, "{w0} {w1}");
        assert!(x1 >= x0 + w0 - 0.01, "{x0} {w0} {x1}");
    }

    #[test]
    fn a_cells_boxes_stay_inside_it() {
        let columns = Arc::new(Columns::default());
        let fixed = Some(Length::Em(5.0));
        let row = row_of(&columns, vec![fixed, fixed], vec![0, 1]);
        let wide = Arc::new(Boxed {
            margin: [Some(Length::Em(-2.0)); 2],
            ..Boxed::default()
        });
        let f = flow(&[
            designed(para(9), &[wide], at(&row, 0)),
            words("next", at(&row, 1)),
        ]);
        let ((x0, w0), (x1, _)) = (across(&f, 0), across(&f, 1));
        assert!(x0 + w0 <= x1 + 0.01, "{x0} {w0} {x1}");
    }

    #[test]
    fn a_cell_spanning_rows_down_sits_across_them() {
        let columns = Arc::new(Columns::default());
        let fixed = Some(Length::Em(8.0));
        let row = |down: Vec<u16>, starts: Vec<u16>| {
            Arc::new(GridRow {
                widths: vec![fixed; starts.len()],
                valign: vec![VAlign::Middle; starts.len()],
                spans: vec![1; starts.len()],
                starts,
                down,
                columns: Arc::clone(&columns),
                ..GridRow::default()
            })
        };
        let (first, second, third) = (
            row(vec![3, 1], vec![0, 1]),
            row(vec![], vec![1]),
            row(vec![], vec![1]),
        );
        let f = flow(&[
            words("tall", at(&first, 0)),
            words("one", at(&first, 1)),
            words("two", at(&second, 0)),
            words("three", at(&third, 0)),
            words("after", None),
        ]);
        let [tall, one, two, three, after] = [0, 1, 2, 3, 4].map(|i| span(&f, 0, i));
        let line = one.1 - one.0;
        // Its own row is no taller; it sits mid-way down all three.
        assert!((two.0 - one.1).abs() < line / 2.0, "{one:?} {two:?}");
        let (middle, rows) = ((tall.0 + tall.1) / 2.0, (one.0 + three.1) / 2.0);
        assert!((middle - rows).abs() < 1.0, "{tall:?} {one:?} {three:?}");
        assert!(after.0 >= three.1 - 0.01, "{three:?} {after:?}");
        // Taller than its rows, the last of them grows to hold it.
        let first = row(vec![2, 1], vec![0, 1]);
        let second = row(vec![], vec![1]);
        let f = flow(&[
            designed(para(60), &[], at(&first, 0)),
            words("one", at(&first, 1)),
            words("two", at(&second, 0)),
            words("after", None),
        ]);
        let [tall, one, two, after] = [0, 1, 2, 3].map(|i| span(&f, 0, i));
        assert!(
            two.0 < tall.1 && (two.0 - one.1).abs() < line / 2.0,
            "{tall:?} {two:?}"
        );
        assert!(after.0 >= tall.1 - 0.01, "{tall:?} {after:?}");
    }

    #[test]
    fn a_cell_spans_the_columns_under_it() {
        let columns = Arc::new(Columns::default());
        let wide = Arc::new(GridRow {
            widths: vec![None],
            valign: vec![VAlign::Top],
            starts: vec![0],
            spans: vec![2],
            columns: Arc::clone(&columns),
            ..GridRow::default()
        });
        let row = row_of(&columns, vec![None, None], vec![0, 1]);
        let f = flow(&[
            designed(para(30), &[], at(&wide, 0)),
            words("one two three", at(&row, 0)),
            words("four five six", at(&row, 1)),
        ]);
        let ((a, aw), (b, _), (c, cw)) = (across(&f, 0), across(&f, 1), across(&f, 2));
        assert!((a - b).abs() < 1.0, "{a} {b}");
        assert!((a + aw - c - cw).abs() < 1.0, "{a} {aw} {c} {cw}");
    }

    #[test]
    fn flex_rows_justify_grow_and_wrap() {
        let flex = |justify, wrap| {
            let flex = Some(Flex {
                justify,
                gap: 0.0,
                wrap,
            });
            Arc::new(Columns {
                flex,
                ..Columns::default()
            })
        };
        let plain = || designed(para(3), &[], None);
        let centred = flex(Justify::Center, false);
        let row = row_of(&centred, vec![None, None], vec![0, 1]);
        let f = flow(&[words("ab", at(&row, 0)), words("cd", at(&row, 1)), plain()]);
        let (left, room) = across(&f, 2);
        assert!(across(&f, 0).0 > left + room / 4.0, "{:?}", across(&f, 0));

        let start = flex(Justify::Start, false);
        let grown = Arc::new(GridRow {
            grow: vec![0.0, 1.0],
            ..(*row_of(&start, vec![None, None], vec![0, 1])).clone()
        });
        let f = flow(&[
            words("ab", at(&grown, 0)),
            words("cd", at(&grown, 1)),
            plain(),
        ]);
        let ((x0, _), (x1, w1)) = (across(&f, 0), across(&f, 1));
        assert!((x0 - left).abs() < 1.0 && (x1 + w1 - left - room).abs() < 1.0);

        let wrapping = flex(Justify::Start, true);
        let row = row_of(&wrapping, vec![Some(Length::Em(12.0)); 4], vec![0, 1, 2, 3]);
        let items: Vec<Block> = (0..4).map(|i| words("w", at(&row, i))).collect();
        let f = flow(&items);
        let (top0, top2) = (span(&f, 0, 0).0, span(&f, 0, 2).0);
        assert!(top2 > top0, "{top0} {top2}");
        assert!((across(&f, 2).0 - across(&f, 0).0).abs() < 1.0);
    }

    #[test]
    fn small_pictures_sit_in_their_line() {
        let mut block = Block::text(Role::Paragraph, "see the sign here");
        if let Content::Text(t) = &mut block.content {
            let picture = picture(8, 8);
            t.inlines.push(Inline {
                at: 8,
                picture,
                width: 1.0,
                height: 1.0,
                rise: Rise::Baseline,
            });
        }
        let f = flow(&[block]);
        let Laid::Text { para, .. } = &*f.laid(0) else {
            return assert!(f.blocks.len() == 1, "text");
        };
        assert_eq!(para.pictures.len(), 1);
        let boxes = para
            .layout
            .lines()
            .flat_map(|l| l.items().collect::<Vec<_>>());
        let boxes = boxes.filter(|i| matches!(i, PositionedLayoutItem::InlineBox(_)));
        assert_eq!(boxes.count(), 1);
    }

    #[test]
    fn round_corners_and_turns_reach_the_page() {
        let b = Arc::new(Boxed {
            border: [0.1; 4],
            edges: [Edge::Solid; 4],
            radius: Some(Length::Em(1.0)),
            ..Boxed::default()
        });
        let mut block = designed(para(5), &[b], None);
        block.frame.rotate = 90.0;
        let f = flow(&[block]);
        assert_eq!(f.blocks.first().map(|s| s.rotate), Some(90.0));
        let radius = f
            .decos
            .first()
            .and_then(|d| d.first())
            .map_or(0.0, |d| d.radius);
        assert!(radius > 1.0, "{radius}");
    }

    #[test]
    fn a_box_cut_by_a_page_stays_open_at_the_cut() {
        let b = Arc::new(Boxed {
            border: [0.1; 4],
            edges: [Edge::Solid; 4],
            ..Boxed::default()
        });
        let blocks: Vec<Block> = (0..30)
            .map(|_| designed(para(60), &[Arc::clone(&b)], None))
            .collect();
        let f = flow(&blocks);
        assert!(f.page_count() > 1);
        let edges = |page: usize| f.decos.get(page).and_then(|d| d.first()).map(|d| d.edges);
        let last = f.page_count() - 1;
        assert!(edges(0).is_some_and(|e| e[0] == Edge::Solid && e[2] == Edge::None));
        assert!(edges(last).is_some_and(|e| e[0] == Edge::None && e[2] == Edge::Solid));
    }

    #[test]
    fn prose_takes_the_books_type_moved_by_the_readers() {
        let typed = |role, size, weight| {
            let mut b = Block::text(role, "Short words");
            b.frame.look = Some(Arc::new(Look {
                size,
                weight,
                ..Look::default()
            }));
            b
        };
        let blocks = Arc::new(vec![
            typed(Role::Heading(2), 1.5, 700.0),
            typed(Role::Paragraph, 1.0, 400.0),
            typed(Role::Paragraph, 0.8, 400.0),
            designed(Block::text(Role::Paragraph, "Short words"), &[], None),
        ]);
        let laid = |typo: Typography| Flow::new(Arc::clone(&blocks), &typo, Fonts::bundled());
        let tall = |f: &Flow, i: usize| {
            let s = f.blocks.get(i);
            s.map_or(0.0, |s| span_height(s, 0..s.units.len()))
        };
        let base = laid(Typography::default());
        let big = laid(Typography {
            font_size: 34.0,
            ..Typography::default()
        });
        let off = laid(Typography {
            design: Design::Off,
            ..Typography::default()
        });
        // The book's steps hold: the heading is 1.5 times, the note 0.8.
        let near = |a: f32, b: f32| (a - b).abs() <= b * 0.06;
        let (heading, body, note) = (tall(&base, 0), tall(&base, 1), tall(&base, 2));
        assert!(
            near(heading / body, 1.5 * HEADING_LINE_HEIGHT / READER.1),
            "{heading} {body}"
        );
        assert!(near(note / body, 0.8), "{note} {body}");
        // Twice the reader's size doubles every size, designed type too.
        for i in 0..4 {
            assert!(near(tall(&big, i), 2.0 * tall(&base, i)), "{i}");
        }
        // The book's margins of none hold; Off keeps the reader's gap.
        let gap = |f: &Flow| span(f, 0, 2).0 - span(f, 0, 1).1;
        assert!(gap(&base).abs() < 0.5, "{}", gap(&base));
        assert!(near(gap(&off), READER.0), "{}", gap(&off));
        // Off sets every paragraph in the reader's one size.
        assert!(near(tall(&off, 2), tall(&off, 1)));
    }

    #[test]
    fn designed_type_is_set_as_on_a_phone_page() {
        let f = flow(&[designed(para(1), &[], None), para(1)]);
        let em = Typography::default().font_size;
        let design = design_em(&f);
        assert!(design > em && design <= em * MAX_DESIGN_SCALE, "{design}");
        let tall = |i| {
            f.blocks
                .get(i)
                .map_or(0.0, |s| span_height(s, 0..s.units.len()))
        };
        assert!(tall(0) > tall(1) * 1.2, "{} {}", tall(0), tall(1));
    }

    #[test]
    fn a_drop_cap_spans_lines_with_the_text_beside_it() {
        let mut t = Text::new(Role::Paragraph);
        let big = Emphasis {
            size: 300,
            ..Emphasis::default()
        };
        t.push("A", big);
        let rest = "lice was beginning to get very tired of sitting by her sister ";
        t.push(&rest.repeat(6), Emphasis::default());
        t.initial = 1;
        let mut b = Block::new(Content::Text(t));
        b.frame.look = Some(Arc::new(Look::default()));
        let f = flow(&[b.clone()]);
        let line = match &*f.laid(0) {
            Laid::Text {
                initial: Some(i), ..
            } => i.line,
            _ => 0,
        };
        assert_eq!(line, 2);
        let lefts = line_lefts(&f, 0);
        assert!(lefts.iter().take(2).all(|x| *x > 17.0), "{lefts:?}");
        assert!(lefts.get(2).is_some_and(|x| *x == 0.0), "{lefts:?}");
        // Read as the reader's own, the letter stays in its line.
        b.frame.look = None;
        let plain = flow(&[b]);
        assert!(matches!(&*plain.laid(0), Laid::Text { initial: None, .. }));
    }

    #[test]
    fn marker_pictures_and_first_lines_lay_out() {
        let mut item = para(3);
        item.frame.marker = Some(String::new());
        item.frame.marker_picture = Picture::probe(png(32, 32)).ok().map(Arc::new);
        let f = flow(&[item]);
        let pictures = match &*f.laid(0) {
            Laid::Text {
                marker: Some(m), ..
            } => m.pictures.len(),
            _ => 0,
        };
        assert_eq!(pictures, 1);
        let mut t = Text::new(Role::Paragraph);
        t.push("one ", Emphasis::default());
        let bold = Emphasis {
            bold: true,
            ..Emphasis::default()
        };
        t.push("two three", bold);
        let upper = Emphasis {
            case: Case::Upper,
            ..Emphasis::default()
        };
        let styled = over_first_line(&t, 7, upper);
        let spans: Vec<(Range<usize>, bool, Case)> = (styled.spans.iter())
            .map(|s| (s.range.clone(), s.emphasis.bold, s.emphasis.case))
            .collect();
        let want = [
            (0..4, false, Case::Upper),
            (4..7, true, Case::Upper),
            (7..13, true, Case::Normal),
        ];
        assert_eq!(spans, want);
        assert_eq!(styled.text, t.text);
    }

    #[test]
    fn text_wraps_past_a_float_then_takes_the_column() {
        let f = flow(&[floated(para(1), false), designed(para(300), &[], None)]);
        let (float, text) = (f.blocks.first(), f.blocks.get(1));
        let fw = float.map_or(f32::NAN, |s| s.width);
        assert!(fw < f.width / 4.0, "{fw}");
        assert!(float.is_some_and(|s| s.float.is_some()));
        assert!(text.is_some_and(|s| s.beside.is_some()));
        let ((a, a_end), (b, _)) = (span(&f, 0, 0), span(&f, 0, 1));
        assert!((a - b).abs() < 0.5, "{a} {b}");
        let lefts = line_lefts(&f, 1);
        assert!(lefts.first().is_some_and(|x| *x >= fw), "{lefts:?}");
        assert!(lefts.last().is_some_and(|x| *x == 0.0), "{lefts:?}");
        // Lines beside the float are the float's height, no more.
        let beside = lefts.iter().take_while(|x| **x > 0.0).count();
        let line = text.map_or(0.0, |s| span_height(s, 0..1));
        assert!(beside as f32 * line <= a_end - a + line + 0.5, "{beside}");
    }

    #[test]
    fn a_right_float_keeps_text_to_its_left() {
        let f = flow(&[floated(para(1), true), designed(para(80), &[], None)]);
        let float = f.blocks.first().map_or((0.0, 0.0), |s| (s.x, s.width));
        assert!((float.0 + float.1 - f.width).abs() < 0.5, "{float:?}");
        assert!(line_lefts(&f, 1).iter().all(|x| *x == 0.0));
        let widths: Vec<f32> = match &*f.laid(1) {
            Laid::Text { para, .. } => para.layout.lines().map(|l| l.metrics().advance).collect(),
            _ => Vec::new(),
        };
        assert!(
            widths.first().is_some_and(|w| *w <= float.0 + 0.5),
            "{widths:?}"
        );
    }

    #[test]
    fn a_picture_after_a_float_starts_below_it() {
        let mut picture = image(400, 200, false);
        picture.frame.look = Some(Arc::new(Look {
            designed: true,
            ..Look::default()
        }));
        let f = flow(&[floated(para(1), false), picture]);
        assert!(f.blocks.get(1).is_some_and(|s| s.clear));
        let ((_, float_end), (top, _)) = (span(&f, 0, 0), span(&f, 0, 1));
        assert!(top >= float_end - 0.5, "{float_end} {top}");
    }

    #[test]
    fn a_set_height_holds_its_box_open() {
        let b = Arc::new(Boxed {
            height: Some(10.0),
            fill: Some(0.1),
            ..Boxed::default()
        });
        let f = flow(&[designed(para(1), &[b], None)]);
        let em = design_em(&f);
        let deco = f.decos.first().and_then(|d| d.first());
        assert!(deco.is_some_and(|d| d.rect[3] >= 10.0 * em - 0.5 && d.fill == Some(0.1)));
    }

    #[test]
    fn cases_show_over_the_text_keeping_its_offsets() {
        let mut t = Text::new(Role::Paragraph);
        let small = Emphasis {
            case: Case::Small,
            ..Emphasis::default()
        };
        t.push("Top ", Emphasis::default());
        t.push("Straße ok", small);
        let (shown, runs) = cased(&t);
        assert_eq!(shown, "Top STRAßE OK");
        assert_eq!(shown.len(), t.text.len());
        let runs: Vec<Range<usize>> = runs.into_iter().map(|(r, _)| r).collect();
        assert_eq!(runs, [5..8, 10..11, 12..14]);
    }

    #[test]
    fn all_small_caps_shrink_capitals_too() {
        let mut t = Text::new(Role::Paragraph);
        let all = Emphasis {
            case: Case::AllSmall,
            ..Emphasis::default()
        };
        t.push("NASA flew", all);
        let (shown, runs) = cased(&t);
        assert_eq!(shown, "NASA FLEW");
        let runs: Vec<Range<usize>> = runs.into_iter().map(|(r, _)| r).collect();
        assert_eq!(runs, [0..4, 5..9]);
    }

    #[test]
    fn splits_keep_the_books_widows_and_orphans() {
        let usual = (2, 2);
        // Ten lines, room for nine: two go over, as one would be alone.
        assert_eq!(balance(9, (0, 10), false, usual), 8);
        assert_eq!(balance(9, (0, 10), false, (2, 3)), 7);
        // One line fits at the foot: the paragraph moves on.
        assert_eq!(balance(1, (0, 10), false, usual), 0);
        assert_eq!(balance(2, (0, 10), false, (3, 2)), 0);
        assert_eq!(balance(3, (0, 10), false, (3, 2)), 3);
        // Atop a page there is no better place: it splits anyway.
        assert_eq!(balance(1, (0, 10), true, (3, 3)), 1);
        assert_eq!(lines_kept(0, true), 2);
        assert_eq!(lines_kept(4, false), 2);
        assert_eq!(lines_kept(200, true), usize::from(MAX_KEPT_LINES));
    }

    #[test]
    fn features_later_win_and_read_as_css() {
        use crate::block::Features;
        let on = |t| Features::of(t, true).unwrap_or_default();
        let off = |t| Features::of(t, false).unwrap_or_default();
        let early = on("liga").then(on("onum"));
        let both = early.then(off("liga")).then(on("zero"));
        assert_eq!(both.source(), "\"liga\" 0, \"onum\" 1, \"zero\" 1");
        assert_eq!(Features::of("wxyz", true), None);
        assert!(Features::default().is_empty());
    }

    #[test]
    fn raised_text_sets_smaller_on_its_line() {
        let mut t = Text::new(Role::Paragraph);
        t.push("x", Emphasis::default());
        let sup = Emphasis {
            script: Script::Super,
            underline: true,
            strike: true,
            ..Emphasis::default()
        };
        t.push("2", sup);
        let f = flow(&[Block::new(Content::Text(t))]);
        let laid = f.laid(0);
        let lines = match &*laid {
            Laid::Text { para, .. } => para.layout.lines().collect(),
            _ => Vec::new(),
        };
        let runs: Vec<(f32, bool, bool)> = lines
            .iter()
            .flat_map(|l| l.items().collect::<Vec<_>>())
            .filter_map(|i| match i {
                parley::PositionedLayoutItem::GlyphRun(r) => Some((
                    r.run().font_size(),
                    r.style().underline.is_some(),
                    r.style().strikethrough.is_some(),
                )),
                parley::PositionedLayoutItem::InlineBox(_) => None,
            })
            .collect();
        assert_eq!(runs.len(), 2, "{runs:?}");
        let (Some(x), Some(two)) = (runs.first(), runs.get(1)) else {
            return;
        };
        assert!(two.0 < x.0 && two.1 && two.2 && !x.1, "{runs:?}");
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
        let f = flow(&[Block::new(Content::Table(Table { rows, rule: None }))]);
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
            let table = Block::new(Content::Table(Table { rows, rule: None }));
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

    /// A paragraph of `text` that reads `dir`.
    fn directed(text: &str, dir: Dir) -> Block {
        let mut b = Block::text(Role::Paragraph, text);
        b.frame.dir = dir;
        b
    }

    #[test]
    fn hebrew_or_arabic_reads_right_to_left_unless_told_otherwise() {
        let hebrew = "שלום עולם";
        let f = flow(&[
            directed(hebrew, Dir::Auto),
            directed("word word", Dir::Auto),
            directed(hebrew, Dir::Ltr),
            directed("word word", Dir::Rtl),
        ]);
        let rtl: Vec<bool> = f.blocks.iter().map(|s| s.rtl).collect();
        assert_eq!(rtl, [true, false, false, true]);
        // A short right-to-left line sits at the right of its column.
        let first = |b| line_lefts(&f, b).first().copied().unwrap_or(f32::NAN);
        assert!(
            first(0) > f.width / 2.0 && first(2) < 1.0,
            "{} {}",
            first(0),
            first(2)
        );
    }

    #[test]
    fn right_to_left_quote_bars_move_to_the_right() {
        let quote = |dir| {
            let mut b = directed("word word", dir);
            b.frame.bars = vec![0.0];
            b
        };
        let f = flow(&[quote(Dir::Ltr), quote(Dir::Rtl)]);
        let bar = |i: usize| {
            let set = f.blocks.get(i);
            set.and_then(|s| s.bars.first().copied())
                .unwrap_or(f32::NAN)
        };
        assert!(
            bar(0) < 1.0 && bar(1) > f.width / 2.0,
            "{} {}",
            bar(0),
            bar(1)
        );
    }

    #[test]
    fn ruby_lays_its_note_and_makes_room_above_the_line() {
        let mut noted = Block::text(Role::Paragraph, "word word");
        if let Content::Text(t) = &mut noted.content {
            t.ruby.push(crate::block::Annotation {
                range: 0..4,
                text: "note".into(),
            });
        }
        let f = flow(&[noted, Block::text(Role::Paragraph, "word word")]);
        let tall = |i: usize| {
            let set = f.blocks.get(i);
            set.and_then(|s| s.units.first()).map_or(0.0, |u| u.1 - u.0)
        };
        assert!(tall(0) > tall(1) + 1.0, "{} {}", tall(0), tall(1));
        let note = match &*f.laid(0) {
            Laid::Text { para, .. } => para
                .ruby
                .first()
                .map(|n| (n.range.clone(), n.para.is_some())),
            _ => None,
        };
        assert_eq!(note, Some((0..4, true)));
    }
}
