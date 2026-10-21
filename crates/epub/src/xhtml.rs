//! Chapter XHTML to blocks: html5ever's tokenizer, our element stack, no DOM.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use mareader_reflow::{
    Align, Anchor, Annotation, Backdrop, BackdropSize, Block, Boxed, Case, Cell, Columns, Content,
    Dir, Edge, Emphasis, Features, Finish, Flex, Floated, Frame, Frame9, GradientShape, GridRow,
    Inline, Justify, Keep, Length, Links, Look, Outline, Picture, Rise, Role, Row, Script, Sheet,
    Spot, Table, Text, TextShadow, VAlign,
};

use crate::calc::Vars;
use crate::css::{
    self, BorderStyle, Computed, Ctx, Decl, Display, El, Float, FontFace, Len, Line, MAX_SHEET,
    PictureSize, Placing, PseudoEl, ROOT_PX, Stylesheet, TextAlign,
};
use crate::decor::{self, Span, Tint};
use crate::generated::{self, Content as Generated, Item, ListStyle};
use crate::href;
use crate::math;
use crate::query;
use crate::svg::{self, Capture};

/// Open elements tracked at once; deeper nesting is read as flat.
pub const MAX_DEPTH: usize = 256;
const MAX_COLUMNS: usize = 32;
/// The most rows a one-column table may have and still read as a box.
const MAX_BOX_ROWS: usize = 8;
const MAX_BARS: usize = 8;
/// The longest ruby note kept, in bytes.
const MAX_NOTE: usize = 256;
/// Runs set across vertical lines, per block.
const MAX_COMBINED: usize = 256;
/// Elements that set as one text block, kept whole without a box.
const TEXT_BLOCKS: [&str; 12] = [
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "dt",
    "pre",
    "caption",
    "figcaption",
];
const MAX_CLASSES: usize = 16;
const MAX_ATTRS: usize = 16;
/// Earlier siblings kept per element, for `+` and `~` selectors.
const MAX_SIBLINGS: usize = 64;
/// Counters in scope past this are not made.
const MAX_COUNTERS: usize = 256;
/// What a book face's family is registered as: never a machine's own.
pub(crate) const BOOK_FACE: &str = mareader_reflow::BOOK_FACE;
/// Generic families: the reader's own faces stand in.
const GENERIC: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "math",
    "emoji",
    "fangsong",
];
/// Stylesheets one chapter applies; later ones are ignored.
pub const MAX_SHEETS: usize = 16;
/// Sheets importing sheets, this deep.
const MAX_IMPORT_DEPTH: usize = 4;
/// Indents in body ems, as mareader's stylesheet has them.
const LIST_INSET: f32 = 1.6;
const QUOTE_INSET: f32 = 1.0;
/// Backgrounds fainter than this against the page are left out.
const MIN_FILL: f32 = 0.08;
/// A fixed page naming no viewport: the reflowed page box, in CSS px.
const DEFAULT_SHEET: (f32, f32) = (794.0, 1123.0);
/// The largest viewport side a fixed page may ask for.
const MAX_SHEET_SIDE: f32 = 10_000.0;
/// Rows a cell spans down, at most.
const MAX_SPAN_DOWN: u16 = 64;
/// Pictures set in one block's lines, at most.
const MAX_INLINES: usize = 256;
/// A picture this many text ems tall or wide, at most, sits in a line.
const INLINE_TALL: f32 = 3.0;
const INLINE_WIDE: f32 = 8.0;
/// The most ink a background under text takes, keeping it readable.
const MAX_TEXT_FILL: f32 = 0.16;
const TAB: &str = "    ";
/// Ids and links kept per book; past these, the rest are ignored.
pub const MAX_PLACES: usize = 1 << 20;
pub const MAX_LINKS: usize = 1 << 20;

/// Where a link leads, as its chapter wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Href {
    /// `path` or `path#id` in the archive, both decoded.
    Inside(String),
    /// Anything with a scheme, checked only when followed.
    Outside(String),
}

/// The places a book's ids mark and its links' destinations.
#[derive(Debug, Default)]
pub(crate) struct Refs {
    /// `path` and `path#id` to where each starts.
    pub(crate) places: HashMap<String, Anchor>,
    /// Indexed by the target a link's text carries.
    pub(crate) hrefs: Vec<Href>,
    /// `path#id` of block elements to the blocks they hold.
    pub(crate) extents: HashMap<String, Extent>,
    /// Links marked as leading to a note.
    pub(crate) noterefs: HashSet<u32>,
    /// The book's fonts that designed text uses.
    pub(crate) faces: Vec<FontFace>,
}

/// The blocks an element with an id holds, and whether it is a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Extent {
    pub(crate) blocks: Range<usize>,
    pub(crate) note: bool,
}

impl Refs {
    /// The place `key` marks, else the start of its file.
    pub(crate) fn place(&self, key: &str) -> Option<Anchor> {
        let file = key.split('#').next().unwrap_or_default();
        self.places
            .get(key)
            .or_else(|| self.places.get(file))
            .copied()
    }
}

/// Where a chapter's images and stylesheets come from.
pub(crate) trait Library {
    /// `path` is absolute in the archive; `None` if missing or unusable.
    fn picture(&mut self, path: &str) -> Option<Arc<Picture>>;
    fn stylesheet(&mut self, path: &str) -> Option<Arc<Stylesheet>>;
    /// A file an SVG links, as it is; `None` if missing or too large.
    fn bytes(&mut self, path: &str) -> Option<Arc<Vec<u8>>>;
}

/// The images SVG at `base` links by `hrefs`, read from the book.
pub(crate) fn svg_links(library: &mut dyn Library, base: &str, hrefs: Vec<String>) -> Links {
    let take = hrefs.into_iter().take(svg::MAX_LINKS);
    take.filter_map(|h| {
        let path = href::resolve(base, &h)?;
        Some((h, library.bytes(&path)?))
    })
    .collect()
}

/// What an inline drawing is read into.
enum Caught {
    Svg(Box<Capture>),
    Math(Box<math::Capture>),
}

/// An inline `svg` or `math` being read: its markup and how it is set.
struct Drawing {
    capture: Caught,
    name: String,
    computed: Computed,
    shown: bool,
    alt: Option<String>,
}

/// What open elements pass down to the text inside them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Style {
    code: bool,
    pre: bool,
    align: Align,
    sided: bool,
    /// Not shown at all: the head, scripts, `display: none`.
    hidden: bool,
    /// Inside SVG or MathML: no text, though SVG images still show.
    graphic: bool,
    /// Inside a link, by its index in [`Refs::hrefs`].
    link: Option<u32>,
    /// List items show no marker; inherited, as in CSS.
    unmarked: bool,
    /// Inside `<rt>`: a ruby note, kept apart from the text.
    note: bool,
}

struct Open {
    name: String,
    style: Style,
    computed: Computed,
    /// Its element, and its children so far: the latest, and a count.
    el: El,
    kids: Vec<El>,
    count: usize,
    /// Children by tag, for `:nth-of-type`.
    types: Vec<(String, usize)>,
    /// An ordered list's last item number.
    counter: u32,
    /// Its `path#id`, first block and whether it is a note.
    extent: Option<(String, usize, bool)>,
    /// It opened a box in [`State::boxes`].
    boxed: bool,
    /// Its part in the layout grid being read.
    part: Part,
    /// It is the float in [`State::float`].
    floated: bool,
    /// It looks laid out: a strong sign of a designed chapter.
    signals: bool,
    /// Its `::after` box, shown as it closes.
    after: Option<Box<Decl>>,
}

/// The generated boxes rules give an element.
#[derive(Default)]
struct Pseudos {
    before: Option<Decl>,
    after: Option<Decl>,
    letter: Option<Decl>,
    line: Option<Decl>,
    marker: Option<Decl>,
}

/// What a counter property does to its counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Reset,
    Add,
    Set,
}

/// Whether `d` makes a `::before` or `::after` box.
fn generates(d: &Decl) -> bool {
    matches!(d.content, Some(Generated::Items(_))) && d.display != Some(Display::None)
}

/// A start tag the reader makes for a generated box.
fn synthetic(name: &str) -> Tag {
    Tag {
        kind: TagKind::StartTag,
        name: name.into(),
        self_closing: false,
        attrs: Vec::new(),
        had_duplicate_attributes: false,
    }
}

/// The bytes of a text's first letter, with punctuation before it.
fn first_letter(s: &str) -> usize {
    let mark = |c: char| {
        matches!(c as u32, 0x300..=0x36f | 0x1ab0..=0x1aff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
            || matches!(c as u32, 0x102b..=0x103e | 0x1056..=0x1059 | 0x1062..=0x1064)
    };
    let (mut end, mut letter) = (0, false);
    for (i, c) in s.char_indices() {
        let next = i + c.len_utf8();
        match (letter, c) {
            (false, c) if c.is_alphanumeric() => (end, letter) = (next, true),
            (false, c) if !c.is_whitespace() && !c.is_alphanumeric() => end = next,
            (true, c) if mark(c) => end = next,
            _ => break,
        }
    }
    if letter { end } else { 0 }
}

/// An element's part in a layout grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    None,
    Table,
    Row,
    Cell,
    /// A flex container laying its items in a row, and one of its items.
    FlexBox,
    FlexItem,
    /// A flex container stacking its items, and one laid as a row of one.
    Stack,
    StackItem,
}

/// A book's box open around what is read (`docs/epub-layout.md`).
struct BoxOpen {
    spec: Boxed,
    /// Made at its first block; every block in it shares it.
    arc: Option<Arc<Boxed>>,
    /// The space waiting above its top border.
    gap: Gap,
    /// Its first block in [`State::blocks`].
    first: usize,
    /// Its background's full strength, drawn only if it holds no text.
    contrast: Option<f32>,
    /// Its finish at full strength, likewise.
    finish: Option<Arc<Finish>>,
}

/// The outermost layout table being read; nested ones read flat.
struct GridBuild {
    /// Its place in [`State::stack`].
    depth: usize,
    row: Option<RowBuild>,
    /// What its rows share.
    columns: Arc<Columns>,
    /// A flex row's items' default vertical place.
    flex: Option<VAlign>,
    /// Rows each column is still taken for by a cell spanning down.
    taken: Vec<u16>,
    /// A column stack's item, sure to stay a row of one.
    single: bool,
}

/// A grid row being read; its blocks hold a placeholder till it ends.
struct RowBuild {
    widths: Vec<Option<Length>>,
    valign: Vec<VAlign>,
    before: f32,
    placeholder: Arc<GridRow>,
    /// Its first block in [`State::blocks`].
    first: usize,
    /// The cell being read, and how many boxes hold the grid.
    column: Option<u16>,
    boxes: u8,
    starts: Vec<u16>,
    spans: Vec<u16>,
    grow: Vec<f32>,
    /// Rows each cell spans down.
    down: Vec<u16>,
    /// The column after the last cell.
    next: u16,
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Elements that end the text before them and start anew.
fn is_block(name: &str) -> bool {
    heading(name).is_some()
        || matches!(
            name,
            "p" | "div"
                | "li"
                | "ul"
                | "ol"
                | "dl"
                | "dt"
                | "dd"
                | "blockquote"
                | "pre"
                | "section"
                | "article"
                | "header"
                | "footer"
                | "aside"
                | "nav"
                | "figure"
                | "figcaption"
                | "body"
                | "main"
                | "address"
                | "center"
                | "table"
                | "caption"
                | "thead"
                | "tbody"
                | "tfoot"
                | "tr"
                | "td"
                | "th"
                | "hr"
        )
}

fn heading(name: &str) -> Option<u8> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Elements whose content the tokenizer must read as raw text.
fn is_raw(name: &str) -> bool {
    matches!(
        name,
        "style" | "script" | "title" | "noscript" | "template" | "textarea" | "xmp"
    )
}

/// HTML's collapsible spaces; U+00A0 and friends stay as written.
fn is_html_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

fn attr<'t>(tag: &'t Tag, name: &str) -> Option<&'t str> {
    tag.attrs
        .iter()
        .find(|a| &*a.name.local == name)
        .map(|a| &*a.value)
}

/// Whether attribute `name` lists one of `words`, as `epub:type` does.
fn typed(tag: &Tag, name: &str, words: &[&str]) -> bool {
    let listed = |w: &str| words.iter().any(|x| w.eq_ignore_ascii_case(x));
    attr(tag, name).is_some_and(|v| v.split_whitespace().any(listed))
}

/// An element holding a note, by EPUB or ARIA semantics.
fn is_note(tag: &Tag) -> bool {
    typed(
        tag,
        "epub:type",
        &["footnote", "endnote", "rearnote", "note"],
    ) || typed(tag, "role", &["doc-footnote", "doc-endnote"])
}

/// A link to a note, by EPUB or ARIA semantics.
fn is_noteref(tag: &Tag) -> bool {
    typed(tag, "epub:type", &["noteref"]) || typed(tag, "role", &["doc-noteref"])
}

/// The table being read; only the outermost one builds rows.
struct TableBuild {
    table: Table,
    frame: Frame,
    caption: Text,
    /// Tables inside this one, read as text in its cells.
    nested: usize,
    in_cell: bool,
    in_caption: bool,
    in_head: bool,
}

impl TableBuild {
    fn cell(&mut self) -> Option<&mut Text> {
        if self.in_caption {
            return Some(&mut self.caption);
        }
        if !self.in_cell {
            return None;
        }
        self.table.rows.last_mut()?.cells.last_mut()
    }
}

struct State<'a> {
    library: &'a mut dyn Library,
    refs: &'a mut Refs,
    /// The book's index of this chapter's first block.
    first: usize,
    /// Ids waiting for the next block, which they mark.
    pending: Vec<String>,
    /// The chapter's own path, for resolving its links.
    base: &'a str,
    sheets: Vec<Arc<Stylesheet>>,
    /// An inline `<style>` being read.
    style_text: String,
    /// In `<ruby>`: where the base starts, and where it ended at `<rt>`.
    ruby: Option<(usize, Option<usize>)>,
    /// The ruby note being read.
    note: String,
    /// The chapter is written vertically.
    turned: bool,
    /// A run set across vertical lines: its element's depth and start.
    combining: Option<(usize, usize)>,
    /// Whether that `<style>`'s `media` applies.
    style_applies: bool,
    /// `html`, styled before its head's sheets; its custom values wait.
    root: Option<El>,
    blocks: Vec<Block>,
    current: Option<Block>,
    table: Option<TableBuild>,
    /// By each `<table>` in order: whether it only lays things out.
    layouts: Vec<bool>,
    /// How many `<table>` tags have started.
    tables: usize,
    /// Layout tables open around the text being read.
    laying: usize,
    stack: Vec<Open>,
    /// A collapsed space waits for the next visible character.
    space: bool,
    /// The next block opens a chapter: it starts a page.
    chapter: bool,
    /// The float open, by its place in the stack; inner ones read in line.
    float: Option<(usize, Arc<Floated>)>,
    /// The book's page colour, against which backgrounds show.
    page: css::Rgb,
    /// The book's own text colour, which the theme's ink stands for.
    book_ink: css::Rgb,
    /// A list item's marker, waiting for its first block.
    marker: Option<String>,
    marker_colour: Option<css::Rgb>,
    /// The first block of an outermost list keeps the full gap above.
    list_opened: bool,
    /// Just inside `<pre>`, where one leading newline is dropped.
    pre_opened: bool,
    /// The root's children, as `Open::kids` keeps an element's.
    top: Vec<El>,
    top_count: usize,
    /// Margins and padding waiting above the next block.
    gap: Gap,
    /// The open block's line height, in px.
    block_line: f32,
    /// Looks made in this chapter; the next equal one is shared.
    looks: Vec<Arc<Look>>,
    /// The chapter has a style of its own: a sheet or a `style` attribute.
    styled: bool,
    signals: Signals,
    /// The book's boxes open around what is read, outermost first.
    boxes: Vec<BoxOpen>,
    grid: Option<GridBuild>,
    /// An inline drawing read to its end before it is shown.
    drawing: Option<Drawing>,
    /// The cascade the next start tag takes: a generated box's.
    forced: Option<Decl>,
    /// The next box opened stands for an `<hr>`.
    rule_next: bool,
    /// Counters in scope: name, value, and their element's depth.
    counters: Vec<(String, i32, usize)>,
    /// Quotes open, which pick the next marks.
    quotes: usize,
    /// `::first-letter` and `::first-line` waiting for text, by their depth.
    initial: Option<(usize, Decl)>,
    first_line: Option<(usize, Decl)>,
    /// A floated inline starting a block, by depth: a drop cap.
    drop: Option<usize>,
    /// A list item's marker picture, waiting with its marker.
    marker_picture: Option<Arc<Picture>>,
    /// A fixed-layout chapter: its viewport, once read, and its page.
    fixed: bool,
    viewport: Option<(f32, f32)>,
    sheet: Option<Arc<Sheet>>,
    /// The root's ground was the body's, so the body box leaves it.
    grounded: bool,
    /// Containing blocks open on a fixed page, innermost last.
    places: Vec<Place>,
}

/// A fixed page's containing block: its depth, rect and positioned box.
#[derive(Clone)]
struct Place {
    depth: usize,
    rect: [f32; 4],
    spot: Option<Arc<Spot>>,
}

/// Vertical space waiting for the next block, in px: margins collapse.
#[derive(Debug, Default, Clone, Copy)]
struct Gap {
    positive: f32,
    negative: f32,
    fixed: f32,
}

impl Gap {
    fn margin(&mut self, px: f32) {
        if px >= 0.0 {
            self.positive = self.positive.max(px);
        } else {
            self.negative = self.negative.min(px);
        }
    }

    /// Space margins don't collapse through, like padding or a blank line.
    fn fixed(&mut self, px: f32) {
        self.fixed += self.positive + self.negative + px;
        (self.positive, self.negative) = (0.0, 0.0);
    }

    fn take(&mut self) -> f32 {
        let total = self.total();
        *self = Self::default();
        total
    }

    fn total(&self) -> f32 {
        self.fixed + self.positive + self.negative
    }
}

/// What tells a designed chapter from prose (`docs/epub-layout.md`).
#[derive(Debug, Default)]
struct Signals {
    /// Layout tables, flex, borders, block backgrounds, widths, `vh`.
    strong: bool,
    faces: bool,
    letters: bool,
    /// Paragraph sizes seen, in hundredths of an em.
    sizes: Vec<u32>,
    /// Characters of text, and those inside what signalled.
    text: usize,
    boxed: usize,
}

impl Signals {
    fn designed(&self) -> bool {
        let light = [self.faces, self.letters, self.sizes.len() >= 3];
        // Prose with a framed figure or note stays prose.
        let laid_out = self.text <= SHORT_CHAPTER || self.boxed * 10 >= self.text * 3;
        (self.strong && laid_out) || light.iter().filter(|s| **s).count() >= 2
    }
}

/// A chapter this many characters long, at most, reads as a laid-out page.
const SHORT_CHAPTER: usize = 800;

impl State<'_> {
    fn top(&self) -> Style {
        self.stack.last().map(|o| o.style).unwrap_or_default()
    }

    fn in_list(&self) -> bool {
        self.stack.iter().any(|o| o.name == "ul" || o.name == "ol")
    }

    /// Indents and quote bars from the open lists and quotes.
    fn frame(&mut self) -> Frame {
        let mut f = Frame::default();
        for open in &self.stack {
            match open.name.as_str() {
                "ul" | "ol" | "dd" => f.inset += LIST_INSET,
                "blockquote" => {
                    if f.bars.len() < MAX_BARS {
                        f.bars.push(f.inset);
                    }
                    f.inset += QUOTE_INSET;
                    f.soft = true;
                }
                _ => {}
            }
        }
        f.align = self.top().align;
        f.sided = self.top().sided;
        f.tight = self.in_list() && !std::mem::take(&mut self.list_opened);
        f.look = Some(self.look());
        f.boxes = self.box_arcs();
        f.cell = self.cell();
        f.float = self.float.as_ref().map(|(_, f)| Arc::clone(f));
        let turns = self.stack.iter().map(|o| o.computed.rotate);
        f.rotate = turns.sum::<f32>() % 360.0;
        f.before = self.gap.take() / ROOT_PX;
        if let Some(c) = self.block_computed() {
            f.keep = Keep {
                orphans: c.orphans,
                widows: c.widows,
                whole: c.whole,
                with_next: c.keep_next,
            };
            f.dir = c.dir;
            f.orient = c.orient;
        }
        f.turned = self.turned;
        f.sheet = self.sheet.clone();
        f.spot = self.places.last().and_then(|p| p.spot.clone());
        f
    }

    /// The open boxes, each made at its first block with its top inside.
    fn box_arcs(&mut self) -> Vec<Arc<Boxed>> {
        let (total, first) = (self.gap.total(), self.blocks.len());
        let mut arcs = Vec::with_capacity(self.boxes.len());
        for b in &mut self.boxes {
            if b.arc.is_none() {
                let mut spec = b.spec.clone();
                if spec.border[0] + spec.padding[0] > 0.0 {
                    spec.top = (total - b.gap.total()).max(0.0) / ROOT_PX;
                }
                (b.arc, b.first) = (Some(Arc::new(spec)), first);
            }
            arcs.extend(b.arc.clone());
        }
        arcs
    }

    /// The grid cell being read, if any.
    fn cell(&self) -> Option<Cell> {
        let row = self.grid.as_ref()?.row.as_ref()?;
        Some(Cell {
            row: Arc::clone(&row.placeholder),
            column: row.column?,
            depth: row.boxes,
        })
    }

    /// Whether the table `tag` starting now only lays things out.
    fn lays_out(&self, tag: &Tag) -> bool {
        let scanned = self.layouts.get(self.tables).copied().unwrap_or(false);
        self.table.is_none() && (scanned || typed(tag, "role", &["presentation", "none"]))
    }

    /// Starts `name`'s part in the layout grid, if it has one.
    fn grid_start(&mut self, name: &str, tag: &Tag, c: &Computed) -> Part {
        if name == "table" {
            if self.grid.is_some() && !tag.self_closing && self.lays_out(tag) {
                self.dissolve_flex();
            }
            if self.grid.is_some() || tag.self_closing || !self.lays_out(tag) {
                return Part::None;
            }
            let depth = self.stack.len();
            let both = c.margin.get(1).zip(c.margin.get(3));
            let columns = Columns {
                flex: None,
                fill: !matches!(c.width, Len::Auto),
                centre: both.is_some_and(|(r, l)| *r == Len::Auto && *l == Len::Auto),
            };
            self.grid = Some(GridBuild {
                depth,
                row: None,
                columns: Arc::new(columns),
                flex: None,
                taken: Vec::new(),
                single: false,
            });
            return Part::Table;
        }
        if let Some(part) = self.flex_start(name, tag, c) {
            return part;
        }
        // Only the grid's own rows and cells, not a nested table's.
        let table = self.stack.iter().rposition(|o| o.name == "table");
        if self.grid.as_ref().is_none_or(|g| Some(g.depth) != table) {
            return Part::None;
        }
        match name {
            "tr" => {
                self.end_row();
                let row = RowBuild::new(self.blocks.len());
                if let Some(grid) = self.grid.as_mut() {
                    grid.row = Some(row);
                }
                Part::Row
            }
            "td" | "th" => {
                let tr = self.stack.iter().rev().find(|o| o.name == "tr");
                let valign = c.valign.or(tr.and_then(|o| o.computed.valign));
                let fresh = self.grid.as_ref().and_then(|g| g.row.as_ref());
                let fresh = fresh.is_none_or(|r| r.widths.is_empty());
                // The first cell takes the space above the row.
                let before = match fresh {
                    true => {
                        self.box_arcs();
                        self.gap.take() / ROOT_PX
                    }
                    false => std::mem::take(&mut self.gap).total() / ROOT_PX,
                };
                let (first, boxes) = (self.blocks.len(), self.boxes.len());
                let Some(grid) = self.grid.as_mut() else {
                    return Part::None;
                };
                let row = grid.row.get_or_insert_with(|| RowBuild::new(first));
                if fresh {
                    row.before = before;
                }
                if row.widths.len() < MAX_COLUMNS {
                    // The first column no cell above still spans down into.
                    let span = |name: &str| {
                        let n = attr(tag, name).and_then(|v| v.trim().parse::<u16>().ok());
                        n.unwrap_or(1).clamp(1, MAX_COLUMNS as u16)
                    };
                    let (across, down) = (span("colspan"), span("rowspan").min(MAX_SPAN_DOWN));
                    let taken = |c: usize| grid.taken.get(c).is_some_and(|t| *t > 0);
                    let mut start = usize::from(row.next);
                    while taken(start) && start < MAX_COLUMNS {
                        start += 1;
                    }
                    let end = (start + usize::from(across)).min(MAX_COLUMNS);
                    if down > 1 {
                        grid.taken.resize(grid.taken.len().max(end), 0);
                        for t in grid.taken.iter_mut().take(end).skip(start) {
                            *t = down;
                        }
                    }
                    row.widths.push(cell_width(c));
                    row.valign.push(valign.unwrap_or_default());
                    row.starts.push(u16::try_from(start).unwrap_or(0));
                    row.spans.push(across);
                    row.down.push(down);
                    row.next = u16::try_from(end).unwrap_or(u16::MAX);
                }
                let column = row.widths.len().saturating_sub(1);
                row.column = u16::try_from(column).ok();
                row.boxes = u8::try_from(boxes).unwrap_or(u8::MAX);
                Part::Cell
            }
            _ => Part::None,
        }
    }

    /// Starts a flex row's container or one of its items, if `name` is.
    fn flex_start(&mut self, name: &str, tag: &Tag, c: &Computed) -> Option<Part> {
        let flex = c.display == Display::Flex && name != "table" && !tag.self_closing;
        let row = flex && !c.flex_column;
        // A column stack's items after the first keep its row gap above.
        let stacked = self
            .stack
            .last()
            .filter(|o| o.part == Part::Stack && o.count > 1);
        if let Some(gap) = stacked.map(|o| o.computed.row_gap).filter(|g| *g > 0.0) {
            self.gap.fixed(gap);
        }
        // A row of one inside lets go, so this row lays out instead.
        let single = self.grid.as_ref().is_some_and(|g| g.single);
        if row && (self.grid.is_none() || (single && self.dissolve_flex())) {
            let flex = Flex {
                justify: c.justify,
                gap: c.gap / ROOT_PX,
                wrap: c.wrap,
            };
            let columns = Columns {
                flex: Some(flex),
                ..Columns::default()
            };
            self.grid = Some(GridBuild {
                depth: self.stack.len(),
                row: Some(RowBuild::new(self.blocks.len())),
                columns: Arc::new(columns),
                flex: Some(c.align_items.unwrap_or(VAlign::Top)),
                taken: Vec::new(),
                single: false,
            });
            return Some(Part::FlexBox);
        }
        if let Some(part) = self.stack_item(c) {
            return Some(part);
        }
        let item = (self.grid.as_ref()).is_some_and(|g| g.depth + 1 == self.stack.len());
        if flex && !item {
            return Some(Part::Stack);
        }
        let grid = self.grid.as_ref()?;
        let align = grid.flex?;
        if grid.depth + 1 != self.stack.len() {
            return Some(Part::None);
        }
        let fresh = grid.row.as_ref().is_none_or(|r| r.widths.is_empty());
        let before = match fresh {
            true => {
                self.box_arcs();
                self.gap.take() / ROOT_PX
            }
            false => std::mem::take(&mut self.gap).total() / ROOT_PX,
        };
        let (first, boxes) = (self.blocks.len(), self.boxes.len());
        let row = self
            .grid
            .as_mut()?
            .row
            .get_or_insert_with(|| RowBuild::new(first));
        if fresh {
            row.before = before;
        }
        if row.widths.len() >= MAX_COLUMNS {
            return Some(Part::None);
        }
        let basis = match c.basis {
            Len::Auto => cell_width(c),
            Len::Px(px) if px > 0.0 => Some(Length::Em(px / ROOT_PX)),
            Len::Percent(p) if p > 0.0 => Some(Length::Percent(p)),
            Len::Mix { px, pct, .. } => mixed(px, pct),
            _ => None,
        };
        row.widths.push(basis);
        row.valign.push(c.align_self.unwrap_or(align));
        row.grow.push(c.grow);
        row.column = u16::try_from(row.widths.len() - 1).ok();
        row.boxes = u8::try_from(boxes).unwrap_or(u8::MAX);
        Some(Part::FlexItem)
    }

    /// Starts an item of the column stack holding it, as a row of one.
    fn stack_item(&mut self, c: &Computed) -> Option<Part> {
        let parent = self.stack.last().filter(|o| o.part == Part::Stack)?;
        let p = &parent.computed;
        let stretch = (c.self_stretch).unwrap_or(p.items_stretch && c.align_self.is_none());
        let justify = match c.align_self.or(p.align_items) {
            _ if stretch => Justify::Start,
            Some(VAlign::Middle) => Justify::Center,
            Some(VAlign::Bottom) => Justify::End,
            _ => Justify::Start,
        };
        let width = cell_width(c);
        // Stretched across, an item lays out as it would in prose.
        if stretch && width.is_none() {
            return None;
        }
        let single = self.grid.as_ref().is_some_and(|g| g.single);
        if self.grid.is_some() && !(single && self.dissolve_flex()) {
            return None;
        }
        let depth = self.stack.len() - 1;
        self.box_arcs();
        let mut row = RowBuild::new(self.blocks.len());
        row.before = self.gap.take() / ROOT_PX;
        row.grow.push(0.0);
        row.widths.push(width);
        row.valign.push(VAlign::Top);
        (row.column, row.boxes) = (Some(0), u8::try_from(self.boxes.len()).unwrap_or(u8::MAX));
        let flex = Flex {
            justify,
            gap: 0.0,
            wrap: false,
        };
        let columns = Columns {
            flex: Some(flex),
            ..Columns::default()
        };
        self.grid = Some(GridBuild {
            depth,
            row: Some(row),
            columns: Arc::new(columns),
            flex: Some(VAlign::Top),
            taken: Vec::new(),
            single: true,
        });
        Some(Part::StackItem)
    }

    /// Lets go of a one-item flex row: its layout table becomes the grid.
    fn dissolve_flex(&mut self) -> bool {
        let single = self.grid.as_ref().is_some_and(|g| {
            g.flex.is_some() && g.row.as_ref().is_none_or(|r| r.widths.len() <= 1)
        });
        if !single {
            return false;
        }
        let Some(row) = self.grid.take().and_then(|g| g.row) else {
            return true;
        };
        let mut emitted = false;
        for block in self.blocks.iter_mut().skip(row.first) {
            emitted = true;
            if block
                .frame
                .cell
                .as_ref()
                .is_some_and(|c| Arc::ptr_eq(&c.row, &row.placeholder))
            {
                block.frame.cell = None;
            }
        }
        if !emitted {
            self.gap.fixed(row.before * ROOT_PX);
        }
        true
    }

    /// Ends a grid cell: what follows is outside it.
    fn end_cell(&mut self) {
        self.gap = Gap::default();
        if let Some(row) = self.grid.as_mut().and_then(|g| g.row.as_mut()) {
            row.column = None;
        }
    }

    /// Ends the grid row, giving its blocks the row they share.
    fn end_row(&mut self) {
        let Some(row) = self.grid.as_mut().and_then(|g| g.row.take()) else {
            return;
        };
        let Some(grid) = self.grid.as_mut() else {
            return;
        };
        for t in &mut grid.taken {
            *t = t.saturating_sub(1);
        }
        let real = Arc::new(GridRow {
            widths: row.widths,
            valign: row.valign,
            before: row.before,
            starts: row.starts,
            spans: row.spans,
            grow: row.grow,
            down: row.down,
            columns: Arc::clone(&grid.columns),
        });
        for block in self.blocks.iter_mut().skip(row.first) {
            if let Some(cell) = block.frame.cell.as_mut()
                && Arc::ptr_eq(&cell.row, &row.placeholder)
            {
                cell.row = Arc::clone(&real);
            }
        }
        self.gap = Gap::default();
    }

    /// Opens `name`'s space above, as a box if it draws or sits apart.
    fn open_block(&mut self, tag: &Tag, (c, d): (&Computed, &Decl), part: Part) -> bool {
        let cell = part == Part::Cell;
        if !cell {
            self.gap.margin(c.margin_px(0));
        }
        let name: &str = &tag.name;
        // A table of data draws its own grid, not a box.
        let layout = name != "table" || self.lays_out(tag);
        let rule = std::mem::take(&mut self.rule_next);
        // A fixed page's root ground is drawn under the whole page.
        let ground = self.grounded && name == "body";
        let (contrast, backdrop, finish) = match layout && !ground {
            true => self.paint(c),
            false => (None, None, None),
        };
        let spec = layout.then(|| boxed(name, c, d, cell)).flatten();
        let shows = finish.as_ref().is_some_and(Finish::shows);
        let painted = contrast.is_some() || backdrop.is_some() || shows;
        // Kept whole, a plain wrapper's blocks need a box to share.
        let wraps = c.whole && !TEXT_BLOCKS.contains(&name);
        let spec = spec.or_else(|| (painted || wraps).then(|| bare(c)));
        let colours = c
            .border_colour
            .map(|ink| ink.and_then(|ink| self.drawn(ink, MAX_BORDER_FADE)));
        let finish = finish.map(Arc::new);
        let spec = spec.map(|spec| Boxed {
            fill: contrast.map(|c| c.min(MAX_TEXT_FILL)),
            colours,
            backdrop,
            finish: finish.as_deref().map(|f| Arc::new(capped(f))),
            rule,
            whole: c.whole,
            ..spec
        });
        let snapshot = self.gap;
        let inside = c.border[0] + c.padding[0];
        if inside > 0.0 {
            self.gap.fixed(inside);
        }
        let Some(spec) = spec else {
            return false;
        };
        self.boxes.push(BoxOpen {
            spec,
            arc: None,
            gap: snapshot,
            first: self.blocks.len(),
            contrast,
            finish,
        });
        true
    }

    /// A background's ink strength, its contrast with the page; its picture.
    fn paint(&mut self, c: &Computed) -> (Option<f32>, Option<Arc<Backdrop>>, Option<Finish>) {
        let contrast = c
            .fill
            .map(|f| (css::luminance(f) - css::luminance(self.page)).abs());
        let backdrop = c.picture.as_deref().and_then(|p| self.backdrop(p, c));
        (
            contrast.filter(|c| *c >= MIN_FILL),
            backdrop,
            self.box_finish(c),
        )
    }

    /// A background picture at `url`, sized and placed as `c` says.
    fn backdrop(&mut self, url: &str, c: &Computed) -> Option<Arc<Backdrop>> {
        let picture = self.css_picture(url)?;
        let length = |l: Option<Len>| match l? {
            Len::Px(px) => Some(Length::Em(px / ROOT_PX)),
            Len::Em(e) | Len::Rem(e) => Some(Length::Em(e)),
            Len::Percent(p) => Some(Length::Percent(p)),
            Len::Mix { px, em, pct } => Some(Length::Mix {
                em: px / ROOT_PX + em,
                percent: pct,
            }),
            _ => None,
        };
        let size = match c.picture_size {
            PictureSize::Auto => BackdropSize::Auto,
            PictureSize::Cover => BackdropSize::Cover,
            PictureSize::Contain => BackdropSize::Contain,
            PictureSize::Set([w, h]) => BackdropSize::Set([length(w), length(h)]),
        };
        Some(Arc::new(Backdrop {
            picture,
            size,
            at: c.picture_at,
            repeat: c.picture_repeat,
        }))
    }

    /// How strongly `tint` shows on the page, as a share of the ink.
    fn strength(&self, (rgb, alpha): Tint, c: &Computed) -> f32 {
        let rgb = rgb.or(c.ink).unwrap_or(self.book_ink);
        let contrast = (css::luminance(rgb) - css::luminance(self.page)).abs();
        (contrast * alpha).clamp(0.0, 1.0)
    }

    /// Shadows, gradients, a border picture, an outline and opacity.
    fn box_finish(&mut self, c: &Computed) -> Option<Finish> {
        let em = |l: Len| l.px(c.size, css::BASE_WIDTH).unwrap_or(0.0) / ROOT_PX;
        let shadows = c.shadows.iter().map(|s| mareader_reflow::Shadow {
            at: s.at.map(em),
            blur: em(s.blur).max(0.0),
            spread: em(s.spread),
            strength: self.strength(s.tint, c),
            inset: s.inset,
        });
        let shadows = shadows.filter(|s| s.strength >= 0.01).collect();
        let mut layers = Vec::new();
        for layer in c.layers.iter() {
            let made = match layer {
                decor::Layer::Url(url) => {
                    self.backdrop(url, c).map(mareader_reflow::Layer::Picture)
                }
                decor::Layer::Gradient(g) => {
                    Some(mareader_reflow::Layer::Gradient(self.gradient(g, c)))
                }
            };
            layers.extend(made);
        }
        let frame = c.frame.as_deref().and_then(|url| self.frame9(url, c));
        let outline = (c.outline > 0.0 && c.outline_style != BorderStyle::None).then(|| Outline {
            width: c.outline / ROOT_PX,
            offset: c.outline_offset / ROOT_PX,
            edge: edge(c.outline_style),
            colour: c
                .outline_colour
                .and_then(|ink| self.drawn(ink, MAX_BORDER_FADE)),
        });
        let finish = Finish {
            shadows,
            layers,
            frame,
            outline,
            opacity: if c.invisible {
                0.0
            } else {
                c.opacity.clamp(0.0, 1.0)
            },
        };
        (finish.shows() || finish.opacity < 0.99).then_some(finish)
    }

    /// A gradient's stops as strengths, its places as lengths.
    fn gradient(&self, g: &decor::Gradient, c: &Computed) -> mareader_reflow::Gradient {
        let place = |l: Len| match l {
            Len::Percent(p) => Length::Percent(p),
            Len::Em(e) => Length::Em(e * c.size / ROOT_PX),
            Len::Mix { px, em, pct } => Length::Mix {
                em: (px + em * c.size) / ROOT_PX,
                percent: pct,
            },
            other => Length::Em(other.px(c.size, css::BASE_WIDTH).unwrap_or(0.0) / ROOT_PX),
        };
        let shape = match g.radial {
            true => GradientShape::Radial {
                at: g.at,
                circle: g.circle,
                near: g.near,
            },
            false => GradientShape::Linear(g.angle),
        };
        mareader_reflow::Gradient {
            shape,
            stops: g
                .stops
                .iter()
                .map(|(tint, at)| (at.map(place), self.strength(*tint, c)))
                .collect(),
            repeating: g.repeating,
        }
    }

    /// A border picture at `url`, cut and sized as `c` says.
    fn frame9(&mut self, url: &str, c: &Computed) -> Option<Frame9> {
        let picture = self.css_picture(url)?;
        let (pw, ph) = (picture.width as f32, picture.height as f32);
        if pw < 1.0 || ph < 1.0 {
            return None;
        }
        let s = c.frame_slice;
        let dims = [ph, pw, ph, pw];
        let at = |a: &[f32; 4], i: usize| a.get(i).copied().unwrap_or_default();
        let slice: [f32; 4] = std::array::from_fn(|i| {
            let cut = at(&s.cut, i);
            let share = match s.percent.get(i).copied().unwrap_or(true) {
                true => cut / 100.0,
                false => cut / at(&dims, i),
            };
            share.clamp(0.0, 1.0)
        });
        let span = |spans: &[Span; 4], i: usize| {
            let px = match spans.get(i).copied().unwrap_or_default() {
                Span::Auto => at(&slice, i) * at(&dims, i),
                Span::Times(n) => n * at(&c.border, i),
                Span::Len(l) => l.px(c.size, css::BASE_WIDTH).unwrap_or(0.0),
            };
            px.max(0.0) / ROOT_PX
        };
        let width = std::array::from_fn(|i| span(&c.frame_width, i));
        let outset = std::array::from_fn(|i| span(&c.frame_outset, i));
        Some(Frame9 {
            picture,
            slice,
            fill: s.fill,
            width,
            outset,
            tile: c.frame_tile,
        })
    }

    /// Closes the innermost box, giving its blocks its bottom inside.
    fn close_box(&mut self, c: &Computed) {
        let Some(mut open) = self.boxes.pop() else {
            return self.space_below(c);
        };
        let inside = c.border[2] + c.padding[2];
        let shows = open.spec.finish.as_ref().is_some_and(|f| f.shows());
        let painted = open.spec.fill.is_some() || open.spec.backdrop.is_some() || shows;
        let edged = painted || open.spec.border.iter().any(|w| *w > 0.0);
        let held = self.blocks.get(open.first..).unwrap_or_default();
        if edged && open.arc.is_some() && held.iter().all(Block::is_blank) {
            // Only blank lines inside, which aren't laid: an empty box.
            self.blocks.truncate(open.first);
            open.arc = None;
        }
        match &open.arc {
            Some(arc) if inside > 0.0 => {
                self.gap.fixed(inside);
                let mut spec = (**arc).clone();
                spec.bottom = self.gap.total() / ROOT_PX;
                let spec = Arc::new(spec);
                for block in self.blocks.iter_mut().skip(open.first) {
                    for b in &mut block.frame.boxes {
                        if Arc::ptr_eq(b, arc) {
                            *b = Arc::clone(&spec);
                        }
                    }
                }
            }
            Some(_) => {}
            // An empty bordered box, a book's rule: a blank block of it.
            None if edged => {
                let tall = open.spec.height.unwrap_or(0.0) * ROOT_PX;
                let gaps = (self.gap.total() - open.gap.total() + inside).max(0.0);
                let height = (gaps + tall) / ROOT_PX;
                self.gap = open.gap;
                // No text to hide: a line or block shows at full strength.
                open.spec.fill = open.contrast;
                open.spec.finish = open.finish.clone();
                self.boxes.push(open);
                let mut block = Block::new(Content::Space(height.max(0.0)));
                block.frame = self.frame();
                self.boxes.pop();
                self.emit(block);
            }
            None if inside > 0.0 => self.gap.fixed(inside),
            None => {}
        }
        self.gap.margin(c.margin_px(2));
    }

    /// The computed style of the innermost open block element.
    fn block_computed(&self) -> Option<&Computed> {
        let open = self.stack.iter().rev().find(|o| is_block(&o.name));
        open.or(self.stack.last()).map(|o| &o.computed)
    }

    fn block_style(&self) -> Computed {
        self.block_computed().cloned().unwrap_or_default()
    }

    /// The look of a block starting now; the chapter decides if it shows.
    fn look(&mut self) -> Arc<Look> {
        let c = self.block_style();
        self.block_line = c.line_px();
        let faces = self.faces(c.family.as_deref());
        let look = Look {
            designed: false,
            size: c.size / ROOT_PX,
            line_height: match c.line {
                Line::Normal => None,
                Line::Factor(f) => Some(f),
                Line::Px(px) => Some(px / c.size),
            },
            letter: c.letter / ROOT_PX,
            word: c.word / ROOT_PX,
            indent: c.indent.px(c.size, css::BASE_WIDTH).unwrap_or(0.0) / ROOT_PX,
            weight: f32::from(c.weight),
            faces,
            justify: c.align == TextAlign::Justify,
            last: c.last,
        };
        if let Some(known) = self.looks.iter().rev().take(8).find(|l| ***l == look) {
            return Arc::clone(known);
        }
        let look = Arc::new(look);
        self.looks.push(Arc::clone(&look));
        look
    }

    /// `family` as faces: the book's own by their registered names.
    fn faces(&mut self, family: Option<&[String]>) -> Vec<String> {
        let mut out = Vec::new();
        for name in family.unwrap_or_default() {
            let lower = name.to_lowercase();
            if GENERIC.contains(&lower.as_str()) {
                // Sans and mono stay apart from the reader's body face.
                if matches!(lower.as_str(), "sans-serif" | "monospace") {
                    out.push(lower);
                }
                break;
            }
            let shipped: Vec<FontFace> = (self.sheets.iter())
                .flat_map(|s| &s.faces)
                .filter(|f| f.family.to_lowercase() == lower)
                .cloned()
                .collect();
            if shipped.is_empty() {
                out.push(name.clone());
                continue;
            }
            for face in shipped {
                if !self.refs.faces.contains(&face) {
                    self.refs.faces.push(face);
                }
            }
            out.push(format!("{BOOK_FACE}{lower}"));
        }
        out
    }

    /// How text written now differs from its block: weight, slant, size.
    fn emphasis(&self) -> Emphasis {
        let style = self.top();
        let c = self.stack.last().map(|o| &o.computed);
        let (weight, size) = c.map_or((400, ROOT_PX), |c| (c.weight, c.size));
        let block = self.block_computed();
        let (block_weight, block_size) = block.map_or((400, ROOT_PX), |c| (c.weight, c.size));
        let ratio = size / block_size.max(1.0);
        let sized = (ratio - 1.0).abs() > 0.02;
        // Raised by the nearest inline that says so; decorations add up.
        let inline = self.stack.iter().rev().take_while(|o| !is_block(&o.name));
        let script = inline
            .map(|o| o.computed.script)
            .find(|s| *s != Script::Normal);
        let (underline, strike) = self.stack.iter().fold((false, false), |(u, s), o| {
            (u || o.computed.decoration.0, s || o.computed.decoration.1)
        });
        Emphasis {
            italic: c.is_some_and(|c| c.italic),
            bold: weight >= 600 && weight > block_weight,
            light: weight < 600 && block_weight >= 600,
            code: style.code,
            size: if sized {
                (ratio * 100.0).round().clamp(1.0, 1000.0) as u16
            } else {
                0
            },
            script: script.unwrap_or_default(),
            case: c.map_or(Case::Normal, Computed::case),
            underline,
            strike,
            colour: self.text_colour(c),
            shadow: c.and_then(|c| self.text_shadow(c)),
            hidden: c.is_some_and(|c| c.invisible),
            features: c.map_or_else(Features::default, Computed::features),
            face: 0,
        }
    }

    /// The text's shadow, unless it barely differs from the page.
    fn text_shadow(&self, c: &Computed) -> Option<TextShadow> {
        let s = c.text_shadow.as_ref()?;
        let strength = self.strength(s.tint, c);
        let share = |l: Len| l.px(c.size, css::BASE_WIDTH).unwrap_or(0.0) / c.size.max(1.0) * 100.0;
        (strength >= MIN_FILL).then(|| TextShadow {
            at: s.at.map(|l| share(l).round().clamp(-127.0, 127.0) as i8),
            blur: share(s.blur).round().clamp(0.0, 255.0) as u8,
            strength: (strength * 255.0).round() as u8,
        })
    }

    /// Text's colour as drawn, faded toward its ground by opacity.
    fn text_colour(&self, c: Option<&Computed>) -> Option<css::Rgb> {
        let c = c?;
        if c.opacity >= 0.99 {
            return c.ink.and_then(|ink| self.drawn(ink, MAX_FADE));
        }
        let ink = c.ink.unwrap_or(self.book_ink);
        let ground = self.ground();
        let a = c.opacity.clamp(0.0, 1.0);
        let mixed: css::Rgb = std::array::from_fn(|i| {
            let (f, g) = (ink.get(i).copied(), ground.get(i).copied());
            let (f, g) = (f32::from(f.unwrap_or(0)), f32::from(g.unwrap_or(255)));
            (f * a + g * (1.0 - a)).round().clamp(0.0, 255.0) as u8
        });
        self.drawn(mixed, MAX_FADE)
    }

    /// The background text sits on: its nearest block's, or the page.
    fn ground(&self) -> css::Rgb {
        let ground = self
            .stack
            .iter()
            .rev()
            .find_map(|o| match is_block(&o.name) {
                true => o.computed.fill,
                false => None,
            });
        ground.unwrap_or(self.page)
    }

    /// `ink` as the page draws it, given the background it sits on.
    fn drawn(&self, ink: css::Rgb, most: f32) -> Option<css::Rgb> {
        drawn(ink, (self.ground(), self.book_ink, self.page), most)
    }

    /// The colour a data table's grid takes: its first drawn border's.
    fn rule(&mut self, c: &Computed) {
        let seen = c.border.iter().zip(&c.border_style).zip(&c.border_colour);
        let ink = seen
            .filter(|((w, s), _)| **w > 0.0 && **s != BorderStyle::None)
            .find_map(|(_, ink)| *ink);
        let drawn = ink.and_then(|ink| self.drawn(ink, MAX_BORDER_FADE));
        if let Some(t) = self.table.as_mut().filter(|t| t.nested == 0) {
            t.table.rule = t.table.rule.or(drawn);
        }
    }

    fn role(&self) -> Role {
        let level = self.stack.iter().rev().find_map(|o| heading(&o.name));
        match level {
            Some(level) => Role::Heading(level),
            None if self.top().pre => Role::Preformatted,
            None => Role::Paragraph,
        }
    }

    fn emit(&mut self, mut block: Block) {
        block.break_before = std::mem::take(&mut self.chapter);
        if let Content::Text(t) = &block.content {
            let boxed = self.laying > 0 || self.stack.iter().any(|o| o.signals);
            let n = t.text.chars().count();
            self.signals.text += n;
            self.signals.boxed += if boxed { n } else { 0 };
        }
        let at = Anchor {
            block: self.first + self.blocks.len(),
            offset: 0,
        };
        for key in self.pending.drain(..) {
            self.refs.places.entry(key).or_insert(at);
        }
        self.blocks.push(block);
    }

    /// Notes where `id` is: mid-paragraph, or the next block.
    fn mark(&mut self, id: &str) {
        let id = id.trim();
        if id.is_empty() || self.refs.places.len() + self.pending.len() >= MAX_PLACES {
            return;
        }
        let key = format!("{}#{id}", self.base);
        let written = match self.current.as_ref().map(|b| &b.content) {
            Some(Content::Text(t)) if self.table.is_none() => t.text.len(),
            _ => 0,
        };
        if written == 0 {
            return self.pending.push(key);
        }
        let at = Anchor {
            block: self.first + self.blocks.len(),
            offset: written,
        };
        self.refs.places.entry(key).or_insert(at);
    }

    /// `href`'s index in [`Refs::hrefs`], resolved from this chapter.
    fn link_to(&mut self, href: &str) -> Option<u32> {
        let href = href.trim();
        let fragment = href.split_once('#').map(|(_, f)| href::percent_decode(f));
        let fragment = fragment.filter(|f| !f.is_empty());
        let target = if let (true, Some(f)) = (href.starts_with('#'), &fragment) {
            Href::Inside(format!("{}#{f}", self.base))
        } else if let Some(path) = href::resolve(self.base, href) {
            match fragment {
                Some(f) => Href::Inside(format!("{path}#{f}")),
                None => Href::Inside(path),
            }
        } else if href.contains(':') {
            Href::Outside(href.to_owned())
        } else {
            return None;
        };
        if self.refs.hrefs.len() >= MAX_LINKS {
            return None;
        }
        let index = u32::try_from(self.refs.hrefs.len()).ok()?;
        self.refs.hrefs.push(target);
        Some(index)
    }

    fn finish(&mut self) {
        if let Some(mut block) = self.current.take() {
            let written = matches!(&block.content, Content::Text(t) if !t.text.is_empty() || !t.inlines.is_empty());
            if let Content::Text(text) = &mut block.content {
                text.trim_end();
            }
            if !block.is_blank() {
                self.emit(block);
            } else if written {
                // A line of only spaces still takes its height.
                self.gap
                    .fixed(block.frame.before * ROOT_PX + self.block_line);
            }
        }
        self.space = false;
    }

    /// The text that characters go to now, if any.
    fn target(&mut self) -> Option<&mut Text> {
        if self.table.is_some() {
            return self.table.as_mut().and_then(TableBuild::cell);
        }
        if self.current.is_none() {
            let mut frame = self.frame();
            frame.marker = self.marker.take();
            frame.marker_colour = self.marker_colour.take();
            frame.marker_picture = self.marker_picture.take();
            let role = self.role();
            if let Some(look) = frame.look.clone() {
                self.observe_text(&look, role);
            }
            let mut block = Block::new(Content::Text(Text::new(role)));
            block.frame = frame;
            self.current = Some(block);
        }
        match self.current.as_mut().map(|b| &mut b.content) {
            Some(Content::Text(text)) => Some(text),
            _ => None,
        }
    }

    /// Text, its block's first letter and line styled as rules say.
    fn characters(&mut self, input: &str) {
        let pending = self.initial.is_some() || self.first_line.is_some();
        let fresh = pending && self.table.is_none() && !self.top().pre && self.fresh();
        if !fresh || input.chars().all(is_html_space) || self.top().hidden {
            return self.read_text(input);
        }
        let line = self.first_line.take().map(|(_, d)| self.emphasis_with(d));
        let body = input.trim_start_matches(is_html_space);
        let split = first_letter(body);
        match self.initial.take().filter(|_| split > 0) {
            Some((_, d)) => {
                let (letter, rest) = body.split_at(split);
                self.read_text(input.get(..input.len() - body.len()).unwrap_or_default());
                self.forced = Some(d);
                self.start(&synthetic("span"));
                self.read_text(letter);
                self.end("span");
                self.read_text(rest);
            }
            None => self.read_text(input),
        }
        if let (Some(line), Some(Content::Text(t))) =
            (line, self.current.as_mut().map(|b| &mut b.content))
        {
            t.first_line = Some(line);
        }
    }

    /// Whether the open block has no text yet.
    fn fresh(&self) -> bool {
        match self.current.as_ref().map(|b| &b.content) {
            None => true,
            Some(Content::Text(t)) => t.text.trim().is_empty() && t.inlines.is_empty(),
            Some(_) => false,
        }
    }

    /// The emphasis text styled by `d` would take, here.
    fn emphasis_with(&mut self, d: Decl) -> Emphasis {
        self.forced = Some(d);
        self.start(&synthetic("span"));
        let emphasis = self.emphasis();
        self.end("span");
        emphasis
    }

    fn read_text(&mut self, input: &str) {
        let style = self.top();
        if self.stack.last().is_some_and(|o| o.name == "style") {
            if self.style_text.len() + input.len() <= MAX_SHEET {
                self.style_text.push_str(input);
            }
            return;
        }
        if style.hidden || style.graphic {
            return;
        }
        if style.note {
            return self.read_note(input);
        }
        let emphasis = self.emphasis();
        let face = self.run_face();
        if style.pre {
            let mut text = input.replace('\r', "").replace('\t', TAB);
            if std::mem::take(&mut self.pre_opened) && text.starts_with('\n') {
                text.remove(0);
            }
            if let Some(t) = self.target() {
                let emphasis = faced(t, face, emphasis);
                put(t, &text, emphasis, style.link);
            }
            return;
        }
        if input.chars().all(is_html_space) {
            self.space |= !input.is_empty();
            return;
        }
        let mut space = self.space;
        let Some(target) = self.target() else {
            return;
        };
        let mut text = String::with_capacity(input.len());
        for c in input.chars() {
            if is_html_space(c) {
                space = true;
                continue;
            }
            let after_picture = target
                .inlines
                .last()
                .is_some_and(|i| i.at == target.text.len());
            let line_start = text.is_empty()
                && !after_picture
                && (target.text.is_empty() || target.text.ends_with('\n'));
            if std::mem::take(&mut space) && !line_start {
                text.push(' ');
            }
            text.push(c);
        }
        let emphasis = faced(target, face, emphasis);
        put(target, &text, emphasis, style.link);
        self.space = space;
    }

    /// Ruby note text, its spaces collapsed.
    fn read_note(&mut self, input: &str) {
        let words: Vec<&str> = input
            .split(is_html_space)
            .filter(|w| !w.is_empty())
            .collect();
        if words.is_empty() || self.note.len() > MAX_NOTE {
            return;
        }
        if !self.note.is_empty() && input.starts_with(is_html_space) {
            self.note.push(' ');
        }
        self.note.push_str(&words.join(" "));
    }

    /// Bytes in the text being read, if a block is open.
    fn text_len(&mut self) -> usize {
        if self.current.is_none() && self.table.is_none() {
            return 0;
        }
        self.target().map_or(0, |t| t.text.len())
    }

    /// The note just read, set over the base before it.
    fn annotate(&mut self) {
        let note = std::mem::take(&mut self.note);
        let Some((start, Some(end))) = self.ruby else {
            return;
        };
        let Some(t) = self.target() else {
            return;
        };
        let base = t.text.get(start..end.min(t.text.len())).unwrap_or_default();
        let lead = base.len() - base.trim_start().len();
        let range = start + lead..start + base.trim_end().len();
        let apart = t.ruby.last().is_none_or(|a| a.range.end <= range.start);
        let fits = !note.is_empty() && note.len() <= MAX_NOTE;
        if fits && !range.is_empty() && apart {
            t.ruby.push(Annotation { range, text: note });
        }
        let next = t.text.len();
        self.ruby = Some((next, None));
    }

    /// The book's family for the run being read, where it isn't its block's.
    fn run_face(&mut self) -> Option<Vec<String>> {
        let own = self.stack.last().and_then(|o| o.computed.family.clone())?;
        let block = self.block_computed().and_then(|c| c.family.clone());
        if block.as_deref() == Some(&*own) {
            return None;
        }
        Some(self.faces(Some(&own))).filter(|f| !f.is_empty())
    }

    fn line_break(&mut self) {
        let pre = self.top().pre;
        let in_table = self.table.is_some();
        let blank = (!in_table && !self.top().hidden).then(|| self.block_style().line_px());
        let target = if pre || in_table {
            self.target()
        } else {
            self.current.as_mut().and_then(|b| match &mut b.content {
                Content::Text(t) => Some(t),
                _ => None,
            })
        };
        match target.filter(|t| pre || !t.text.is_empty()) {
            Some(t) => t.push("\n", Emphasis::default()),
            // A break with nothing before it holds a blank line open.
            None => {
                if let Some(line) = blank {
                    self.gap.fixed(line);
                }
            }
        }
        self.space = false;
    }

    /// A block boundary: new block outside tables, new line in a cell.
    fn boundary(&mut self, name: &str) {
        let Some(table) = self.table.as_mut() else {
            return self.finish();
        };
        let structural = matches!(
            name,
            "table" | "tr" | "td" | "th" | "thead" | "tbody" | "tfoot" | "caption"
        );
        if !structural
            && let Some(cell) = table.cell()
            && !cell.text.is_empty()
            && !cell.text.ends_with('\n')
        {
            cell.push("\n", Emphasis::default());
        }
        self.space = false;
    }

    /// Ends elements HTML closes implicitly, like `<p>` before `<div>`.
    fn close_implied(&mut self, name: &str) {
        let closes: &[&str] = match name {
            "li" => &["p", "li"],
            "dt" | "dd" => &["p", "dt", "dd"],
            "tr" => &["p", "tr", "td", "th"],
            "td" | "th" => &["p", "td", "th"],
            _ if is_block(name) => &["p"],
            _ => &[],
        };
        while let Some(at) = self.stack.iter().rposition(|o| is_block(&o.name)) {
            let innermost = self.stack.get(at).map(|o| o.name.as_str());
            if !innermost.is_some_and(|n| closes.contains(&n)) {
                break;
            }
            self.close_from(at);
        }
    }

    /// Counts a list item; `value` resets the count.
    /// Gives its number and the list's marker.
    fn next_item(&mut self, value: Option<u32>) -> (u32, ListStyle) {
        let depth = self
            .stack
            .iter()
            .filter(|o| o.name == "ul" || o.name == "ol")
            .count();
        let list = self
            .stack
            .iter_mut()
            .rev()
            .find(|o| o.name == "ul" || o.name == "ol");
        let Some(list) = list else {
            return (1, ListStyle::Symbol('•'));
        };
        list.counter = value.unwrap_or_else(|| list.counter.saturating_add(1));
        let style = match depth % 3 {
            _ if list.name == "ol" => ListStyle::Decimal,
            2 => ListStyle::Symbol('◦'),
            0 => ListStyle::Symbol('▪'),
            _ => ListStyle::Symbol('•'),
        };
        (list.counter, style)
    }

    /// Sets a list item's marker: its text, picture and colour.
    fn list_marker(
        &mut self,
        tag: &Tag,
        (computed, style): (&Computed, &Style),
        marker: Option<Decl>,
    ) {
        let value = attr(tag, "value").and_then(|v| v.trim().parse::<u32>().ok());
        // An unmarked item still counts, as CSS counters do.
        let (n, own) = self.next_item(value);
        let list_style = computed.list_style.clone().unwrap_or(own);
        let n = i32::try_from(n).unwrap_or(i32::MAX);
        let mut text = generated::marker(n, &list_style).filter(|_| !style.unmarked);
        let picture = (computed.list_image.as_deref()).and_then(|url| self.css_picture(url));
        let mut ink = computed.ink;
        if let Some(d) = marker {
            if let Some(Generated::Items(items)) = &d.content {
                let (shown, _) = self.content_text(items);
                text = Some(shown).filter(|s| !s.trim().is_empty());
            }
            ink = computed.child(&d).ink.or(ink);
        }
        self.marker_picture = picture;
        self.marker = match self.marker_picture.is_some() {
            true => Some(String::new()),
            false => text,
        };
        self.marker_colour = ink.and_then(|ink| self.drawn(ink, MAX_FADE));
    }

    /// A picture a stylesheet names, by archive path or `data:` URL.
    fn css_picture(&mut self, url: &str) -> Option<Arc<Picture>> {
        match url.starts_with("data:") {
            true => href::data_url(url)
                .and_then(|b| Picture::probe(b).ok())
                .map(Arc::new),
            false => self.library.picture(url),
        }
    }

    /// Applies `d`'s counter resets, increments and sets at `depth`.
    fn count(&mut self, d: &Decl, depth: usize) {
        let each = |c: &Option<css::Counters>| c.as_deref().unwrap_or_default().to_vec();
        let resets = each(&d.counter_reset).into_iter().map(|c| (c, Step::Reset));
        let adds = each(&d.counter_increment)
            .into_iter()
            .map(|c| (c, Step::Add));
        let sets = each(&d.counter_set).into_iter().map(|c| (c, Step::Set));
        for ((name, n), step) in resets.chain(adds).chain(sets) {
            // A reset replaces a sibling's; the rest take the innermost.
            let found = match step {
                Step::Reset => (self.counters.iter()).rposition(|c| c.0 == name && c.2 == depth),
                _ => self.counters.iter().rposition(|c| c.0 == name),
            };
            let full = self.counters.len() >= MAX_COUNTERS;
            match found.and_then(|at| self.counters.get_mut(at)) {
                Some(c) if step == Step::Add => c.1 = c.1.saturating_add(n),
                Some(c) => c.1 = n,
                None if full => {}
                None if step == Step::Reset => self.counters.push((name, n, depth)),
                // Never reset, a counter counts from the root.
                None => self.counters.push((name, n, 0)),
            }
        }
    }

    /// A counter's value; `list-item` is the list's own count.
    fn counter(&self, name: &str) -> i32 {
        if let Some(c) = self.counters.iter().rev().find(|c| c.0 == name) {
            return c.1;
        }
        let list = self
            .stack
            .iter()
            .rev()
            .find(|o| o.name == "ul" || o.name == "ol");
        match list.filter(|_| name == "list-item") {
            Some(list) => i32::try_from(list.counter).unwrap_or(i32::MAX),
            None => 0,
        }
    }

    /// The next quote mark, opening or closing, by how many are open.
    fn quote(&mut self, open: bool) -> String {
        let marks = self.stack.last().and_then(|o| o.computed.quotes.clone());
        let marks = marks.unwrap_or_else(generated::default_quotes);
        if !open {
            if self.quotes == 0 {
                return String::new();
            }
            self.quotes -= 1;
        }
        let depth = self.quotes.min(marks.len().saturating_sub(1));
        let pair = marks.get(depth);
        if open {
            self.quotes = self.quotes.saturating_add(1);
        }
        pair.map(|p| if open { p.0.clone() } else { p.1.clone() })
            .unwrap_or_default()
    }

    /// `content` parts as text, and the pictures after each text run.
    fn content_text(&mut self, items: &[Item]) -> (String, Vec<(usize, String)>) {
        let (mut text, mut pictures) = (String::new(), Vec::new());
        for item in items {
            match item {
                Item::Text(t) => text.push_str(t),
                Item::Attr(name) => {
                    let el = self.stack.last().map(|o| &o.el);
                    let attrs = el.into_iter().flat_map(|el| &el.attrs);
                    if let Some((_, v)) = attrs.into_iter().find(|(k, _)| k == name) {
                        text.push_str(v);
                    }
                }
                Item::Counter(name, style) => {
                    text.push_str(&generated::format(self.counter(name), style));
                }
                Item::Counters(name, joint, style) => {
                    let all: Vec<String> = (self.counters.iter())
                        .filter(|c| c.0 == *name)
                        .map(|c| generated::format(c.1, style))
                        .collect();
                    match all.is_empty() {
                        true => text.push_str(&generated::format(self.counter(name), style)),
                        false => text.push_str(&all.join(joint)),
                    }
                }
                Item::OpenQuote => text.push_str(&self.quote(true)),
                Item::CloseQuote => text.push_str(&self.quote(false)),
                Item::NoOpenQuote => self.quotes = self.quotes.saturating_add(1),
                Item::NoCloseQuote => self.quotes = self.quotes.saturating_sub(1),
                Item::Url(url) => pictures.push((text.len(), url.clone())),
            }
        }
        (text, pictures)
    }

    /// Shows a generated box styled by `d`, as the top element's child.
    fn generate(&mut self, d: Decl) {
        let Some(Generated::Items(items)) = d.content.clone() else {
            return;
        };
        if self.top().hidden || self.top().graphic {
            return;
        }
        self.count(&d, self.stack.len());
        let (text, pictures) = self.content_text(&items);
        let block = matches!(
            d.display,
            Some(Display::Block | Display::Flex | Display::Grid | Display::Table)
        );
        let edged = (d.border_style.iter()).any(|s| s.is_some_and(|s| s != BorderStyle::None));
        let painted = d.background == Some(true) || d.background_image == Some(true);
        let shows = edged || painted || d.height.is_some();
        // An empty box drawing nothing, as a clearfix, is skipped.
        if text.trim().is_empty() && pictures.is_empty() && !(block && shows) {
            return;
        }
        let name = if block { "div" } else { "span" };
        let mut d = d;
        if block {
            d.display = Some(Display::Block);
        }
        self.forced = Some(d);
        self.start(&synthetic(name));
        let mut from = 0;
        for (at, url) in pictures {
            self.read_text(text.get(from..at).unwrap_or_default());
            from = at;
            let picture = self.css_picture(&url);
            self.picture(picture, None, (None, None));
        }
        self.read_text(text.get(from..).unwrap_or_default());
        self.end(name);
    }

    /// Shows the `::after` boxes of open elements from `at`, innermost first.
    fn generate_after(&mut self, at: usize) {
        loop {
            let open = |o: &Open| o.after.is_some();
            let Some(i) = (at..self.stack.len())
                .rev()
                .find(|&i| self.stack.get(i).is_some_and(open))
            else {
                return;
            };
            let after = self.stack.get_mut(i).and_then(|o| o.after.take());
            if i + 1 < self.stack.len() {
                self.close_from(i + 1);
            }
            if let Some(d) = after {
                self.generate(*d);
            }
        }
    }

    /// Marks a short, large floated run starting the block as its drop cap.
    fn mark_initial(&mut self) {
        let Some(Content::Text(t)) = self.current.as_mut().map(|b| &mut b.content) else {
            return;
        };
        let len = t.text.len();
        let short = (1..=3).contains(&t.text.trim().chars().count()) && t.inlines.is_empty();
        let large =
            (t.spans.first()).is_some_and(|s| s.range == (0..len) && s.emphasis.size >= 150);
        if short && large && !t.text.starts_with(' ') {
            t.initial = len;
        }
    }

    fn style_for(&self, name: &str, c: &Computed) -> Style {
        let mut st = self.top();
        match name {
            "code" | "kbd" | "samp" | "tt" => st.code = true,
            "pre" | "listing" | "xmp" => st.pre = true,
            "svg" | "math" => st.graphic = true,
            "head" | "style" | "script" | "title" | "template" | "noscript" | "rp" => {
                st.hidden = true;
            }
            "rt" | "rtc" => st.note = true,
            _ => {}
        }
        st.align = c.align.align();
        st.sided = c.align.sided();
        st.unmarked = c.unmarked;
        st.hidden |= c.display == Display::None;
        st
    }

    /// `tag` as selectors see it, placed among its parent's children.
    fn element(&self, tag: &Tag, name: &str) -> El {
        let (count, types) = match self.stack.last() {
            Some(o) => (o.count, o.types.as_slice()),
            None => (self.top_count, &[][..]),
        };
        let typed = types.iter().find(|(t, _)| t == name).map_or(0, |t| t.1);
        let attrs: Vec<(String, String)> = (tag.attrs.iter())
            .take(MAX_ATTRS)
            .map(|a| {
                (
                    a.name.local.to_ascii_lowercase().to_string(),
                    a.value.to_string(),
                )
            })
            .collect();
        let value = |n: &str| attrs.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        El {
            tag: name.to_owned(),
            id: value("id"),
            classes: (value("class").unwrap_or_default())
                .split_ascii_whitespace()
                .take(MAX_CLASSES)
                .map(str::to_owned)
                .collect(),
            index: count + 1,
            type_index: typed + 1,
            attrs,
        }
    }

    /// The cascade for `el`, about to open inside the stack.
    fn declared(&self, el: &El, tag: &Tag) -> Decl {
        let inherited = match self.stack.last() {
            Some(open) => Arc::clone(&open.computed.vars),
            None => Arc::from([]),
        };
        css::declared(
            &self.sheets,
            &self.path(el),
            &hints(tag, &el.tag),
            (attr(tag, "style"), self.base),
            &inherited,
        )
    }

    /// `:root`'s custom properties, writing mode and direction, read now
    /// the head's sheets are in.
    fn root_vars(&mut self) {
        let Some(root) = self.root.take() else {
            return;
        };
        let path = [Ctx {
            el: &root,
            before: &[],
        }];
        let none: Vars = Arc::from([]);
        let d = css::declared(&self.sheets, &path, &Decl::default(), (None, ""), &none);
        let Some(open) = self.stack.first_mut().filter(|o| o.name == "html") else {
            return;
        };
        if let Some(vars) = d.vars {
            open.computed.vars = vars;
        }
        open.computed.writing = d.writing.unwrap_or(open.computed.writing);
        open.computed.dir = d.dir.unwrap_or(open.computed.dir);
    }

    /// The generated boxes rules give `el`, about to open; `d` is its own.
    fn pseudos(&self, el: &El, d: &Decl) -> Pseudos {
        let path = self.path(el);
        let vars = d.vars.clone().unwrap_or_else(|| Arc::from([]));
        let get = |which| css::declared_pseudo(&self.sheets, &path, which, &vars);
        let block = is_block(&el.tag);
        Pseudos {
            before: get(PseudoEl::Before).filter(generates),
            after: get(PseudoEl::After).filter(generates),
            letter: block.then(|| get(PseudoEl::FirstLetter)).flatten(),
            line: block.then(|| get(PseudoEl::FirstLine)).flatten(),
            marker: (el.tag == "li").then(|| get(PseudoEl::Marker)).flatten(),
        }
    }

    /// The open elements as selectors see them, then `el`.
    fn path<'s>(&'s self, el: &'s El) -> Vec<Ctx<'s>> {
        fn siblings(kids: &[El], open: bool) -> &[El] {
            match open {
                true => kids.get(..kids.len().saturating_sub(1)).unwrap_or_default(),
                false => kids,
            }
        }
        let mut path: Vec<Ctx<'_>> = Vec::with_capacity(self.stack.len() + 1);
        let mut kids = self.top.as_slice();
        for open in &self.stack {
            path.push(Ctx {
                el: &open.el,
                before: siblings(kids, true),
            });
            kids = &open.kids;
        }
        path.push(Ctx {
            el,
            before: siblings(kids, false),
        });
        path
    }

    /// Records `el` among its parent's children.
    fn adopt(&mut self, el: &El) {
        let (kids, count) = match self.stack.last_mut() {
            Some(o) => {
                match o.types.iter_mut().find(|(t, _)| *t == el.tag) {
                    Some(t) => t.1 += 1,
                    None => o.types.push((el.tag.clone(), 1)),
                }
                (&mut o.kids, &mut o.count)
            }
            None => (&mut self.top, &mut self.top_count),
        };
        if kids.len() >= MAX_SIBLINGS {
            kids.remove(0);
        }
        kids.push(el.clone());
        *count += 1;
    }

    /// Notes what makes the chapter designed rather than prose.
    fn observe(&mut self, name: &str, c: &Computed, d: &Decl) -> bool {
        let block = is_block(name) && !matches!(name, "html" | "body");
        let cell = matches!(
            name,
            "table" | "td" | "th" | "tr" | "thead" | "tbody" | "tfoot" | "caption" | "hr"
        );
        let width = match c.width {
            Len::Auto => false,
            Len::Percent(p) => p < 99.0,
            _ => true,
        };
        let strong = matches!(c.display, Display::Flex | Display::Grid)
            || (block && !cell && c.border.iter().any(|b| *b > 0.0))
            || (block && c.background)
            || (block && !cell && width)
            || matches!(d.height, Some(Len::Vh(_)));
        self.signals.strong |= strong;
        strong
    }

    /// Notes a text block's size, spacing and faces for the chapter.
    fn observe_text(&mut self, look: &Look, role: Role) {
        self.signals.faces |= look.faces.iter().any(|f| f.starts_with(BOOK_FACE));
        self.signals.letters |= look.letter >= 0.1 * look.size;
        let size = (look.size * 20.0).round() as u32 * 5;
        if role == Role::Paragraph && !self.signals.sizes.contains(&size) {
            self.signals.sizes.push(size);
        }
    }

    /// A chapter keeps a few sheets, once each, imports first.
    fn add_sheet(&mut self, sheet: Arc<Stylesheet>, depth: usize) {
        if depth < MAX_IMPORT_DEPTH {
            for path in &sheet.imports {
                if let Some(imported) = self.library.stylesheet(path) {
                    self.add_sheet(imported, depth + 1);
                }
            }
        }
        let known = self.sheets.iter().any(|s| Arc::ptr_eq(s, &sheet));
        if !known && self.sheets.len() < MAX_SHEETS {
            self.sheets.push(sheet);
            self.styled = true;
        }
    }

    fn link(&mut self, tag: &Tag) {
        let rel = |want: &str| {
            attr(tag, "rel").is_some_and(|r| {
                r.split_ascii_whitespace()
                    .any(|w| w.eq_ignore_ascii_case(want))
            })
        };
        // Alternate sheets wait to be picked; `media` must apply.
        let media = attr(tag, "media").is_none_or(query::media);
        let sheet = rel("stylesheet") && !rel("alternate") && media;
        let path = attr(tag, "href").and_then(|h| href::resolve(self.base, h));
        if let (true, Some(path)) = (sheet, path)
            && let Some(sheet) = self.library.stylesheet(&path)
        {
            self.add_sheet(sheet, 0);
        }
    }

    /// Each image is its own block; one that won't load shows its alt.
    fn image(
        &mut self,
        src: Option<&str>,
        alt: Option<&str>,
        sized: (Option<Length>, Option<&Computed>),
    ) {
        let src = src.map(str::trim);
        let picture = match src {
            Some(s) if s.starts_with("data:") => {
                let bytes = href::data_url(s);
                bytes.and_then(|b| Picture::probe(b).ok()).map(Arc::new)
            }
            _ => {
                let path = src.and_then(|s| href::resolve(self.base, s));
                path.and_then(|p| self.library.picture(&p))
            }
        };
        self.picture(picture, alt, sized);
    }

    /// Starts reading an inline drawing at its `svg` or `math` start tag.
    fn begin_drawing(&mut self, tag: &Tag) {
        let name: &str = &tag.name;
        let capture = match () {
            _ if tag.self_closing => return,
            _ if svg::is_root(name) => Caught::Svg(Box::new(Capture::new(tag))),
            _ if name.rsplit(':').next() == Some("math") => {
                Caught::Math(Box::new(math::Capture::new(tag)))
            }
            _ => return,
        };
        let open = self.stack.last().filter(|o| o.name == name);
        self.drawing = Some(Drawing {
            capture,
            name: name.to_owned(),
            computed: open.map_or_else(Computed::default, |o| o.computed.clone()),
            shown: open.is_some_and(|o| !o.style.hidden),
            alt: attr(tag, "aria-label").map(str::to_owned),
        });
    }

    /// Feeds a drawing; at its end it is shown as one picture.
    fn draw(&mut self, token: &Token) {
        let done = self.drawing.as_mut().is_some_and(|d| match &mut d.capture {
            Caught::Svg(c) => c.token(token),
            Caught::Math(m) => m.token(token),
        });
        let Some(d) = self.drawing.take_if(|_| done) else {
            return;
        };
        // Its place closes with it, but the picture still goes there.
        let places = self.places.clone();
        self.end(&d.name);
        let after = std::mem::replace(&mut self.places, places);
        let svg = match d.capture {
            Caught::Svg(c) => Some(c),
            Caught::Math(m) if d.shown => {
                self.formula(*m, &d.computed);
                None
            }
            Caught::Math(_) => None,
        };
        if let Some(c) = svg.filter(|c| d.shown && !c.broken) {
            let (width, alt) = (picture_width(&d.computed), d.alt.as_deref());
            let c = *c;
            match c.images.as_slice() {
                // A frame round one image, as covers are: that image shows.
                [only] if !c.drawn => self.image(Some(only), alt, (None, None)),
                _ => {
                    let links = svg_links(&mut *self.library, self.base, c.images);
                    let picture = Picture::svg(c.markup.into_bytes(), links).ok();
                    self.picture(picture.map(Arc::new), alt, (width, Some(&d.computed)));
                }
            }
        }
        self.places = after;
        if matches!(token, Token::EOFToken) {
            self.eof();
        }
    }

    /// Sets a `math` element: in its line, or centred as its own block.
    fn formula(&mut self, capture: math::Capture, computed: &Computed) {
        if self.top().hidden {
            return;
        }
        let broken = capture.broken;
        let Some(root) = capture.root() else {
            return;
        };
        let block = math::is_block(&root) || computed.display == Display::Block;
        let drawn = (!broken).then(|| math::render(&root, block)).flatten();
        let picture = drawn.as_ref().and_then(|f| {
            let svg = f.svg.as_bytes().to_vec();
            Picture::svg(svg, Links::new()).ok().map(Arc::new)
        });
        let (Some(f), Some(picture)) = (drawn, picture) else {
            return self.formula_alt(&root);
        };
        // A table's cells hold lines only, so display math sits in one.
        if block && self.table.is_none() {
            self.finish();
            let mut b = Block::new(Content::Image {
                picture,
                cover: false,
                width: Some(Length::Em(f.width)),
            });
            b.frame = self.frame();
            b.frame.align = Align::Center;
            self.emit(b);
            return;
        }
        let space = std::mem::take(&mut self.space);
        let Some(t) = self.target() else {
            return;
        };
        if t.inlines.len() >= MAX_INLINES {
            return self.formula_alt(&root);
        }
        if space && !t.text.is_empty() && !t.text.ends_with('\n') {
            t.push(" ", Emphasis::default());
        }
        t.inlines.push(Inline {
            at: t.text.len(),
            picture,
            width: f.width,
            height: f.height,
            rise: Rise::By(-f.depth),
        });
    }

    /// A formula that can't be drawn: its own picture, else its text.
    fn formula_alt(&mut self, root: &math::Node) {
        let alt = math::alt(root);
        if let Some(src) = root.attr("altimg") {
            let src = src.to_owned();
            return self.image(Some(&src), alt.as_deref(), (None, None));
        }
        if let (Some(text), Some(t)) = (alt, self.target()) {
            t.push(&text, Emphasis::default());
        }
    }

    /// Opens a fixed page at `<body>`: its viewport and root ground.
    fn open_sheet(&mut self, body: &Computed) {
        let sized = |l: Len| l.px(ROOT_PX, 0.0).filter(|v| *v >= 1.0);
        let own = sized(body.width).zip(sized(body.height));
        let (width, height) = self.viewport.or(own).unwrap_or(DEFAULT_SHEET);
        let root = self.stack.first().map(|o| o.computed.clone());
        let paints = |p: &(Option<f32>, Option<Arc<Backdrop>>, Option<Finish>)| {
            p.0.is_some() || p.1.is_some() || p.2.as_ref().is_some_and(Finish::shows)
        };
        let from_root = root.map(|c| self.paint(&c)).filter(paints);
        self.grounded = from_root.is_none();
        let (fill, backdrop, finish) = match from_root {
            Some(p) => p,
            None => self.paint(body),
        };
        let ground = Boxed {
            fill: fill.map(|c| c.min(MAX_TEXT_FILL)),
            backdrop,
            finish: finish.map(|f| Arc::new(capped(&f))),
            ..Boxed::default()
        };
        let shows = ground.fill.is_some() || ground.backdrop.is_some() || ground.finish.is_some();
        self.sheet = Some(Arc::new(Sheet {
            width,
            height,
            ground: shows.then(|| Arc::new(ground)),
        }));
        self.places.push(Place {
            depth: 0,
            rect: [0.0, 0.0, width, height],
            spot: None,
        });
    }

    /// Opens the containing block `position` makes; true when out of flow.
    fn place_start(&mut self, c: &Computed) -> bool {
        let Some(sheet) = self.sheet.clone().filter(|_| self.fixed) else {
            return false;
        };
        let Some(outer) = self.places.last().cloned() else {
            return false;
        };
        if c.placing == Placing::Static || self.table.is_some() {
            return false;
        }
        let view = (sheet.width, sheet.height);
        let [cx, cy, cw, ch] = match c.placing {
            Placing::Fixed => [0.0, 0.0, view.0, view.1],
            _ => outer.rect,
        };
        let at = |l: Len, base: f32| placed_px(l, base, view);
        let side = |i: usize, base: f32| c.offsets.get(i).and_then(|l| at(*l, base));
        let (top, right, bottom, left) = (side(0, ch), side(1, cw), side(2, ch), side(3, cw));
        let depth = self.stack.len();
        if c.placing == Placing::Relative {
            // In the flow still: only what it holds is placed from it.
            let x = cx + left.or(right.map(|r| -r)).unwrap_or(0.0);
            let y = cy + top.or(bottom.map(|b| -b)).unwrap_or(0.0);
            let (w, h) = (
                at(c.width, cw).unwrap_or(cw),
                at(c.height, ch).unwrap_or(ch),
            );
            let spot = outer.spot;
            self.places.push(Place {
                depth,
                rect: [x, y, w, h],
                spot,
            });
            return false;
        }
        let margin =
            |i: usize, base: f32| c.margin.get(i).and_then(|m| at(*m, base)).unwrap_or(0.0);
        let (m_top, m_right, m_bottom, m_left) =
            (margin(0, cw), margin(1, cw), margin(2, cw), margin(3, cw));
        let across = c.padding[1] + c.padding[3] + c.border[1] + c.border[3];
        let down = c.padding[0] + c.padding[2] + c.border[0] + c.border[2];
        let room = |l: Option<f32>, r: Option<f32>| {
            cw - l.unwrap_or(0.0) - r.unwrap_or(0.0) - m_left - m_right
        };
        let width = at(c.width, cw)
            .map_or_else(|| room(left, right), |w| w + across)
            .max(1.0);
        let x = match (left, right) {
            (Some(l), _) => cx + l + m_left,
            (None, Some(r)) => cx + cw - r - m_right - width,
            (None, None) => cx + m_left,
        };
        let height = at(c.height, ch).map(|h| h + down);
        let height = height.or_else(|| top.zip(bottom).map(|(t, b)| ch - t - b - m_top - m_bottom));
        let height = height.map(|h| h.max(1.0));
        let (y, hung) = match (top, bottom, height) {
            (Some(t), _, _) => (cy + t + m_top, None),
            (None, Some(b), Some(h)) => (cy + ch - b - m_bottom - h, None),
            (None, Some(b), None) => (cy, Some(cy + ch - b - m_bottom)),
            (None, None, _) => (cy + m_top, None),
        };
        let spot = Spot {
            x,
            y,
            width,
            height,
            bottom: hung,
            z: c.z_index,
            depth: u8::try_from(self.boxes.len()).unwrap_or(u8::MAX),
        };
        // What came before stays in the flow; this starts its own blocks.
        self.finish();
        self.gap = Gap::default();
        self.places.push(Place {
            depth,
            rect: [x, y, width, height.unwrap_or(ch)],
            spot: Some(Arc::new(spot)),
        });
        true
    }

    /// Closes the containing blocks of elements at depth `at` and in.
    fn place_end(&mut self, at: usize) {
        let keep = self
            .places
            .iter()
            .take_while(|p| p.depth < at || p.depth == 0);
        let keep = keep.count();
        self.places.truncate(keep);
    }

    /// Shows `picture` in a line or as its own block, or else its alt.
    fn picture(
        &mut self,
        picture: Option<Arc<Picture>>,
        alt: Option<&str>,
        sized: (Option<Length>, Option<&Computed>),
    ) {
        let (width, computed) = sized;
        if self.top().hidden {
            return;
        }
        let alt = alt.map(str::trim).filter(|a| !a.is_empty());
        if self.table.is_some() {
            if let (Some(alt), Some(cell)) = (alt, self.target()) {
                cell.push(alt, Emphasis::default());
            }
            return;
        }
        let small = picture
            .as_ref()
            .zip(computed)
            .and_then(|(p, c)| inline_size(p, c));
        let after_text = self.current.as_ref().is_some_and(|b| match &b.content {
            Content::Text(t) => !t.text.trim().is_empty(),
            _ => false,
        });
        let parent = self.stack.last().map(|o| o.name.as_str());
        let small = small.filter(|_| after_text || parent.is_some_and(in_line));
        let depth = self.stack.len();
        let own = |p: &Place| p.depth + 1 >= depth && p.spot.is_some();
        let small = small.filter(|_| !self.places.last().is_some_and(own));
        if let (Some(picture), Some((width, height))) = (&picture, small) {
            // A small picture is a glyph of its line.
            let space = std::mem::take(&mut self.space);
            if let Some(t) = self.target() {
                let spaced = !t.text.is_empty() && !t.text.ends_with('\n');
                if space && spaced {
                    t.push(" ", Emphasis::default());
                }
                let at = t.text.len();
                if t.inlines.len() < MAX_INLINES {
                    let picture = Arc::clone(picture);
                    let rise = computed.map_or_else(Default::default, |c| c.rise);
                    t.inlines.push(Inline {
                        at,
                        picture,
                        width,
                        height,
                        rise,
                    });
                }
                return;
            }
        }
        self.finish();
        if let Some(picture) = picture {
            let mut block = Block::new(Content::Image {
                picture,
                cover: false,
                width,
            });
            block.frame = self.frame();
            // A grid's portrait sits over its centred name, read in a row.
            if self.laying > 0 && self.top().align == Align::Start {
                block.frame.align = Align::Center;
            }
            // A column stack's picture takes the place it sets for items.
            let stack = self.stack.last().filter(|o| o.part == Part::Stack);
            match stack.and_then(|o| o.computed.align_items) {
                Some(VAlign::Middle) => block.frame.align = Align::Center,
                Some(VAlign::Bottom) => block.frame.align = Align::End,
                _ => {}
            }
            self.emit(block);
        } else if let Some(alt) = alt {
            let italic = Emphasis {
                italic: true,
                ..Emphasis::default()
            };
            if let Some(t) = self.target() {
                t.push(alt, italic);
            }
            self.finish();
        }
    }

    fn start_table(&mut self, tag_name: &str, tag: &Tag) {
        if tag_name == "table" {
            let ordinal = self.tables;
            self.tables += 1;
            // A table that only lays things out reads as plain blocks.
            let scanned = self.layouts.get(ordinal).copied().unwrap_or(false);
            let layout = scanned || typed(tag, "role", &["presentation", "none"]);
            if let Some(t) = self.table.as_mut() {
                t.nested += 1;
            } else if layout {
                self.laying += usize::from(!tag.self_closing);
                self.signals.strong = true;
            } else {
                let frame = self.frame();
                self.table = Some(TableBuild {
                    table: Table::default(),
                    frame,
                    caption: Text::new(Role::Paragraph),
                    nested: 0,
                    in_cell: false,
                    in_caption: false,
                    in_head: false,
                });
            }
            return;
        }
        let Some(t) = self.table.as_mut() else {
            return;
        };
        if t.nested > 0 {
            if matches!(tag_name, "td" | "th")
                && let Some(cell) = t.cell()
            {
                cell.push(" ", Emphasis::default());
            }
            return;
        }
        match tag_name {
            "thead" => t.in_head = true,
            "tbody" | "tfoot" => t.in_head = false,
            "caption" => t.in_caption = !tag.self_closing,
            "tr" => t.table.rows.push(Row {
                cells: Vec::new(),
                head: t.in_head,
            }),
            "td" | "th" => {
                if t.table.rows.is_empty() {
                    t.table.rows.push(Row {
                        cells: Vec::new(),
                        head: t.in_head,
                    });
                }
                let in_head = t.in_head;
                if let Some(row) = t.table.rows.last_mut() {
                    let head = in_head || tag_name == "th";
                    row.head = if row.cells.is_empty() {
                        head
                    } else {
                        row.head && head
                    };
                    if row.cells.len() < MAX_COLUMNS {
                        row.cells.push(Text::new(Role::Paragraph));
                    }
                }
                t.in_cell = true;
            }
            _ => {}
        }
    }

    fn end_table(&mut self, name: &str) {
        let Some(t) = self.table.as_mut() else {
            if name == "table" {
                self.laying = self.laying.saturating_sub(1);
            }
            return;
        };
        match name {
            "table" if t.nested > 0 => t.nested -= 1,
            "table" => self.flush_table(),
            "td" | "th" if t.nested == 0 => t.in_cell = false,
            "caption" => t.in_caption = false,
            "thead" => t.in_head = false,
            _ => {}
        }
    }

    /// The caption, centred, then the table; empty ones vanish.
    fn flush_table(&mut self) {
        let Some(t) = self.table.take() else {
            return;
        };
        let mut caption = t.caption;
        let mut frame = t.frame;
        caption.trim_end();
        if !caption.is_blank() {
            let mut block = Block::new(Content::Text(caption));
            block.frame = Frame {
                align: Align::Center,
                ..frame.clone()
            };
            self.emit(block);
            frame.before = 0.0;
        }
        let mut table = t.table;
        for cell in table.rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
            cell.trim_end();
        }
        drop_blank_columns(&mut table);
        table.rows.retain(|r| !r.cells.is_empty());
        let mut block = Block::new(Content::Table(table));
        block.frame = frame;
        if !block.is_blank() {
            self.emit(block);
        }
    }

    /// Returns whether the tokenizer should read raw text next.
    fn start(&mut self, tag: &Tag) -> bool {
        let name: &str = &tag.name;
        let forced = self.forced.take();
        let generated = forced.is_some();
        if !generated {
            self.close_implied(name);
        }
        let el = self.element(tag, name);
        match name {
            "html" if self.stack.is_empty() => self.root = Some(el.clone()),
            "body" => self.root_vars(),
            "meta" if self.fixed && attr(tag, "name").is_some_and(|n| n.trim() == "viewport") => {
                self.viewport = attr(tag, "content").and_then(viewport).or(self.viewport);
            }
            _ => {}
        }
        let quiet = self.top().hidden || self.top().graphic;
        let (declared, pseudos) = match forced {
            Some(d) => (d, Pseudos::default()),
            None if quiet => (self.declared(&el, tag), Pseudos::default()),
            None => {
                let declared = self.declared(&el, tag);
                let pseudos = self.pseudos(&el, &declared);
                (declared, pseudos)
            }
        };
        self.styled |= attr(tag, "style").is_some();
        let computed = match self.stack.last() {
            Some(open) => open.computed.child(&declared),
            None => Computed::default().child(&declared),
        };
        if !generated {
            self.adopt(&el);
        }
        let shown = !self.top().hidden && computed.display != Display::None;
        if shown && !generated {
            self.count(&declared, self.stack.len());
        }
        let (before, after) = match shown {
            true => (pseudos.before, pseudos.after),
            false => (None, None),
        };
        if name == "hr" && (before.is_some() || after.is_some()) && self.table.is_none() {
            self.ornament(declared, (before, after));
            return false;
        }
        if name == "hr" && shown && self.table.is_none() && styled_rule(&declared) {
            self.rule_box(declared);
            return false;
        }
        let signals = shown && self.observe(name, &computed, &declared);
        if matches!(name, "html" | "body") {
            self.page = computed.fill.unwrap_or(self.page);
            self.book_ink = computed.ink.unwrap_or(self.book_ink);
            // Only the chapter's root turns it; inner boxes stay level.
            self.turned = computed.writing && !self.fixed;
        }
        if name == "body" && self.fixed && self.sheet.is_none() {
            self.open_sheet(&computed);
        }
        let placed = shown && self.place_start(&computed);
        if computed.combine && self.combining.is_none() && !is_block(name) {
            self.combining = Some((self.stack.len(), self.text_len()));
        }
        let id = attr(tag, "id").or_else(|| attr(tag, "name").filter(|_| name == "a"));
        if let Some(id) = id.filter(|_| !is_block(name)) {
            self.mark(id);
        }
        match name {
            "ruby" => self.ruby = Some((self.text_len(), None)),
            "rt" | "rtc" if !self.top().note => {
                let end = self.text_len();
                if let Some((_, base)) = &mut self.ruby {
                    *base = Some(end);
                }
                self.note.clear();
            }
            _ => {}
        }
        match name {
            "br" => {
                self.line_break();
                return false;
            }
            "hr" => {
                self.boundary(name);
                if self.table.is_none() && shown {
                    self.space_above(&computed);
                    let mut rule = Block::new(Content::Rule);
                    rule.frame = self.frame();
                    self.emit(rule);
                    self.space_below(&computed);
                }
                return false;
            }
            "img" => {
                let width = picture_width(&computed);
                let alt = attr(tag, "alt");
                self.image(attr(tag, "src"), alt, (width, Some(&computed)));
                self.place_end(self.stack.len());
                return false;
            }
            "image" => {
                let href = attr(tag, "href").or_else(|| attr(tag, "xlink:href"));
                self.image(href, None, (None, None));
            }
            "link" => {
                self.link(tag);
                return false;
            }
            _ => {}
        }
        let mut extent = None;
        if is_block(name) {
            self.boundary(name);
            if let Some(id) = id {
                self.mark(id);
                let key = format!("{}#{}", self.base, id.trim());
                extent = Some((key, self.first + self.blocks.len(), is_note(tag)));
            }
        }
        let (mut boxed, mut part) = (false, Part::None);
        if is_block(name) && shown && computed.break_before && self.table.is_none() {
            self.chapter = true;
        }
        let floats = computed.float != Float::None && is_block(name) && shown;
        let floated = floats && self.float.is_none() && self.table.is_none() && !tag.self_closing;
        if floated {
            let depth = u8::try_from(self.boxes.len()).unwrap_or(u8::MAX);
            let right = computed.float == Float::Right;
            self.float = Some((self.stack.len(), Arc::new(Floated { right, depth })));
        }
        if is_block(name) && shown && self.table.is_none() {
            part = self.grid_start(name, tag, &computed);
            // A positioned box's place is its own: no margins, no width.
            let own = placed.then(|| Computed {
                width: Len::Auto,
                margin: [Len::Px(0.0); 4],
                ..computed.clone()
            });
            let c = own.as_ref().unwrap_or(&computed);
            boxed = self.open_block(tag, (c, &declared), part);
        }
        if matches!(
            name,
            "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" | "caption"
        ) {
            self.start_table(name, tag);
            if matches!(name, "table" | "td" | "th") {
                self.rule(&computed);
            }
        }
        let empty = tag.self_closing && before.is_none() && after.is_none();
        if is_void(name) || empty || self.stack.len() >= MAX_DEPTH {
            if shown && is_block(name) && self.table.is_none() {
                self.finish();
                match boxed {
                    true => self.close_box(&computed),
                    false => self.space_below(&computed),
                }
                self.end_part(part);
            }
            self.place_end(self.stack.len());
            return false;
        }
        if matches!(name, "ul" | "ol") && !self.in_list() {
            self.list_opened = true;
        }
        let counter = attr(tag, "start")
            .and_then(|s| s.trim().parse::<u32>().ok())
            .map_or(0, |s| s.saturating_sub(1));
        let mut style = self.style_for(name, &computed);
        if name == "li" {
            self.list_marker(tag, (&computed, &style), pseudos.marker);
        }
        let floated_inline = !is_block(name) && computed.float != Float::None;
        if floated_inline && shown && self.table.is_none() && self.drop.is_none() && self.fresh() {
            self.drop = Some(self.stack.len());
        }
        if is_block(name) && shown {
            let depth = self.stack.len();
            if let Some(d) = pseudos.letter {
                self.initial = Some((depth, d));
            }
            if let Some(d) = pseudos.line {
                self.first_line = Some((depth, d));
            }
        }
        if let Some(href) = attr(tag, "href").filter(|_| name == "a") {
            style.link = self.link_to(href);
            if let Some(i) = style.link.filter(|_| is_noteref(tag)) {
                self.refs.noterefs.insert(i);
            }
        }
        self.pre_opened = name == "pre";
        self.stack.push(Open {
            name: name.to_owned(),
            style,
            computed,
            el,
            kids: Vec::new(),
            count: 0,
            types: Vec::new(),
            counter,
            extent,
            boxed,
            part,
            floated,
            signals,
            after: after.map(Box::new),
        });
        if name == "style" {
            self.style_text.clear();
            self.style_applies = attr(tag, "media").is_none_or(query::media);
        }
        if let Some(d) = before {
            self.generate(d);
        }
        if tag.self_closing {
            self.end(name);
            return false;
        }
        is_raw(name)
    }

    /// An `<hr>` showing generated content: a centred ornament, no line.
    fn ornament(&mut self, mut d: Decl, (before, after): (Option<Decl>, Option<Decl>)) {
        d.border_style = [Some(BorderStyle::None); 4];
        d.display = Some(Display::Block);
        self.forced = Some(d);
        self.start(&synthetic("div"));
        for d in [before, after].into_iter().flatten() {
            self.generate(d);
        }
        self.end("div");
    }

    /// An `<hr>` the book draws itself: a box of its borders and ground.
    fn rule_box(&mut self, mut d: Decl) {
        d.display = Some(Display::Block);
        self.forced = Some(d);
        self.rule_next = true;
        self.start(&synthetic("div"));
        self.end("div");
        self.rule_next = false;
    }

    fn end(&mut self, name: &str) {
        if name == "style" && self.stack.last().is_some_and(|o| o.name == "style") {
            let text = std::mem::take(&mut self.style_text);
            if self.style_applies {
                let sheet = Stylesheet::parse(&text, self.base);
                self.add_sheet(Arc::new(sheet), 0);
            }
        }
        if let Some(at) = self.stack.iter().rposition(|o| o.name == name) {
            self.generate_after(at);
        }
        if is_block(name) {
            self.boundary(name);
        }
        self.end_table(name);
        // Close the innermost match and anything left open in it.
        if let Some(at) = self.stack.iter().rposition(|o| o.name == name) {
            self.close_from(at);
        }
        if matches!(name, "ul" | "ol") && !self.in_list() {
            self.list_opened = false;
        }
        match name {
            "rt" | "rtc" if !self.top().note => self.annotate(),
            "ruby" => self.ruby = None,
            _ => {}
        }
        if let Some((_, from)) = self.combining.filter(|c| self.stack.len() <= c.0) {
            self.combining = None;
            self.combine(from);
        }
    }

    /// Sets the text read since `from` across the vertical line.
    fn combine(&mut self, from: usize) {
        let Some(t) = self.target() else {
            return;
        };
        let run = t.text.get(from..).unwrap_or_default();
        let lead = run.len() - run.trim_start().len();
        let range = from + lead..from + run.trim_end().len();
        let apart = t.combine.last().is_none_or(|r| r.end <= range.start);
        if !range.is_empty() && apart && t.combine.len() < MAX_COMBINED {
            t.combine.push(range);
        }
    }

    /// Pops the open elements from `at`, ending their extents.
    fn close_from(&mut self, at: usize) {
        self.generate_after(at);
        if self.drop.is_some_and(|d| d >= at) {
            self.drop = None;
            self.mark_initial();
        }
        let ends = |o: &Open| o.extent.is_some() || o.boxed || o.part != Part::None;
        let spot = |p: &Place| p.depth >= at && p.spot.is_some();
        let placed = self.places.last().is_some_and(spot);
        if (placed || self.stack.iter().skip(at).any(ends)) && self.table.is_none() {
            self.finish();
        }
        let end = self.first + self.blocks.len();
        let closed: Vec<Open> = self.stack.drain(at..).collect();
        self.counters.retain(|c| c.2 <= at);
        if self.initial.as_ref().is_some_and(|i| i.0 >= at) {
            self.initial = None;
        }
        if self.first_line.as_ref().is_some_and(|i| i.0 >= at) {
            self.first_line = None;
        }
        for open in closed.iter().rev() {
            if is_block(&open.name) && !open.style.hidden && self.table.is_none() {
                match open.boxed {
                    true => self.close_box(&open.computed),
                    false => self.space_below(&open.computed),
                }
                self.end_part(open.part);
                if open.computed.break_after {
                    self.chapter = true;
                }
            }
            if open.floated {
                self.float = None;
            }
        }
        self.place_end(at);
        for (key, start, note) in closed.into_iter().filter_map(|o| o.extent) {
            if end > start && self.refs.extents.len() < MAX_PLACES {
                let blocks = start..end;
                self.refs.extents.insert(key, Extent { blocks, note });
            }
        }
    }

    /// Ends an element's part in the layout grid.
    fn end_part(&mut self, part: Part) {
        match part {
            Part::None => {}
            Part::Cell => self.end_cell(),
            Part::Row => self.end_row(),
            Part::Table => {
                self.end_row();
                self.grid = None;
            }
            // A flex row let go of has nothing left to end.
            _ if self.grid.as_ref().is_none_or(|g| g.flex.is_none()) => {}
            Part::FlexItem => self.end_cell(),
            Part::FlexBox => {
                self.end_row();
                self.grid = None;
            }
            Part::Stack => {}
            // The space below the item parts it from the next.
            Part::StackItem => {
                let below = self.gap;
                self.end_cell();
                self.end_row();
                (self.grid, self.gap) = (None, below);
            }
        }
    }

    /// A block's top margin, then its border and padding, above what follows.
    fn space_above(&mut self, c: &Computed) {
        self.gap.margin(c.margin_px(0));
        let inside = c.border[0] + c.padding[0];
        if inside > 0.0 {
            self.gap.fixed(inside);
        }
    }

    fn space_below(&mut self, c: &Computed) {
        let inside = c.border[2] + c.padding[2];
        if inside > 0.0 {
            self.gap.fixed(inside);
        }
        self.gap.margin(c.margin_px(2));
    }

    /// Marks every look of the chapter as designed.
    fn design(&mut self) {
        // Keeping the old looks keeps their addresses unique.
        let old = std::mem::take(&mut self.looks);
        let new: HashMap<usize, Arc<Look>> = (old.iter())
            .map(|l| {
                let look = Look {
                    designed: true,
                    ..(**l).clone()
                };
                (Arc::as_ptr(l) as usize, Arc::new(look))
            })
            .collect();
        for block in &mut self.blocks {
            if let Some(look) = &mut block.frame.look {
                match new.get(&(Arc::as_ptr(look) as usize)) {
                    Some(designed) => *look = Arc::clone(designed),
                    None => Arc::make_mut(look).designed = true,
                }
            }
        }
    }

    fn eof(&mut self) {
        self.finish();
        self.flush_table();
        self.close_from(0);
        if self.signals.designed() {
            self.design();
        } else if !self.styled {
            // Unstyled, as converted text is, it reads in the reader's type.
            for block in &mut self.blocks {
                block.frame.look = None;
            }
        }
        // Ids after the last block mark where the next chapter starts.
        let at = Anchor {
            block: self.first + self.blocks.len(),
            offset: 0,
        };
        for key in self.pending.drain(..) {
            self.refs.places.entry(key).or_insert(at);
        }
    }
}

/// A colour's background, the book's text colour and its page colour.
type Grounds = (css::Rgb, css::Rgb, css::Rgb);

/// How far a book's text colour may fade toward the paper, staying legible.
const MAX_FADE: f32 = 0.55;
/// Borders may stay fainter: a book's light rules are meant quiet.
const MAX_BORDER_FADE: f32 = 0.85;
/// Below this gap between the book's text and page, its colours mean little.
const MIN_SPAN: f32 = 0.3;

/// `ink` on the page's greys, faded by how far its contrast falls short.
fn drawn(ink: css::Rgb, (ground, base, page): Grounds, most: f32) -> Option<css::Rgb> {
    let unit = |c: css::Rgb| c.map(|v| f32::from(v) / 255.0);
    let luma = |[r, g, b]: [f32; 3]| 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let span = (luma(unit(base)) - luma(unit(page))).abs();
    if span < MIN_SPAN {
        return None;
    }
    let colour = unit(ink);
    let grey = luma(colour);
    let contrast = (grey - luma(unit(ground))).abs() / span;
    let off = colour.map(|v| v - grey);
    // Never darker than the hue needs: black holds none.
    let need = off.iter().fold(0.0f32, |n, &o| n.max(-o));
    let fade = (1.0 - contrast).max(need).clamp(0.0, most);
    if fade < 0.03 && off.iter().all(|o| o.abs() < 0.04) {
        return None;
    }
    // The most of the hue that fits around this grey.
    let room = off.iter().fold(1.0f32, |k, &o| match o {
        o if o > 1e-4 => k.min((1.0 - fade) / o),
        o if o < -1e-4 => k.min(fade / -o),
        _ => k,
    });
    Some(off.map(|o| ((fade + o * room) * 255.0).round().clamp(0.0, 255.0) as u8))
}

/// The box `name` draws or sits in; a cell's column sets its width.
fn boxed(name: &str, c: &Computed, d: &Decl, cell: bool) -> Option<Boxed> {
    if matches!(name, "tr" | "thead" | "tbody" | "tfoot" | "caption" | "hr") {
        return None;
    }
    let mut edges = c.border_style.map(edge);
    for (e, clear) in edges.iter_mut().zip(c.border_clear) {
        if clear && *e != Edge::None {
            *e = Edge::Clear;
        }
    }
    let mut border = c.border.map(|w| w.max(0.0) / ROOT_PX);
    for (w, e) in border.iter_mut().zip(&edges) {
        if *e == Edge::None {
            *w = 0.0;
        }
    }
    let side = |i: usize| match c.margin.get(i).copied().unwrap_or(Len::Auto) {
        Len::Auto => None,
        Len::Percent(p) | Len::Vw(p) => Some(Length::Percent(p)),
        Len::Mix { px, pct, .. } => Some(Length::Mix {
            em: px / ROOT_PX,
            percent: pct,
        }),
        _ => Some(Length::Em(c.margin_px(i) / ROOT_PX)),
    };
    let zero = Some(Length::Em(0.0));
    let margin = if cell { [zero; 2] } else { [side(3), side(1)] };
    let width = match c.width {
        _ if cell => None,
        Len::Px(px) if px > 0.0 => Some(Length::Em(px / ROOT_PX)),
        Len::Percent(p) | Len::Vw(p) if p > 0.0 => Some(Length::Percent(p)),
        Len::Mix { px, pct, .. } => mixed(px, pct),
        _ => None,
    };
    let flex = matches!(c.display, Display::Flex | Display::Grid);
    let centre = match d.height {
        Some(Len::Vh(v)) if flex && v > 0.0 => Some((v / 100.0).min(1.0)),
        _ => None,
    };
    let padding = c.padding.map(|p| p.max(0.0) / ROOT_PX);
    let flat = margin.iter().all(|m| m.is_none() || *m == zero);
    let drawn = border.iter().any(|w| *w > 0.0);
    // A share of the screen is the page's, not a length to scale.
    let height = match c.height {
        _ if matches!(d.height, Some(Len::Vh(_))) => None,
        Len::Px(px) if px > 0.0 && !cell => Some(px / ROOT_PX),
        _ => None,
    };
    let apart = padding[1] > 0.0 || padding[3] > 0.0 || width.is_some() || centre.is_some();
    (drawn || apart || !flat).then_some(Boxed {
        margin,
        border,
        edges,
        padding,
        width,
        centre,
        height,
        radius: radius(c),
        ..Boxed::default()
    })
}

/// A border style as the page draws it.
fn edge(s: BorderStyle) -> Edge {
    match s {
        BorderStyle::None => Edge::None,
        BorderStyle::Solid => Edge::Solid,
        BorderStyle::Double => Edge::Double,
        BorderStyle::Dashed => Edge::Dashed,
        BorderStyle::Dotted => Edge::Dotted,
        BorderStyle::Groove => Edge::Groove,
        BorderStyle::Ridge => Edge::Ridge,
        BorderStyle::Inset => Edge::Inset,
        BorderStyle::Outset => Edge::Outset,
    }
}

/// A finish with its gradients no stronger than text can sit on.
fn capped(f: &Finish) -> Finish {
    let mut f = f.clone();
    for layer in &mut f.layers {
        if let mareader_reflow::Layer::Gradient(g) = layer {
            for stop in &mut g.stops {
                stop.1 = stop.1.min(MAX_TEXT_FILL);
            }
        }
    }
    f
}

/// Whether the book draws an `<hr>` itself: borders, a ground, a picture.
fn styled_rule(d: &Decl) -> bool {
    let bordered = (d.border_style.iter().zip(&d.border_width)).any(|(s, w)| {
        let thin = matches!(w, Some(Len::Px(px)) if *px <= 0.0);
        matches!(s, Some(s) if *s != BorderStyle::None) && !thin
    });
    let shadowed = d.shadows.as_ref().is_some_and(|s| !s.is_empty());
    let layered = d.layers.as_ref().is_some_and(|l| !l.is_empty());
    bordered
        || shadowed
        || layered
        || d.background == Some(true)
        || d.background_image == Some(true)
        || d.frame.as_ref().is_some_and(Option::is_some)
}

/// A box's corner radius as the book sets it.
fn radius(c: &Computed) -> Option<Length> {
    match c.radius {
        Len::Px(px) if px > 0.0 => Some(Length::Em(px / ROOT_PX)),
        Len::Percent(p) if p > 0.0 => Some(Length::Percent(p.min(50.0))),
        Len::Mix { px, pct, .. } => mixed(px, pct.min(50.0)),
        _ => None,
    }
}

/// A box drawn only for its background: its width, padding and height.
fn bare(c: &Computed) -> Boxed {
    let width = match c.width {
        Len::Px(px) if px > 0.0 => Some(Length::Em(px / ROOT_PX)),
        Len::Percent(p) | Len::Vw(p) if p > 0.0 => Some(Length::Percent(p)),
        Len::Mix { px, pct, .. } => mixed(px, pct),
        _ => None,
    };
    let height = match c.height {
        Len::Px(px) if px > 0.0 => Some(px / ROOT_PX),
        _ => None,
    };
    Boxed {
        padding: c.padding.map(|p| p.max(0.0) / ROOT_PX),
        width,
        height,
        radius: radius(c),
        ..Boxed::default()
    }
}

/// Whether a small picture in `parent` is set in a line of text.
fn in_line(parent: &str) -> bool {
    let heading = parent.len() == 2 && parent.starts_with('h') && parent != "hr";
    heading
        || matches!(
            parent,
            "p" | "span"
                | "a"
                | "li"
                | "dt"
                | "dd"
                | "em"
                | "strong"
                | "b"
                | "i"
                | "u"
                | "sup"
                | "sub"
                | "small"
                | "big"
                | "q"
                | "cite"
                | "label"
                | "ruby"
        )
}

/// A small picture's size in ems of its text, to sit in a line.
fn inline_size(picture: &Picture, c: &Computed) -> Option<(f32, f32)> {
    if matches!(c.width, Len::Percent(_) | Len::Vw(_) | Len::Mix { .. }) {
        return None;
    }
    let (pw, ph) = (picture.width as f32, picture.height as f32);
    let size = c.size.max(1.0);
    let set = |l: Len| match l {
        Len::Px(px) if px > 0.0 => Some(px),
        _ => None,
    };
    if pw <= 0.0 || ph <= 0.0 {
        return None;
    }
    let (w, h) = match (set(c.width), set(c.height)) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * ph / pw),
        (None, Some(h)) => (h * pw / ph, h),
        (None, None) => (pw, ph),
    };
    let fits = h <= INLINE_TALL * size && w <= INLINE_WIDE * size;
    fits.then(|| (w / size, h / size))
}

/// A grid cell's width as the book sets it.
fn cell_width(c: &Computed) -> Option<Length> {
    match c.width {
        Len::Px(px) if px > 0.0 => Some(Length::Em(px / ROOT_PX)),
        Len::Percent(p) | Len::Vw(p) if p > 0.0 => Some(Length::Percent(p)),
        Len::Mix { px, pct, .. } => mixed(px, pct),
        _ => None,
    }
}

impl RowBuild {
    fn new(first: usize) -> Self {
        Self {
            widths: Vec::new(),
            valign: Vec::new(),
            before: 0.0,
            placeholder: Arc::new(GridRow::default()),
            first,
            column: None,
            boxes: 0,
            starts: Vec::new(),
            spans: Vec::new(),
            grow: Vec::new(),
            down: Vec::new(),
            next: 0,
        }
    }
}

/// An image's width as the book sets it, in reader ems or percent.
fn picture_width(c: &Computed) -> Option<Length> {
    match c.width {
        Len::Px(px) if px > 0.0 => Some(Length::Em((px / ROOT_PX).min(200.0))),
        Len::Percent(p) | Len::Vw(p) if p > 0.0 => Some(Length::Percent(p.min(100.0))),
        Len::Mix { px, pct, .. } => mixed(px, pct.min(100.0)),
        _ => None,
    }
}

/// A computed sum as ems and a column share, if it comes out wide.
fn mixed(px: f32, pct: f32) -> Option<Length> {
    let rough = px + pct * css::BASE_WIDTH / 100.0;
    (rough > 0.0).then_some(Length::Mix {
        em: px / ROOT_PX,
        percent: pct,
    })
}

/// What HTML attributes say about style, beneath every sheet.
fn hints(tag: &Tag, name: &str) -> Decl {
    let mut d = Decl::default();
    if let Some(a) = attr(tag, "align") {
        d = Decl::parse(&format!("text-align: {a}"));
    }
    let sized = matches!(
        name,
        "img" | "table" | "td" | "th" | "col" | "image" | "svg"
    );
    if let Some(w) = attr(tag, "width").filter(|_| sized) {
        let w = w.trim();
        let unit = if w.ends_with('%') { "" } else { "px" };
        d.width = Decl::parse(&format!("width: {w}{unit}")).width;
    }
    let ink = match name {
        "font" => attr(tag, "color"),
        "body" => attr(tag, "text"),
        _ => None,
    };
    if let Some(c) = ink {
        d.ink = css::colour(&c.trim().to_ascii_lowercase()).flatten();
    }
    if let Some(dir) = attr(tag, "dir") {
        d.dir = match dir.trim().to_ascii_lowercase().as_str() {
            "rtl" => Some(Dir::Rtl),
            "ltr" => Some(Dir::Ltr),
            "auto" => Some(Dir::Auto),
            _ => None,
        };
    }
    if let Some(c) = attr(tag, "bgcolor") {
        d.background = Some(true);
        d.fill = css::colour(&c.trim().to_ascii_lowercase());
    }
    if let Some(t) = attr(tag, "type").filter(|_| matches!(name, "ol" | "ul" | "li")) {
        // The letters' case matters here, unlike in CSS.
        let word = match t.trim() {
            "1" => "decimal",
            "a" => "lower-alpha",
            "A" => "upper-alpha",
            "i" => "lower-roman",
            "I" => "upper-roman",
            other => other,
        };
        d.list_style = generated::list_style(word).or(d.list_style);
    }
    if let Some(v) = attr(tag, "valign").filter(|_| matches!(name, "td" | "th" | "tr")) {
        d.valign = Decl::parse(&format!("vertical-align: {}", v.trim())).valign;
    }
    d
}

/// `e` with family `names`, registered with `t`; too many keep the block's.
fn faced(t: &mut Text, names: Option<Vec<String>>, e: Emphasis) -> Emphasis {
    let Some(names) = names else {
        return e;
    };
    let at = match t.faces.iter().position(|f| *f == names) {
        Some(at) => at,
        None if t.faces.len() < usize::from(u8::MAX) => {
            t.faces.push(names);
            t.faces.len() - 1
        }
        None => return e,
    };
    let face = u8::try_from(at + 1).unwrap_or(0);
    Emphasis { face, ..e }
}

/// Drops columns blank in every row, such as spacers.
fn drop_blank_columns(table: &mut Table) {
    let columns = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
    let filled = |i: usize| {
        let cell = |r: &Row| r.cells.get(i).is_some_and(|c| !c.is_blank());
        table.rows.iter().any(cell)
    };
    let keep: Vec<bool> = (0..columns).map(filled).collect();
    if !keep.contains(&true) {
        return;
    }
    for row in &mut table.rows {
        let mut kept = keep.iter();
        row.cells.retain(|_| kept.next().copied().unwrap_or(true));
    }
}

/// One table as the scan sees it.
struct Scanned {
    /// Its place among the chapter's tables.
    at: usize,
    pictured: bool,
    /// Head cells or a caption: a table of data.
    data: bool,
    /// Paragraphs or divisions inside its cells.
    blocks: bool,
    /// Each row's cells: whether each holds anything.
    rows: Vec<Vec<bool>>,
    in_cell: bool,
}

impl Scanned {
    /// Pictures; or, with no heads, block cells beside a spacer column.
    fn layout(&self) -> bool {
        let columns = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        let filled = |i: usize| self.rows.iter().any(|r| r.get(i) == Some(&true));
        let full = (0..columns).filter(|&i| filled(i)).count();
        let spacer = full >= 2 && full < columns;
        // A single column of blocks is a box, as a framed note.
        let boxed = columns == 1 && self.rows.len() <= MAX_BOX_ROWS;
        self.pictured || (!self.data && self.blocks && (spacer || boxed))
    }

    fn fill(&mut self) {
        if !self.in_cell {
            return;
        }
        if let Some(cell) = self.rows.last_mut().and_then(|r| r.last_mut()) {
            *cell = true;
        }
    }
}

/// Reads each `<table>`'s shape, in order, before the chapter is read.
struct Scan(RefCell<(Vec<bool>, Vec<Scanned>)>);

impl TokenSink for Scan {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let (flags, open) = &mut *self.0.borrow_mut();
        let tag = match token {
            Token::TagToken(tag) => tag,
            Token::CharacterTokens(text) => {
                if !text.trim().is_empty() {
                    open.iter_mut().for_each(Scanned::fill);
                }
                return TokenSinkResult::Continue;
            }
            Token::EOFToken => {
                while let Some(t) = open.pop() {
                    close_scanned(flags, &t);
                }
                return TokenSinkResult::Continue;
            }
            _ => return TokenSinkResult::Continue,
        };
        let name: &str = &tag.name;
        let start = tag.kind == TagKind::StartTag;
        match (start, name) {
            (true, "table") => {
                if !tag.self_closing {
                    open.push(Scanned {
                        at: flags.len(),
                        pictured: false,
                        data: false,
                        blocks: false,
                        rows: Vec::new(),
                        in_cell: false,
                    });
                }
                flags.push(false);
            }
            (false, "table") => {
                if let Some(t) = open.pop() {
                    close_scanned(flags, &t);
                }
            }
            (true, "img" | "image" | "svg") => {
                for t in open.iter_mut() {
                    t.pictured = true;
                    t.fill();
                }
            }
            (true, "tr") => {
                if let Some(t) = open.last_mut() {
                    t.rows.push(Vec::new());
                }
            }
            (true, "td" | "th") => {
                if let Some(t) = open.last_mut() {
                    if t.rows.is_empty() {
                        t.rows.push(Vec::new());
                    }
                    if let Some(row) = t.rows.last_mut().filter(|r| r.len() < MAX_COLUMNS) {
                        row.push(false);
                    }
                    t.in_cell = true;
                    t.data |= name == "th";
                }
            }
            (false, "td" | "th") => {
                if let Some(t) = open.last_mut() {
                    t.in_cell = false;
                }
            }
            (true, "thead" | "caption") => {
                if let Some(t) = open.last_mut() {
                    t.data = true;
                }
            }
            (true, raw) if is_raw(raw) && !tag.self_closing => {
                return TokenSinkResult::RawData(RawKind::Rawtext);
            }
            (true, block) if is_block(block) => {
                if let Some(t) = open.last_mut().filter(|t| t.in_cell) {
                    t.blocks = true;
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn close_scanned(flags: &mut [bool], t: &Scanned) {
    if let Some(f) = flags.get_mut(t.at) {
        *f = t.layout();
    }
}

/// Which of the chapter's tables only lay things out; none without one.
fn layout_tables(xhtml: &str) -> Vec<bool> {
    if !xhtml.contains("<table") && !xhtml.contains("<TABLE") {
        return Vec::new();
    }
    let sink = Scan(RefCell::new((Vec::new(), Vec::new())));
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(xhtml));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.0.into_inner().0
}

fn put(text: &mut Text, s: &str, emphasis: Emphasis, link: Option<u32>) {
    match link {
        Some(target) => text.push_link(s, emphasis, target),
        None => text.push(s, emphasis),
    }
}

struct Sink<'a>(RefCell<State<'a>>);

impl TokenSink for Sink<'_> {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut s = self.0.borrow_mut();
        if s.drawing.is_some() {
            s.draw(&token);
            return TokenSinkResult::Continue;
        }
        match token {
            Token::TagToken(tag) => match tag.kind {
                TagKind::StartTag => {
                    if s.start(&tag) {
                        return TokenSinkResult::RawData(RawKind::Rawtext);
                    }
                    s.begin_drawing(&tag);
                }
                TagKind::EndTag => s.end(&tag.name),
            },
            Token::CharacterTokens(text) => s.characters(&text),
            Token::EOFToken => s.eof(),
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

/// Appends a spine picture filling its page, its own size when `fixed`.
pub(crate) fn read_svg_page(
    path: &str,
    library: &mut dyn Library,
    (out, refs): (&mut Vec<Block>, &mut Refs),
    fixed: bool,
) {
    let Some(picture) = library.picture(path) else {
        return;
    };
    let start = Anchor {
        block: out.len(),
        offset: 0,
    };
    refs.places.entry(path.to_owned()).or_insert(start);
    let side = |n: u32| (n as f32).clamp(1.0, MAX_SHEET_SIDE);
    let sheet = fixed.then(|| Sheet {
        width: side(picture.width),
        height: side(picture.height),
        ground: None,
    });
    let mut block = Block::new(Content::Image {
        picture,
        cover: true,
        width: None,
    });
    block.break_before = true;
    block.frame.sheet = sheet.map(Arc::new);
    out.push(block);
}

/// Appends the blocks of `xhtml`, found at `path`; the first starts a page.
pub(crate) fn read_chapter(
    xhtml: &str,
    path: &str,
    library: &mut dyn Library,
    (out, refs): (&mut Vec<Block>, &mut Refs),
) {
    read(xhtml, (path, false), library, (out, refs));
}

/// Appends a fixed-layout page: its blocks laid at its own viewport.
pub(crate) fn read_fixed_chapter(
    xhtml: &str,
    path: &str,
    library: &mut dyn Library,
    (out, refs): (&mut Vec<Block>, &mut Refs),
) {
    read(xhtml, (path, true), library, (out, refs));
}

fn read(
    xhtml: &str,
    (path, fixed): (&str, bool),
    library: &mut dyn Library,
    (out, refs): (&mut Vec<Block>, &mut Refs),
) {
    let first = out.len();
    let start = Anchor {
        block: first,
        offset: 0,
    };
    refs.places.entry(path.to_owned()).or_insert(start);
    let sink = Sink(RefCell::new(State {
        library,
        refs,
        first,
        pending: Vec::new(),
        base: path,
        sheets: Vec::new(),
        style_text: String::new(),
        ruby: None,
        note: String::new(),
        turned: false,
        combining: None,
        style_applies: true,
        root: None,
        blocks: Vec::new(),
        current: None,
        table: None,
        layouts: layout_tables(xhtml),
        tables: 0,
        laying: 0,
        stack: Vec::new(),
        space: false,
        chapter: true,
        float: None,
        page: [255; 3],
        book_ink: [0; 3],
        marker: None,
        marker_colour: None,
        list_opened: false,
        pre_opened: false,
        top: Vec::new(),
        top_count: 0,
        gap: Gap::default(),
        block_line: ROOT_PX * 1.2,
        looks: Vec::new(),
        styled: false,
        signals: Signals::default(),
        boxes: Vec::new(),
        grid: None,
        drawing: None,
        forced: None,
        rule_next: false,
        counters: Vec::new(),
        quotes: 0,
        initial: None,
        first_line: None,
        drop: None,
        marker_picture: None,
        fixed,
        viewport: None,
        sheet: None,
        grounded: false,
        places: Vec::new(),
    }));
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(xhtml));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let state = tokenizer.sink.0.into_inner();
    let mut blocks = state.blocks;
    if fixed {
        // One page: every block on it, and a page even when it is blank.
        let (width, height) = state.viewport.unwrap_or(DEFAULT_SHEET);
        let blank = || {
            Arc::new(Sheet {
                width,
                height,
                ground: None,
            })
        };
        let sheet = state.sheet.unwrap_or_else(blank);
        let mut end = Block::new(Content::Space(0.0));
        end.break_before = blocks.is_empty();
        blocks.push(end);
        for block in &mut blocks {
            block.frame.sheet = Some(Arc::clone(&sheet));
        }
    }
    out.extend(blocks);
}

/// A viewport `<meta>`'s size in CSS px, as `width=1200, height=1600`.
fn viewport(content: &str) -> Option<(f32, f32)> {
    let mut size = (None, None);
    for item in content.split([',', ';']) {
        let Some((key, value)) = item.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_end_matches("px").parse::<f32>().ok();
        let value = value
            .filter(|v| v.is_finite() && *v >= 1.0)
            .map(|v| v.min(MAX_SHEET_SIDE));
        match key.trim().to_ascii_lowercase().as_str() {
            "width" => size.0 = value,
            "height" => size.1 = value,
            _ => {}
        }
    }
    size.0.zip(size.1)
}

/// A length on a fixed page in its CSS px: viewport units are the page's.
fn placed_px(l: Len, base: f32, (width, height): (f32, f32)) -> Option<f32> {
    match l {
        Len::Vw(n) => Some(n * width / 100.0),
        Len::Vh(n) => Some(n * height / 100.0),
        other => other.px(ROOT_PX, base),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use mareader_reflow::Rise;

    /// In-memory images and stylesheets, keyed by absolute path.
    #[derive(Default)]
    struct Shelf {
        pictures: HashMap<String, Arc<Picture>>,
        sheets: HashMap<String, Arc<Stylesheet>>,
        files: HashMap<String, Arc<Vec<u8>>>,
    }

    impl Library for Shelf {
        fn picture(&mut self, path: &str) -> Option<Arc<Picture>> {
            self.pictures.get(path).cloned()
        }

        fn stylesheet(&mut self, path: &str) -> Option<Arc<Stylesheet>> {
            self.sheets.get(path).cloned()
        }

        fn bytes(&mut self, path: &str) -> Option<Arc<Vec<u8>>> {
            self.files.get(path).cloned()
        }
    }

    const BASE: &str = "/OEBPS/text/c1.xhtml";

    fn read_refs(xhtml: &str) -> (Vec<Block>, Refs) {
        let (mut out, mut refs) = (Vec::new(), Refs::default());
        read_chapter(xhtml, BASE, &mut Shelf::default(), (&mut out, &mut refs));
        (out, refs)
    }

    fn read_with(shelf: &mut Shelf, xhtml: &str) -> Vec<Block> {
        let mut out = Vec::new();
        read_chapter(xhtml, BASE, shelf, (&mut out, &mut Refs::default()));
        out
    }

    fn read(xhtml: &str) -> Vec<Block> {
        read_with(&mut Shelf::default(), xhtml)
    }

    fn text(b: &Block) -> Option<&Text> {
        match &b.content {
            Content::Text(t) => Some(t),
            _ => None,
        }
    }

    fn texts(blocks: &[Block]) -> Vec<&str> {
        blocks
            .iter()
            .filter_map(text)
            .map(|t| t.text.as_str())
            .collect()
    }

    fn role(b: Option<&Block>) -> Option<Role> {
        b.and_then(text).map(|t| t.role)
    }

    fn spans(b: Option<&Block>) -> Vec<(&str, Emphasis)> {
        let Some(t) = b.and_then(text) else {
            return Vec::new();
        };
        t.spans
            .iter()
            .filter_map(|s| Some((t.text.get(s.range.clone())?, s.emphasis)))
            .collect()
    }

    fn png(w: u32, h: u32) -> Option<Arc<Picture>> {
        let img = image::RgbImage::new(w, h);
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Picture::probe(out.into_inner()).ok().map(Arc::new)
    }

    #[test]
    fn headings_and_paragraphs_become_blocks() {
        let b = read(
            "<?xml version='1.0'?><html><head><title>T</title>\
             <style>p { color: red }</style></head><body>\
             <h2>Chapter  One</h2><p>First\n   para.</p><p>Second.</p></body></html>",
        );
        assert_eq!(texts(&b), ["Chapter One", "First para.", "Second."]);
        assert_eq!(role(b.first()), Some(Role::Heading(2)));
        assert_eq!(role(b.get(1)), Some(Role::Paragraph));
    }

    #[test]
    fn book_colours_ride_the_themes_greys_keeping_their_hue() {
        let b = read(
            "<html><head><style>body { color: #333 } .r { color: #c00 } \
             .g { color: #999 } .k { color: #000 } .w { color: #fff } \
             .box { background: #000 }</style></head><body>\
             <p>plain <span class='r'>red</span> <span class='g'>grey</span> \
             <span class='k'>black</span> <font color='#c00'>font</font></p>\
             <div class='box'><p class='w'>white on black</p></div>\
             <p style='color: #fafafa'>faint</p></body></html>",
        );
        let marks: Vec<(String, Option<[u8; 3]>)> = b
            .iter()
            .flat_map(|b| spans(Some(b)))
            .map(|(t, e)| (t.trim().to_owned(), e.colour))
            .collect();
        let of = |word: &str| marks.iter().find(|(t, _)| t == word).and_then(|m| m.1);
        let near =
            |c: Option<[u8; 3]>, v: u8| c.is_some_and(|c| c.iter().all(|x| x.abs_diff(v) <= 1));
        // The book's own ink, and anything darker, is the theme's ink.
        assert!(marks.iter().all(|(t, _)| t != "plain"), "{marks:?}");
        assert_eq!(of("black"), None, "{marks:?}");
        assert_eq!(of("white on black"), None, "{marks:?}");
        // A hue keeps the grey it needs; greys fade by their contrast.
        assert_eq!(of("red"), Some([204, 0, 0]), "{marks:?}");
        assert_eq!(of("font"), of("red"));
        assert!(near(of("grey"), 128), "{marks:?}");
        // A faint colour fades only so far.
        assert!(near(of("faint"), 140), "{marks:?}");
    }

    #[test]
    fn borders_markers_and_grids_take_the_books_colours() {
        let b = read(
            "<html><head><style>.box { border: 2px solid #c00; border-left-color: #000 } \
             .tint { color: #1c71d8; border-bottom: 1px dotted } .faint { border: 1px solid #ddd } \
             li { color: #c00 } td { border: 1px solid #c00 }</style></head><body>\
             <div class='box'><p>boxed</p></div><p class='tint'>tinted</p>\
             <div class='faint'><p>faint</p></div><ul><li>item</li></ul>\
             <table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table></body></html>",
        );
        let block = |word: &str| {
            b.iter()
                .find(|b| text(b).is_some_and(|t| t.text.trim() == word))
        };
        let sides = |word: &str| {
            let frame = block(word).map(|b| &b.frame);
            frame.and_then(|f| f.boxes.last()).map(|b| b.colours)
        };
        let red = Some([204, 0, 0]);
        // The book's own ink stays the theme's; other sides keep theirs.
        assert_eq!(sides("boxed"), Some([red, red, red, None]));
        // Unsaid, a border takes its text's colour.
        let blue = sides("tinted").and_then(|s| s[2]);
        assert!(blue.is_some_and(|[r, _, b]| b > r + 100), "{blue:?}");
        // A light rule may stay lighter than text would.
        let faint = sides("faint").and_then(|s| s[0]);
        assert!(
            faint.is_some_and(|c| c.iter().all(|v| *v > 150)),
            "{faint:?}"
        );
        assert_eq!(block("item").and_then(|b| b.frame.marker_colour), red);
        let grid = b.iter().find_map(|b| match &b.content {
            Content::Table(t) => Some(t.rule),
            _ => None,
        });
        assert_eq!(grid, Some(red));
    }

    #[test]
    fn inline_marks_raise_case_and_line_their_text() {
        let b = read(
            "<p>mc<sup>2</sup> H<sub>2</sub>O <u>u</u> <del>d</del> \
             <span style='font-variant: small-caps'>sc</span> \
             <span style='text-transform: uppercase'>up</span></p>",
        );
        let marks: Vec<(String, Emphasis)> = spans(b.first())
            .into_iter()
            .map(|(t, e)| (t.trim().to_owned(), e))
            .collect();
        let of = |word: &str| marks.iter().find(|(t, _)| t == word).map(|(_, e)| *e);
        assert_eq!(of("2").map(|e| e.script), Some(Script::Super), "{marks:?}");
        assert!(
            marks
                .iter()
                .any(|(t, e)| t == "2" && e.script == Script::Sub)
        );
        assert!(of("u").is_some_and(|e| e.underline && !e.strike));
        assert!(of("d").is_some_and(|e| e.strike && !e.underline));
        assert_eq!(of("sc").map(|e| e.case), Some(Case::Small));
        assert_eq!(of("up").map(|e| e.case), Some(Case::Upper));
    }

    #[test]
    fn page_breaks_before_and_after_start_pages() {
        let b = read(
            "<p>a</p><p style='page-break-after: always'>b</p><p>c</p>\
             <h2 style='break-before: page'>d</h2><p>e</p>",
        );
        let breaks: Vec<bool> = b.iter().map(|b| b.break_before).collect();
        assert_eq!(breaks, [true, false, true, true, false]);
    }

    #[test]
    fn backgrounds_show_by_their_contrast_with_the_page() {
        let b = read(
            "<body style='background: #000'>\
             <div style='background-color: #fff; width: 1px; height: 6em'><p><br/></p></div>\
             <div style='background: white; padding: 1em'><p>words</p></div>\
             <div style='background: #111'><p>faint</p></div></body>",
        );
        assert_eq!(kinds(&b), ["space", "text", "text"]);
        let fill = |i: usize| {
            let block = b.get(i);
            block
                .and_then(|b| b.frame.boxes.first())
                .and_then(|x| x.fill)
        };
        let line = b.first();
        assert!(line.is_some_and(|l| matches!(l.content, Content::Space(h) if h >= 6.0)));
        assert!(fill(0).is_some_and(|f| f > 0.9), "{:?}", fill(0));
        assert_eq!(fill(1), Some(MAX_TEXT_FILL));
        assert_eq!(fill(2), None);
    }

    #[test]
    fn a_background_picture_resolves_against_its_sheet() {
        let mut shelf = Shelf::default();
        if let Some(p) = png(4, 4) {
            shelf.pictures.insert("/OEBPS/images/c1.png".to_owned(), p);
        }
        let b = read_with(
            &mut shelf,
            "<html><head><style>.b { background: url('../images/c1.png') no-repeat \
             center / cover; padding: 1em }</style></head>\
             <body><div class='b'><p>over</p></div></body></html>",
        );
        let backdrop = b
            .first()
            .and_then(|b| b.frame.boxes.first())
            .and_then(|x| x.backdrop.clone());
        let Some(backdrop) = backdrop else {
            return assert_eq!(kinds(&b), ["backdrop"]);
        };
        assert_eq!(backdrop.size, BackdropSize::Cover);
        assert!(!backdrop.repeat);
        assert_eq!(backdrop.at, [0.5, 0.5]);
    }

    #[test]
    fn a_floats_blocks_share_it_and_what_follows_does_not() {
        let b = read(
            "<div style='float: right'><p>Key</p><p>word</p></div><p>text</p>\
             <span style='float: left'>inline</span>",
        );
        let float = |i: usize| b.get(i).and_then(|b| b.frame.float.clone());
        let (Some(a), Some(c)) = (float(0), float(1)) else {
            return assert_eq!(texts(&b), ["float"]);
        };
        assert!(Arc::ptr_eq(&a, &c) && a.right);
        assert!(float(2).is_none() && float(3).is_none());
    }

    #[test]
    fn only_the_first_block_starts_a_page() {
        let b = read("<p>a</p><p>b</p>");
        let breaks: Vec<bool> = b.iter().map(|b| b.break_before).collect();
        assert_eq!(breaks, [true, false]);
    }

    #[test]
    fn emphasis_marks_spans() {
        let b = read("<p>a <em>b <strong>c</strong></em> <code>d</code></p>");
        assert_eq!(texts(&b), ["a b c d"]);
        let e = |italic, bold, code| Emphasis {
            italic,
            bold,
            code,
            ..Emphasis::default()
        };
        // A collapsed space joins the run after it.
        assert_eq!(
            spans(b.first()),
            [
                (" b", e(true, false, false)),
                (" c", e(true, true, false)),
                (" d", e(false, false, true))
            ]
        );
    }

    #[test]
    fn self_closing_tags_open_nothing() {
        let b = read(
            "<head><script src='x.js'/><link rel='stylesheet'/></head>\
             <p><a id='n1'/>Kept <em/>plain.</p>",
        );
        assert_eq!(texts(&b), ["Kept plain."]);
        assert!(spans(b.first()).is_empty());
    }

    #[test]
    fn unclosed_and_stray_tags_are_tolerated() {
        let b = read("<p>one<p>two</em></b><div>three</span>");
        assert_eq!(texts(&b), ["one", "two", "three"]);
    }

    #[test]
    fn no_break_spaces_are_kept() {
        let b = read("<p>a&nbsp;&nbsp;b   c</p>");
        assert_eq!(texts(&b), ["a\u{a0}\u{a0}b c"]);
    }

    #[test]
    fn line_breaks_survive_collapsing() {
        let b = read("<p>line one<br/>\n  line two</p>");
        assert_eq!(texts(&b), ["line one\nline two"]);
    }

    #[test]
    fn deep_nesting_is_read_flat() {
        let deep = "<em>".repeat(10_000) + "x" + &"</em>".repeat(10_000);
        let b = read(&format!("<p>{deep}</p>"));
        assert_eq!(texts(&b), ["x"]);
    }

    #[test]
    fn hidden_text_stays_hidden() {
        // A small drawing joins its line; its words aren't the text's.
        let b = read(
            "<p>a<svg width='16' height='16'><text>no</text></svg>b\
             <script>if (a<b) {}</script></p>",
        );
        assert_eq!(texts(&b), ["ab"]);
        assert_eq!(b.first().and_then(text).map(|t| t.inlines.len()), Some(1));
    }

    #[test]
    fn lists_indent_mark_and_tighten_their_items() {
        let b = read(
            "<p>Before</p><ul><li>one</li><li>two<ul><li>deep</li></ul></li></ul>\
             <ol start='4'><li>four<li>five</ol><p>After</p>",
        );
        assert_eq!(
            texts(&b),
            ["Before", "one", "two", "deep", "four", "five", "After"]
        );
        let frames: Vec<(f32, Option<&str>, bool)> = b
            .iter()
            .map(|b| (b.frame.inset, b.frame.marker.as_deref(), b.frame.tight))
            .collect();
        assert_eq!(
            frames,
            [
                (0.0, None, false),
                (1.6, Some("•"), false),
                (1.6, Some("•"), true),
                (3.2, Some("◦"), true),
                (1.6, Some("4."), false),
                (1.6, Some("5."), true),
                (0.0, None, false),
            ]
        );
    }

    #[test]
    fn list_style_none_drops_the_marker_but_keeps_the_count() {
        let b = read(
            "<ol><li>one</li><li style='list-style: none'>two</li><li>three</li></ol>\
             <ul style='list-style-type:none'><li>bare</li></ul>",
        );
        let markers: Vec<Option<&str>> = b.iter().map(|b| b.frame.marker.as_deref()).collect();
        assert_eq!(markers, [Some("1."), None, Some("3."), None]);
    }

    #[test]
    fn generated_content_counts_quotes_and_reads_attributes() {
        let b = read(
            "<html><head><style>body { counter-reset: ch } h2 { counter-increment: ch } \
             h2::before { content: 'Chapter ' counter(ch, upper-roman) ': ' } \
             a::after { content: ' (' attr(href) ')' } \
             .n::before { counter-increment: note; content: counter(note) '. ' } \
             hr.o { border: none } hr.o::after { content: '\\2766' } \
.star::before { content: '*' } \
             .rule::before { content: ''; display: block; border-top: 1px solid } \
             .fix::after { content: ''; display: table }</style></head><body>\
             <h2>Start</h2><h2>Next</h2><p><a href='x.html'>link</a></p>\
             <p><q>a <q>b</q> c</q></p><p class='n'>one</p><p class='n'>two</p>\
             <hr class='o'/><div class='star'/><div class='rule fix'><p>after</p></div>\
             </body></html>",
        );
        let texts: Vec<String> = b
            .iter()
            .filter_map(text)
            .map(|t| t.text.trim().to_owned())
            .collect();
        assert_eq!(
            texts,
            [
                "Chapter I: Start",
                "Chapter II: Next",
                "link (x.html)",
                "\u{201c}a \u{2018}b\u{2019} c\u{201d}",
                "1. one",
                "2. two",
                "❦",
                "*",
                "after",
            ]
        );
        assert!(!b.iter().any(|b| matches!(b.content, Content::Rule)));
        // The bordered empty box shows as a rule of the book's.
        let ruled = b.iter().rev().nth(1);
        assert!(
            ruled.is_some_and(|b| matches!(b.content, Content::Space(_))),
            "{ruled:?}"
        );
    }

    #[test]
    fn list_styles_markers_and_marker_boxes_show() {
        let b = read(
            "<html><head><style>ol.r { list-style-type: lower-roman } \
             ol.arrow li::marker { content: '→'; color: #c00 } \
             ol.my { list-style: myanmar }</style></head><body>\
             <ol class='r'><li>a</li><li>b</li></ol><ol type='A'><li>c</li></ol>\
             <ul><li style='list-style: square'>d</li><li value='7'>e</li></ul>\
             <ol class='arrow'><li>f</li></ol><ol class='my' start='4'><li>g</li></ol>\
             <ol><li value='9'>h</li><li>i</li></ol></body></html>",
        );
        let markers: Vec<Option<&str>> = b.iter().map(|b| b.frame.marker.as_deref()).collect();
        assert_eq!(
            markers,
            [
                Some("i."),
                Some("ii."),
                Some("A."),
                Some("▪"),
                Some("•"),
                Some("→"),
                Some("၄."),
                Some("9."),
                Some("10.")
            ]
        );
        let arrow = b.get(5).and_then(|b| b.frame.marker_colour);
        assert!(arrow.is_some_and(|[r, g, _]| r > g), "{arrow:?}");
    }

    #[test]
    fn drop_caps_and_first_lines_mark_their_text() {
        let b = read(
            "<html><head><style>p.first::first-letter { float: left; font-size: 3em } \
             p.raised::first-letter { font-size: 2em } \
             p.lead::first-line { font-variant: small-caps }</style></head><body>\
             <p class='first'>\u{201c}Alice was here</p>\
             <p><span style='float: left; font-size: 4em'>T</span>he end</p>\
             <p class='raised'>Raised</p><p class='lead'>Lead line</p></body></html>",
        );
        let t: Vec<&Text> = b.iter().filter_map(text).collect();
        let initial: Vec<usize> = t.iter().map(|t| t.initial).collect();
        assert_eq!(initial, [4, 1, 0, 0]);
        assert_eq!(
            t.first().map(|t| t.text.as_str()),
            Some("\u{201c}Alice was here")
        );
        let size = |i: usize| {
            t.get(i)
                .and_then(|t| t.spans.first())
                .map(|s| s.emphasis.size)
        };
        assert_eq!((size(0), size(2)), (Some(300), Some(200)));
        let lead = t.get(3).and_then(|t| t.first_line);
        assert_eq!(lead.map(|e| e.case), Some(Case::Small));
    }

    #[test]
    fn quotes_indent_with_a_bar_and_soften() {
        let b = read("<blockquote><p>said</p><blockquote>again</blockquote></blockquote>");
        let frames: Vec<(f32, &[f32], bool)> = b
            .iter()
            .map(|b| (b.frame.inset, b.frame.bars.as_slice(), b.frame.soft))
            .collect();
        assert_eq!(
            frames,
            [(1.0, &[0.0][..], true), (2.0, &[0.0, 1.0][..], true)]
        );
    }

    #[test]
    fn preformatted_text_keeps_its_spaces_and_lines() {
        let b = read("<pre>\nfn main() {\n\tgo(<b>1</b>);\n}\n</pre><p>a  b</p>");
        assert_eq!(texts(&b), ["fn main() {\n    go(1);\n}", "a b"]);
        assert_eq!(role(b.first()), Some(Role::Preformatted));
    }

    #[test]
    fn tables_read_rows_cells_and_heads() {
        let b = read(
            "<table><caption>Prices</caption><thead><tr><th>Item<th>Cost</thead>\
             <tbody><tr><td>Tea<td><p>3</p><p>each</p></tr>\
             <tr><td>Rice<table><tr><td>in</td><td>sacks</td></tr></table></td></tr>\
             </tbody></table><p>after</p>",
        );
        assert_eq!(texts(&b), ["Prices", "after"]);
        assert_eq!(b.first().map(|b| b.frame.align), Some(Align::Center));
        let table = b.get(1).and_then(|b| match &b.content {
            Content::Table(t) => Some(t),
            _ => None,
        });
        let rows: Vec<(Vec<&str>, bool)> = table
            .map(|t| t.rows.iter())
            .into_iter()
            .flatten()
            .map(|r| (r.cells.iter().map(|c| c.text.as_str()).collect(), r.head))
            .collect();
        assert_eq!(
            rows,
            [
                (vec!["Item", "Cost"], true),
                (vec!["Tea", "3\neach"], false),
                (vec!["Rice in sacks"], false),
            ]
        );
    }

    #[test]
    fn rules_and_images_are_blocks_and_missing_images_show_alt() {
        let mut shelf = Shelf::default();
        if let Some(p) = png(4, 2) {
            shelf.pictures.insert("/OEBPS/images/a.png".into(), p);
        }
        let b = read_with(
            &mut shelf,
            "<p>one</p><hr/><div class='c' style='text-align:center'>\
             <img src='../images/a.png' alt='A'/></div>\
             <img src='gone.png' alt='Gone'/><img src='https://x/y.png'/>\
             <svg><image xlink:href='../images/a.png'/></svg>",
        );
        let kinds: Vec<&str> = b
            .iter()
            .map(|b| match &b.content {
                Content::Text(_) => "text",
                Content::Rule => "rule",
                Content::Space(_) => "space",
                Content::Image { .. } => "image",
                Content::Table(_) => "table",
            })
            .collect();
        assert_eq!(kinds, ["text", "rule", "image", "text", "image"]);
        assert_eq!(b.get(2).map(|b| b.frame.align), Some(Align::Center));
        let italic = Emphasis {
            italic: true,
            ..Emphasis::default()
        };
        assert_eq!(spans(b.get(3)), [("Gone", italic)]);
    }

    /// Block `i`'s picture, if it is one.
    fn picture_of(b: &[Block], i: usize) -> Option<Arc<Picture>> {
        match &b.get(i)?.content {
            Content::Image { picture, .. } => Some(Arc::clone(picture)),
            _ => None,
        }
    }

    #[test]
    fn inline_svg_is_drawn_as_one_picture() {
        let mut shelf = Shelf::default();
        let dot = Arc::new(b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec());
        shelf.files.insert("/OEBPS/images/dot.svg".into(), dot);
        let b = read_with(
            &mut shelf,
            "<p>Before</p>\
             <svg viewBox='0 0 400 100' epub:type='ornament' aria-label='Leaf'>\
             <style><![CDATA[ .a { fill: black } ]]></style>\
             <lineargradient id='g'/><path class='a' d='M0 0L400 100'/>\
             <text>No words</text><image xlink:href='../images/dot.svg'/></svg>\
             <p>After</p>",
        );
        assert_eq!(kinds(&b), ["text", "image", "text"]);
        assert_eq!(texts(&b), ["Before", "After"]);
        let p = picture_of(&b, 1);
        assert!(p.as_ref().is_some_and(|p| p.vector()));
        assert_eq!(p.map(|p| (p.width, p.height)), Some((400, 100)));
    }

    #[test]
    fn a_frame_round_one_image_shows_that_image() {
        let mut shelf = Shelf::default();
        if let Some(p) = png(4, 2) {
            shelf.pictures.insert("/OEBPS/images/c.png".into(), p);
        }
        let b = read_with(
            &mut shelf,
            "<div><svg:svg xmlns:svg='http://www.w3.org/2000/svg' viewBox='0 0 4 2'>\
             <svg:image xlink:href='../images/c.png'/></svg:svg></div>",
        );
        let p = picture_of(&b, 0);
        assert!(p.as_ref().is_some_and(|p| !p.vector() && p.width == 4));
        // Hidden drawings and their text show nothing.
        let none = read("<svg style='display:none'><path d='M0 0'/><text>x</text></svg>");
        assert!(none.is_empty(), "{none:?}");
    }

    #[test]
    fn transparent_borders_keep_their_room_but_draw_nothing() {
        let b = read(
            "<div style='border: 4px solid transparent; border-top-color: red'><p>a</p></div>",
        );
        let spec = b.first().and_then(|b| b.frame.boxes.first());
        let edges = spec.map(|b| b.edges);
        assert_eq!(
            edges,
            Some([Edge::Solid, Edge::Clear, Edge::Clear, Edge::Clear])
        );
        let room = 4.0 / ROOT_PX;
        assert!(spec.is_some_and(|b| b.border.iter().all(|w| (w - room).abs() < 1e-3)));
    }

    #[test]
    fn a_book_drawn_hr_is_a_box_and_a_plain_one_a_rule() {
        let b = read(
            "<html><head><style>hr.s { border: 0; border-top: 3px double; width: 30%; \
             margin: 1em auto } hr.g { border: none; height: 2px; \
             background: linear-gradient(to right, transparent, #000, transparent) }\
             </style></head><body><p>a</p><hr/><hr class='s'/><hr class='g'/><p>b</p>\
             </body></html>",
        );
        assert_eq!(kinds(&b), ["text", "rule", "space", "space", "text"]);
        let boxed = |i: usize| b.get(i).and_then(|b| b.frame.boxes.first());
        assert!(boxed(2).is_some_and(|x| x.rule
            && x.edges[0] == Edge::Double
            && x.width == Some(Length::Percent(30.0))));
        // Empty, its gradient shows at full strength.
        let ramp = boxed(3).and_then(|x| x.finish.clone());
        let stops = ramp.and_then(|f| match f.layers.first() {
            Some(mareader_reflow::Layer::Gradient(g)) => Some(g.stops.clone()),
            _ => None,
        });
        assert!(stops.is_some_and(|s| s.len() == 3 && s.get(1).is_some_and(|s| s.1 > 0.9)));
    }

    #[test]
    fn boxes_carry_shadows_gradients_outlines_and_border_pictures() {
        let svg = "data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' \
                   width='90' height='90'%3E%3C/svg%3E";
        let css = format!(
            ".f {{ box-shadow: 2px 2px 4px rgba(0,0,0,.5); background: \
             linear-gradient(to right, #000, transparent), url(\"{svg}\"); \
             outline: 1px dashed; outline-offset: 2px; border: 30px solid transparent; \
             border-image: url(\"{svg}\") 30 round; opacity: .5 }}"
        );
        let b = read(&format!(
            "<html><head><style>{css}</style></head><body>\
             <div class='f'><p>a</p></div></body></html>"
        ));
        let spec = b.first().and_then(|b| b.frame.boxes.first()).cloned();
        assert_eq!(spec.as_ref().map(|s| s.edges), Some([Edge::Clear; 4]));
        let f = spec.and_then(|s| s.finish.clone());
        let Some(f) = f else {
            return assert!(b.is_empty(), "no finish");
        };
        let shadow = f.shadows.first().copied().unwrap_or_default();
        assert!((shadow.strength - 0.5).abs() < 0.02 && (shadow.at[0] - 0.125).abs() < 1e-3);
        assert!(matches!(f.layers.as_slice(), [
            mareader_reflow::Layer::Gradient(g),
            mareader_reflow::Layer::Picture(_),
        ] if g.stops.iter().all(|s| s.1 <= MAX_TEXT_FILL)));
        let outline = f.outline.map(|o| (o.edge, o.width, o.offset));
        assert_eq!(outline, Some((Edge::Dashed, 1.0 / ROOT_PX, 2.0 / ROOT_PX)));
        let frame = f.frame.as_ref().map(|r| (r.slice, r.width, r.tile, r.fill));
        let third = 30.0 / 90.0;
        assert_eq!(
            frame,
            Some((
                [third; 4],
                [30.0 / ROOT_PX; 4],
                [mareader_reflow::Tile::Round; 2],
                false
            ))
        );
        assert!((f.opacity - 0.5).abs() < 1e-3);
    }

    #[test]
    fn carved_borders_keep_their_style() {
        let b = read("<div style='border: 3px groove; border-left-style: outset'><p>a</p></div>");
        let edges = b
            .first()
            .and_then(|b| b.frame.boxes.first())
            .map(|b| b.edges);
        assert_eq!(
            edges,
            Some([Edge::Groove, Edge::Groove, Edge::Groove, Edge::Outset])
        );
    }

    #[test]
    fn text_shadow_and_opacity_reach_the_text() {
        let b = read(
            "<p>a <span style='text-shadow: 1px 1px #000'>b</span> \
             <span style='opacity: 0.5'>c</span> \
             <span style='text-shadow: 0 0 3px #fff'>d</span></p>",
        );
        let spans = b.first().and_then(text).map_or(Vec::new(), |t| {
            t.spans.iter().map(|s| s.emphasis).collect::<Vec<_>>()
        });
        // A white shadow on white paper is nothing to draw.
        let shadows: Vec<_> = spans.iter().filter_map(|e| e.shadow).collect();
        let one = TextShadow {
            at: [6, 6],
            blur: 0,
            strength: 255,
        };
        assert_eq!(shadows, [one], "{spans:?}");
        let grey = |e: &Emphasis| e.colour.is_some_and(|[r, ..]| r > 60 && r < 200);
        assert!(spans.iter().any(grey), "{spans:?}");
    }

    #[test]
    fn hidden_text_keeps_its_room_and_filters_fade_boxes() {
        let b = read(
            "<div style='visibility: hidden; border: 1px solid'><p>a \
             <span style='visibility: visible'>b</span></p></div>\
             <div style='filter: opacity(50%) drop-shadow(1px 1px 2px #000); \
             border: 1px solid'><p>c</p></div>",
        );
        let hidden = b.first().and_then(text).map_or(Vec::new(), |t| {
            let spans = t.spans.iter().filter(|s| s.emphasis.hidden);
            let words = spans.filter_map(|s| t.text.get(s.range.clone()));
            words.map(|w| w.trim().to_owned()).collect::<Vec<_>>()
        });
        assert_eq!(hidden, ["a"]);
        let finish = |i: usize| {
            let spec = b.get(i).and_then(|b| b.frame.boxes.first());
            spec.and_then(|s| s.finish.clone())
        };
        assert!(finish(0).is_some_and(|f| f.opacity == 0.0));
        let faded = finish(1);
        assert!(faded.is_some_and(|f| (f.opacity - 0.5).abs() < 1e-3 && f.shadows.len() == 1));
    }

    #[test]
    fn background_pictures_may_be_data_urls() {
        let svg = "data:image/svg+xml;utf8,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 \
                   width=%2220%22 height=%2220%22%3E%3C/svg%3E";
        let b = read(&format!(
            "<div style=\"background: url('{svg}') no-repeat; padding: 1em\"><p>a</p></div>"
        ));
        let spec = b.first().and_then(|b| b.frame.boxes.first());
        let size = spec
            .and_then(|s| s.backdrop.as_ref())
            .map(|d| d.picture.width);
        assert_eq!(size, Some(20));
    }

    #[test]
    fn data_urls_are_pictures() {
        let svg = "data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' \
                   width='30' height='10'%3E%3C/svg%3E";
        let b = read(&format!("<p>a</p><img src=\"{svg}\"/>"));
        assert_eq!(picture_of(&b, 1).map(|p| p.width), Some(30));
        // A 1 × 1 PNG, base64.
        let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAf\
                   FcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let b = read(&format!("<p>a</p><img src='{png}'/>"));
        assert_eq!(picture_of(&b, 1).map(|p| p.width), Some(1));
    }

    #[test]
    fn an_svg_page_fills_its_page() {
        let mut shelf = Shelf::default();
        let page = b"<svg xmlns='http://www.w3.org/2000/svg' width='600' height='800'/>";
        let picture = Picture::probe(page.to_vec()).ok().map(Arc::new);
        if let Some(p) = picture {
            shelf.pictures.insert("/OPS/p1.svg".into(), p);
        }
        let (mut out, mut refs) = (Vec::new(), Refs::default());
        read_svg_page("/OPS/p1.svg", &mut shelf, (&mut out, &mut refs), false);
        let cover = matches!(
            out.first().map(|b| &b.content),
            Some(Content::Image { cover: true, .. })
        );
        assert!(cover && out.first().is_some_and(|b| b.break_before));
        assert_eq!(refs.place("/OPS/p1.svg").map(|a| a.block), Some(0));
    }

    fn kinds(b: &[Block]) -> Vec<&'static str> {
        b.iter()
            .map(|b| match &b.content {
                Content::Text(_) => "text",
                Content::Rule => "rule",
                Content::Space(_) => "space",
                Content::Image { .. } => "image",
                Content::Table(_) => "table",
            })
            .collect()
    }

    #[test]
    fn a_table_laying_out_pictures_reads_as_blocks() {
        let mut shelf = Shelf::default();
        for name in ["c1", "c2"] {
            if let Some(p) = png(4, 4) {
                shelf
                    .pictures
                    .insert(format!("/OEBPS/images/{name}.png"), p);
            }
        }
        let b = read_with(
            &mut shelf,
            "<h2>Main</h2><table class='grid'><tr>\
             <td style='text-align:center'><img src='../images/c1.png' alt='char1'/>\
             <p>Sistine</p><p>The honour student</p></td>\
             <td class='gap'>&#160;</td>\
             <td><div><img src='../images/c2.png' alt='char2'/></div><p>Glenn</p>\
             <table><tr><th>Age</th><td>19</td></tr></table></td>\
             </tr></table><p>after</p>",
        );
        assert_eq!(
            kinds(&b),
            [
                "text", "image", "text", "text", "image", "text", "table", "text"
            ]
        );
        let words = texts(&b);
        assert!(!words.iter().any(|t| t.contains("char")), "{words:?}");
        assert_eq!(b.get(1).map(|b| b.frame.align), Some(Align::Center));
        // Unaligned, a grid's picture is centred too.
        assert_eq!(b.get(4).map(|b| b.frame.align), Some(Align::Center));
    }

    #[test]
    fn presentation_tables_read_as_blocks_and_spacer_columns_go() {
        let b = read(
            "<table role='presentation'><tr><td>one</td><td>two</td></tr></table>\
             <table><tr><th>Word</th><th></th><th>Means</th></tr>\
             <tr><td>tea</td><td> </td><td>a drink</td></tr></table>",
        );
        assert_eq!(kinds(&b), ["text", "text", "table"]);
        let cells: Vec<Vec<&str>> = match b.get(2).map(|b| &b.content) {
            Some(Content::Table(t)) => t
                .rows
                .iter()
                .map(|r| r.cells.iter().map(|c| c.text.as_str()).collect())
                .collect(),
            _ => Vec::new(),
        };
        assert_eq!(cells, [vec!["Word", "Means"], vec!["tea", "a drink"]]);
    }

    #[test]
    fn a_grid_of_text_with_spacers_reads_as_blocks_but_data_stays() {
        let b = read(
            "<table><tr><td><div>Wendy</div><p>A noble</p></td><td> </td>\
             <td><div>Lynn</div><p>Shy</p></td></tr></table>\
             <table><tr><td><p>tea</p></td><td><p>3</p></td></tr>\
             <tr><td><p>rice</p></td><td><p>5</p></td></tr></table>",
        );
        assert_eq!(kinds(&b), ["text", "text", "text", "text", "table"]);
        assert_eq!(texts(&b), ["Wendy", "A noble", "Lynn", "Shy"]);
    }

    #[test]
    fn boxes_keep_their_border_width_and_place() {
        let b = read(
            "<div style=\"border: 3px double; width: 10em; margin: 0 auto\">\
             <p style=\"margin: 0\">one</p><p style=\"margin: 0\">two</p></div>\
             <p>after</p>",
        );
        assert_eq!(texts(&b), ["one", "two", "after"]);
        let boxes = |i: usize| b.get(i).map_or(&[][..], |b| b.frame.boxes.as_slice());
        let (one, two) = (boxes(0).first(), boxes(1).first());
        assert!(one.zip(two).is_some_and(|(a, b)| Arc::ptr_eq(a, b)));
        assert!(boxes(2).is_empty());
        let spec = one.map(|b| (**b).clone()).unwrap_or_default();
        assert_eq!(spec.edges, [Edge::Double; 4]);
        assert_eq!(
            (spec.width, spec.margin),
            (Some(Length::Em(10.0)), [None, None])
        );
        // Its inside top and bottom: the border, the paragraphs flush.
        let border = 3.0 / ROOT_PX;
        assert!((spec.top - border).abs() < 1e-3 && (spec.bottom - border).abs() < 1e-3);
    }

    #[test]
    fn an_empty_bordered_box_is_a_rule_of_its_own() {
        let b = read(
            "<p>a</p><p style=\"border-top: 2px solid; width: 14em; margin: 0 auto\"></p>\
             <p>b</p>",
        );
        assert_eq!(kinds(&b), ["text", "space", "text"]);
        let rule = b.get(1);
        assert!(rule.is_some_and(|r| matches!(r.content, Content::Space(h) if h > 0.1)));
        let edges = rule.and_then(|r| r.frame.boxes.first()).map(|b| b.edges);
        assert_eq!(
            edges,
            Some([Edge::Solid, Edge::None, Edge::None, Edge::None])
        );
    }

    #[test]
    fn a_layout_grids_cells_share_their_row() {
        let b = read(
            "<table role=\"presentation\" style=\"width: 22em\"><tr>\
             <td style=\"width: 10em; vertical-align: top\"><p>a</p></td>\
             <td style=\"width: 2em\"></td><td style=\"width: 10em\"><p>b</p><p>c</p></td>\
             </tr><tr><td><p>d</p></td></tr></table><p>after</p>",
        );
        assert_eq!(texts(&b), ["a", "b", "c", "d", "after"]);
        let cell = |i: usize| b.get(i).and_then(|b| b.frame.cell.clone());
        let columns: Vec<_> = (0..5).map(|i| cell(i).map(|c| c.column)).collect();
        assert_eq!(columns, [Some(0), Some(2), Some(2), Some(0), None]);
        let row = cell(0).map(|c| c.row);
        let same = |i: usize| {
            cell(i)
                .zip(row.as_ref())
                .is_some_and(|(c, r)| Arc::ptr_eq(&c.row, r))
        };
        assert!(same(1) && same(2) && !same(3));
        let row = row.map(|r| (*r).clone()).unwrap_or_default();
        assert_eq!(row.widths.len(), 3);
        assert_eq!(row.widths.first(), Some(&Some(Length::Em(10.0))));
        assert_eq!(row.valign.first(), Some(&VAlign::Top));
        // The table's box holds the grid, outside each cell.
        assert!(cell(0).is_some_and(|c| c.depth == 1));
        let table = b.first().and_then(|b| b.frame.boxes.first());
        assert!(table.is_some_and(|t| t.width == Some(Length::Em(22.0))));
    }

    /// Block `i`'s grid row and item.
    fn cell_of(b: &[Block], i: usize) -> Option<(Arc<GridRow>, u16)> {
        b.get(i)
            .and_then(|b| b.frame.cell.clone())
            .map(|c| (c.row, c.column))
    }

    #[test]
    fn flex_rows_read_their_items_as_cells() {
        let b = read(
            "<div style='display: flex; justify-content: center; gap: 16px; flex-wrap: wrap'>\
             <div style='flex: 1'><p>a</p></div>\
             <div style='flex-basis: 5em; align-self: flex-end'><p>b</p></div></div>\
             <p>after</p>",
        );
        assert_eq!(texts(&b), ["a", "b", "after"]);
        let (Some((row, 0)), Some((same, 1)), None) =
            (cell_of(&b, 0), cell_of(&b, 1), cell_of(&b, 2))
        else {
            return assert_eq!(b.len(), 0, "cells");
        };
        assert!(Arc::ptr_eq(&row, &same));
        let flex = Flex {
            justify: Justify::Center,
            gap: 1.0,
            wrap: true,
        };
        assert_eq!(row.columns.flex, Some(flex));
        assert_eq!(row.grow, [1.0, 0.0]);
        assert_eq!(row.widths, [None, Some(Length::Em(5.0))]);
        assert_eq!(row.valign, [VAlign::Top, VAlign::Bottom]);
    }

    #[test]
    fn a_flex_row_of_one_lets_its_layout_table_be_the_grid() {
        let b = read(
            "<div style='display: flex; justify-content: center'><div>\
             <table role='presentation'><tr><td><p>a</p></td><td><p>b</p></td></tr>\
             </table></div></div>",
        );
        let (Some((row, 0)), Some((_, 1))) = (cell_of(&b, 0), cell_of(&b, 1)) else {
            return assert_eq!(b.len(), 0, "cells");
        };
        assert_eq!(row.columns.flex, None);
    }

    #[test]
    fn cells_span_columns_and_rows() {
        let b = read(
            "<table role='presentation' style='width: 20em'>\
             <tr><td colspan='2'><p>a</p></td></tr>\
             <tr><td rowspan='2'><p>b</p></td><td><p>c</p></td></tr>\
             <tr><td><p>d</p></td></tr></table>",
        );
        assert_eq!(texts(&b), ["a", "b", "c", "d"]);
        let row = |i: usize| cell_of(&b, i).map(|(r, _)| r).unwrap_or_default();
        let (a, b2, d) = (row(0), row(1), row(3));
        assert_eq!((a.starts.clone(), a.spans.clone()), (vec![0], vec![2]));
        assert_eq!(
            (b2.starts.clone(), b2.spans.clone()),
            (vec![0, 1], vec![1, 1])
        );
        assert_eq!(d.starts, [1]);
        assert!(a.columns.fill && Arc::ptr_eq(&a.columns, &d.columns));
    }

    #[test]
    fn boxes_turn_and_round_and_small_pictures_join_their_line() {
        let mut shelf = Shelf::default();
        if let Some(p) = png(16, 16) {
            shelf.pictures.insert("/OEBPS/images/s.png".into(), p);
        }
        let b = read_with(
            &mut shelf,
            "<div style='transform: rotate(0.25turn); border: 1px solid; \
             border-radius: 8px'><p>t</p></div>\
             <p>x <img src='../images/s.png'/> y</p>",
        );
        assert_eq!(kinds(&b), ["text", "text"]);
        let first = b.first().map(|b| &b.frame);
        assert_eq!(first.map(|f| f.rotate), Some(90.0));
        let radius = first.and_then(|f| f.boxes.first()).and_then(|b| b.radius);
        assert_eq!(radius, Some(Length::Em(0.5)));
        let inlines = b
            .get(1)
            .and_then(text)
            .map(|t| t.inlines.clone())
            .unwrap_or_default();
        assert_eq!(inlines.len(), 1);
        assert!(
            inlines
                .first()
                .is_some_and(|i| i.at <= 2 && (i.width - 1.0).abs() < 0.01)
        );
    }

    #[test]
    fn a_column_flex_lays_each_item_as_its_own_row() {
        let mut shelf = Shelf::default();
        if let Some(p) = png(400, 300) {
            shelf.pictures.insert("/OEBPS/images/a.png".into(), p);
        }
        let b = read_with(
            &mut shelf,
            "<div style='display: flex; flex-direction: column; align-items: center; \
             gap: 2em 1em'><div><p>One</p></div><div style='align-self: stretch'>\
             <p>Two</p></div><div style='width: 5em'><div style='display: flex'>\
             <p>A</p><p>B</p></div></div><img src='../images/a.png'/></div>",
        );
        let texts: Vec<&str> = b.iter().filter_map(text).map(|t| t.text.trim()).collect();
        let row = |i: usize| b.get(i).and_then(|b| b.frame.cell.as_ref()).map(|c| &c.row);
        let justify = |i| row(i).and_then(|r| r.columns.flex).map(|f| f.justify);
        assert_eq!(texts, ["One", "Two", "A", "B"]);
        assert_eq!(justify(0), Some(Justify::Center));
        // Stretched, the second reads as prose, two ems below the first.
        assert!(row(1).is_none(), "{:?}", b.get(1));
        assert!(
            b.get(1).is_some_and(|b| b.frame.before >= 2.0),
            "{:?}",
            b.get(1)
        );
        // The row inside an item lets the item go and lays out itself.
        assert_eq!(row(2).map(|r| r.widths.len()), Some(2));
        assert!(row(2).zip(row(3)).is_some_and(|(a, b)| Arc::ptr_eq(a, b)));
        assert_eq!(b.get(4).map(|b| b.frame.align), Some(Align::Center));
        assert!(b.get(4).is_some_and(|b| b.frame.cell.is_none()));
    }

    #[test]
    fn a_full_height_flex_box_holds_its_row() {
        let b = read(
            "<div style='display: flex; height: 94vh; align-items: center'>\
             <div><p>Key</p></div></div>",
        );
        let frame = b.first().map(|b| &b.frame);
        let centre = frame.and_then(|f| f.boxes.first()).and_then(|b| b.centre);
        assert_eq!(centre, Some(0.94), "{frame:?}");
        let held = frame.and_then(|f| f.boxes.first()).and_then(|b| b.height);
        assert_eq!(held, None, "the page's share, not a height");
        assert_eq!(
            frame.and_then(|f| f.cell.as_ref()).map(|c| c.depth),
            Some(1)
        );
        assert!(
            frame
                .and_then(|f| f.look.as_ref())
                .is_some_and(|l| l.designed)
        );
    }

    #[test]
    fn line_pictures_sit_where_their_vertical_align_puts_them() {
        let mut shelf = Shelf::default();
        if let Some(p) = png(16, 16) {
            shelf.pictures.insert("/OEBPS/images/s.png".into(), p);
        }
        let img =
            |align: &str| format!("<img style='vertical-align: {align}' src='../images/s.png'/>");
        let html = ["middle", "text-top", "bottom", "0.25em", "-8px", "auto"].map(img);
        let b = read_with(&mut shelf, &format!("<p>x {}</p>", html.concat()));
        let rises: Vec<Rise> = (b.first().and_then(text).into_iter())
            .flat_map(|t| t.inlines.iter().map(|i| i.rise))
            .collect();
        let expected = [
            Rise::Middle,
            Rise::TextTop,
            Rise::Bottom,
            Rise::By(0.25),
            Rise::By(-0.5),
            Rise::Baseline,
        ];
        assert_eq!(rises, expected);
    }

    #[test]
    fn a_one_cell_table_of_paragraphs_is_a_framed_box() {
        let b = read(
            "<table style=\"border: 3px double; width: 17em; margin: 0 auto\">\
             <tr><td><p>KEY WORD</p><p>A sword.</p></td></tr></table>",
        );
        assert_eq!(kinds(&b), ["text", "text"]);
        let edges = b
            .get(1)
            .and_then(|b| b.frame.boxes.first())
            .map(|b| b.edges);
        assert_eq!(edges, Some([Edge::Double; 4]));
    }

    #[test]
    fn images_take_the_width_the_book_sets() {
        let mut shelf = Shelf::default();
        if let Some(p) = png(4, 4) {
            shelf.pictures.insert("/OEBPS/images/a.png".into(), p);
        }
        let sheet = Stylesheet::parse("img.small { width: 2em }", "/OEBPS/s.css");
        shelf.sheets.insert("/OEBPS/s.css".into(), Arc::new(sheet));
        let b = read_with(
            &mut shelf,
            "<link rel='stylesheet' href='../s.css'/>\
             <img src='../images/a.png' style='width: 6.5em'/>\
             <img class='small' src='../images/a.png'/>\
             <img src='../images/a.png' width='32'/>\
             <img src='../images/a.png' style='width: 50%'/>\
             <img src='../images/a.png'/>",
        );
        let widths: Vec<Option<Length>> = b
            .iter()
            .filter_map(|b| match &b.content {
                Content::Image { width, .. } => Some(*width),
                _ => None,
            })
            .collect();
        assert_eq!(
            widths,
            [
                Some(Length::Em(6.5)),
                Some(Length::Em(2.0)),
                Some(Length::Em(2.0)),
                Some(Length::Percent(50.0)),
                None,
            ]
        );
    }

    #[test]
    fn stylesheets_style_by_tag_and_class() {
        let mut shelf = Shelf::default();
        let linked = Stylesheet::parse(
            ".it { font-style: italic } .gone { display: none }",
            "/OEBPS/css/book.css",
        );
        shelf
            .sheets
            .insert("/OEBPS/css/book.css".into(), Arc::new(linked));
        let b = read_with(
            &mut shelf,
            "<head><link rel='stylesheet' href='../css/book.css'/>\
             <style>p.c { text-align: center } span.b { font-weight: bold }</style></head>\
             <p class='c'>mid <span class='b it'>x</span></p>\
             <p class='gone'>hidden</p><p style='font-style:italic'>y</p>",
        );
        assert_eq!(texts(&b), ["mid x", "y"]);
        assert_eq!(b.first().map(|b| b.frame.align), Some(Align::Center));
        let e = |italic, bold| Emphasis {
            italic,
            bold,
            ..Emphasis::default()
        };
        assert_eq!(spans(b.first()), [(" x", e(true, true))]);
        assert_eq!(spans(b.get(1)), [("y", e(true, false))]);
    }

    #[test]
    fn a_chapter_applies_a_few_sheets_each_once() {
        let mut shelf = Shelf::default();
        let linked = Stylesheet::parse(".it { font-style: italic }", "/OEBPS/css/book.css");
        shelf
            .sheets
            .insert("/OEBPS/css/book.css".into(), Arc::new(linked));
        let links = "<link rel='stylesheet' href='../css/book.css'/>".repeat(100);
        let styles: String = (0..MAX_SHEETS)
            .map(|i| format!("<style>.c{i} {{ text-align: center }}</style>"))
            .collect();
        let last = MAX_SHEETS - 2;
        let body = format!(
            "<p class='c0'>a</p><p class='c{last}'>b</p><p class='c{}'>c</p>",
            last + 1
        );
        let b = read_with(&mut shelf, &format!("<head>{links}{styles}</head>{body}"));
        let aligns: Vec<Align> = b.iter().map(|b| b.frame.align).collect();
        // The linked sheet counts once, so the last style is one too many.
        assert_eq!(aligns, [Align::Center, Align::Center, Align::Start]);
    }

    #[test]
    fn sheets_load_by_media_and_import_and_fill_in_custom_values() {
        let mut shelf = Shelf::default();
        for (path, css) in [
            (
                "/OEBPS/css/book.css",
                "@import url(base.css); @import 'print.css' print; .it { font-style: italic }",
            ),
            (
                "/OEBPS/css/base.css",
                "p { --mid: center; text-align: var(--mid) }",
            ),
            ("/OEBPS/css/print.css", "p { display: none }"),
            ("/OEBPS/css/night.css", "p { display: none }"),
            (
                "/OEBPS/css/loop.css",
                "@import url(loop.css); .lp { font-weight: bold }",
            ),
        ] {
            let sheet = Arc::new(Stylesheet::parse(css, path));
            shelf.sheets.insert(path.into(), sheet);
        }
        let b = read_with(
            &mut shelf,
            "<head><link rel='stylesheet' href='../css/book.css'/>\
             <link rel='alternate stylesheet' href='../css/night.css'/>\
             <link rel='stylesheet' media='print' href='../css/print.css'/>\
             <link rel='stylesheet' href='../css/loop.css'/>\
             <style media='amzn-kf8'>p { display: none }</style>\
             <style media='screen'>.wide { width: calc(100% - 4em) }</style></head>\
             <p>a <span class='it lp'>b</span></p>\
             <div class='wide' style='border: 1px solid'><p>c</p></div>",
        );
        assert_eq!(texts(&b), ["a b", "c"]);
        assert_eq!(b.first().map(|b| b.frame.align), Some(Align::Center));
        let e = Emphasis {
            italic: true,
            bold: true,
            ..Emphasis::default()
        };
        assert_eq!(spans(b.first()), [(" b", e)]);
        let wide = b
            .get(1)
            .and_then(|b| b.frame.boxes.first())
            .map(|x| x.width);
        let mix = Length::Mix {
            em: -4.0,
            percent: 100.0,
        };
        assert_eq!(wide, Some(Some(mix)));
    }

    #[test]
    fn root_custom_values_reach_the_body_once_sheets_load() {
        let b = read(
            "<html class='x'><head><style>:root { --mid: center } \
             html.x { --pad: 2em } p { text-align: var(--mid); text-indent: var(--pad) }\
             </style></head><body><p>a</p></body></html>",
        );
        assert_eq!(b.first().map(|b| b.frame.align), Some(Align::Center));
        let indent = b
            .first()
            .and_then(|b| b.frame.look.as_ref())
            .map(|l| l.indent);
        assert_eq!(indent, Some(2.0));
    }

    #[test]
    fn ids_mark_their_block_or_their_place_in_its_text() {
        let (_, refs) = read_refs(
            "<h1 id='top'>Title</h1><p>ab <span id='mid'>c</span></p>\
             <div id='empty'></div><p><a name='old'></a>d</p>\
             <table><tr><td id='cell'>e</td></tr></table><p id='end'></p>",
        );
        let at = |key: &str| {
            let a = refs.place(&format!("{BASE}{key}"));
            a.map(|a| (a.block, a.offset))
        };
        assert_eq!(at(""), Some((0, 0)));
        assert_eq!(at("#top"), Some((0, 0)));
        assert_eq!(at("#mid"), Some((1, 2)));
        assert_eq!(at("#empty"), Some((2, 0)));
        assert_eq!(at("#old"), Some((2, 0)));
        assert_eq!(at("#cell"), Some((3, 0)));
        // Past the last block: where the next chapter will start.
        assert_eq!(at("#end"), Some((4, 0)));
        // An unknown id falls back to its file's start.
        assert_eq!(at("#nowhere"), Some((0, 0)));
    }

    #[test]
    fn links_mark_their_words_and_keep_where_they_lead() {
        let (blocks, refs) = read_refs(
            "<p>See <a href='c2.xhtml#n%201'>the <i>first</i> note</a> \
             or <a href='https://example.org/'>a site</a>.</p>\
             <p><a href='#top'>Back</a> <a href='javascript:x()'>run</a>\
             <a href=''>none</a></p>",
        );
        let links = |b: Option<&Block>| {
            let t = b.and_then(text);
            let words = |l: &mareader_reflow::Link| {
                let s = t.and_then(|t| t.text.get(l.range.clone()));
                (s.unwrap_or_default().to_owned(), l.target)
            };
            t.map(|t| t.links.iter().map(words).collect::<Vec<_>>())
        };
        let first = links(blocks.first()).unwrap_or_default();
        assert_eq!(first, [("the first note".into(), 0), ("a site".into(), 1)]);
        let second = links(blocks.get(1)).unwrap_or_default();
        assert_eq!(second, [("Back".into(), 2), ("run".into(), 3)]);
        let inside = |s: &str| Href::Inside(s.to_owned());
        assert_eq!(
            refs.hrefs,
            [
                inside("/OEBPS/text/c2.xhtml#n 1"),
                Href::Outside("https://example.org/".into()),
                inside(&format!("{BASE}#top")),
                Href::Outside("javascript:x()".into()),
            ]
        );
    }

    fn look(b: Option<&Block>) -> Option<&Look> {
        b.and_then(|b| b.frame.look.as_deref())
    }

    #[test]
    fn designed_chapters_keep_the_books_type_and_space() {
        let b = read(
            "<style>@font-face { font-family: Title; src: url(../Fonts/t.ttf) }\
             .t { font-family: Title, serif; font-size: 2em; letter-spacing: 0.3em }\
             .gap { margin-top: 1em } p { margin: 0.5em 0 } .lh { line-height: 1em }\
             </style><p class='t'>TITLE <span style='font-size: 50%'>small</span></p>\
             <div class='gap'><p class='lh'>cast</p></div><p>after</p>",
        );
        assert_eq!(texts(&b), ["TITLE small", "cast", "after"]);
        let title = look(b.first());
        assert!(title.is_some_and(|l| l.designed && l.size == 2.0));
        assert_eq!(
            title.map(|l| l.faces.clone()),
            Some(vec![format!("{BOOK_FACE}title")])
        );
        assert!(title.is_some_and(|l| (l.letter - 0.6).abs() < 0.01));
        let small = spans(b.first())
            .into_iter()
            .find(|(t, _)| t.trim() == "small");
        assert_eq!(small.map(|(_, e)| e.size), Some(50));
        // Margins collapse to the largest; a block's own are in its em.
        let before: Vec<f32> = b.iter().map(|b| b.frame.before).collect();
        assert_eq!(before, [1.0, 1.0, 0.5]);
        assert_eq!(look(b.get(1)).and_then(|l| l.line_height), Some(1.0));
    }

    #[test]
    fn a_framed_figure_leaves_prose_prose_but_a_framed_page_is_designed() {
        let prose = "<p>Once upon a midnight dreary, while I pondered.</p>".repeat(40);
        let designed = |b: &[Block]| b.iter().any(|b| look(Some(b)).is_some_and(|l| l.designed));
        let figure = "<div style='width: 556px'><p>A figure's caption.</p></div>\
             <p style='border-top: 1px dotted'>Transcriber's note.</p>";
        assert!(!designed(&read(&format!("{prose}{figure}{prose}"))));
        // Short, or mostly inside its frames, a chapter is laid out.
        assert!(designed(&read(figure)));
        let framed = format!("<div style='border: 1px solid'>{prose}</div>");
        assert!(designed(&read(&framed)));
    }

    #[test]
    fn unstyled_chapters_leave_the_type_to_the_reader() {
        let b = read("<h1>One</h1><p>a</p>");
        assert!(b.iter().all(|b| b.frame.look.is_none()), "{b:?}");
        let b = read("<h1>One</h1><p style='text-indent: 1em'>a</p>");
        assert!(b.iter().all(|b| b.frame.look.is_some()), "{b:?}");
        // Sans and mono stay marked; the machine's own names stay too.
        let b = read("<style>p { font-family: Georgia, monospace }</style><p>a</p>");
        let faces = look(b.first()).map(|l| l.faces.clone());
        assert_eq!(
            faces,
            Some(vec!["Georgia".to_owned(), "monospace".to_owned()])
        );
    }

    #[test]
    fn a_headings_plain_words_stay_light_at_their_size() {
        let b = read(
            "<style>.t { font-weight: normal; font-size: 26px }\
             .n { font-weight: normal; font-size: 94px }</style>\
             <h2><span class='t'>Chapter</span> <span class='n'>2</span> The Pool</h2>",
        );
        let runs: Vec<(String, bool, u16)> = (spans(b.first()).into_iter())
            .map(|(t, e)| (t.trim().to_owned(), e.light, e.size))
            .collect();
        // Against the heading's 24 px: 26 and 94 px.
        let want = [("Chapter", true, 108), ("2", true, 392)];
        assert_eq!(runs, want.map(|(t, l, s)| (t.to_owned(), l, s)), "{b:?}");
    }

    #[test]
    fn prose_chapters_carry_their_type_unlaid() {
        let b = read(
            "<style>p { margin: 1em 0; font-size: 1.1em } h1 { font-size: 2em }\
             h1 + p { text-align: center }</style>\
             <h1>One</h1><p>a <b>b</b></p><p>c</p>",
        );
        assert!(b.iter().all(|b| look(Some(b)).is_some_and(|l| !l.designed)));
        // A heading's own weight is no emphasis; bold in a paragraph is.
        assert!(spans(b.first()).iter().all(|(_, e)| !e.bold));
        assert!(
            spans(b.get(1))
                .iter()
                .any(|(t, e)| t.trim() == "b" && e.bold)
        );
        let aligns: Vec<Align> = b.iter().map(|b| b.frame.align).collect();
        assert_eq!(aligns, [Align::Start, Align::Center, Align::Start]);
    }

    #[test]
    fn direction_follows_the_attribute_and_the_sheet() {
        let blocks = read(
            "<html><head><style>.r { direction: rtl } .l { direction: ltr }</style></head>\
             <body><div class=\"r\"><p>a</p><p dir=\"ltr\">b</p></div>\
             <p dir=\"rtl\" class=\"l\">c</p><p dir=\"auto\">d</p><p>e</p></body></html>",
        );
        let dirs: Vec<Dir> = blocks.iter().map(|b| b.frame.dir).collect();
        // The sheet beats the attribute, which is only a hint.
        assert_eq!(dirs, [Dir::Rtl, Dir::Ltr, Dir::Ltr, Dir::Auto, Dir::Auto]);
    }

    #[test]
    fn ruby_without_rt_or_too_long_a_note_adds_nothing() {
        let long = "x".repeat(MAX_NOTE + 1);
        let blocks = read(&format!(
            "<body><p><ruby>base</ruby> <ruby>more<rt>{long}</rt></ruby>\
             <ruby>end<rt> </rt></ruby></p></body>"
        ));
        let t = blocks.first().and_then(text);
        assert_eq!(
            t.map(|t| (t.text.as_str(), t.ruby.len())),
            Some(("base moreend", 0))
        );
    }
}
