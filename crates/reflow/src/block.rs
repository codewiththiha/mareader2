//! What a book says, format-free: framed blocks of text, pictures, tables.
use std::ops::Range;
use std::sync::Arc;

use crate::picture::Picture;

/// A text block's part; it picks the face, size and spacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Level 1 to 6, as in HTML.
    Heading(u8),
    Paragraph,
    /// Code and verse: spaces and line breaks kept, set in mono.
    Preformatted,
}

impl Role {
    pub fn is_heading(self) -> bool {
        matches!(self, Self::Heading(_))
    }
}

/// How a run of text differs from the block around it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Emphasis {
    pub italic: bool,
    pub bold: bool,
    /// Inline code, set in the mono face.
    pub code: bool,
    /// Percent of the block's text size; 0 keeps it. Designed text only.
    pub size: u16,
}

/// `range` is in bytes of the block's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub emphasis: Emphasis,
}

/// A run that leads elsewhere; the format knows what `target` means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub range: Range<usize>,
    pub target: u32,
}

/// Styled text; spans cover only emphasised runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub role: Role,
    pub text: String,
    pub spans: Vec<Span>,
    /// Linked runs, in order and apart.
    pub links: Vec<Link>,
}

impl Text {
    pub fn new(role: Role) -> Self {
        Self {
            role,
            text: String::new(),
            spans: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Appends `text` as part of link `target`; a leading space isn't.
    pub fn push_link(&mut self, text: &str, emphasis: Emphasis, target: u32) {
        let at = self.text.len();
        let start = at + (text.len() - text.trim_start_matches(' ').len());
        self.push(text, emphasis);
        let end = self.text.len();
        if start >= end {
            return;
        }
        match self.links.last_mut() {
            // The same link going on, past a space between words.
            Some(last) if last.range.end == at && last.target == target => {
                last.range.end = end;
            }
            _ => self.links.push(Link {
                range: start..end,
                target,
            }),
        }
    }

    /// Appends `text`, merging with the last span when they match.
    pub fn push(&mut self, text: &str, emphasis: Emphasis) {
        let start = self.text.len();
        self.text.push_str(text);
        if emphasis == Emphasis::default() || text.is_empty() {
            return;
        }
        let end = self.text.len();
        match self.spans.last_mut() {
            Some(last) if last.range.end == start && last.emphasis == emphasis => {
                last.range.end = end;
            }
            _ => self.spans.push(Span {
                range: start..end,
                emphasis,
            }),
        }
    }

    /// Drops trailing whitespace, clipping spans to the text.
    pub fn trim_end(&mut self) {
        let len = self.text.trim_end().len();
        self.text.truncate(len);
        for span in &mut self.spans {
            span.range.end = span.range.end.min(len);
        }
        self.spans.retain(|s| !s.range.is_empty());
        for link in &mut self.links {
            link.range.end = link.range.end.min(len);
        }
        self.links.retain(|l| !l.range.is_empty());
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// One table row; a head row is set bold on a shaded ground.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<Text>,
    pub head: bool,
}

/// Rows of cells in equal columns; rows never split across pages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub rows: Vec<Row>,
}

/// What a block holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Text(Text),
    /// A thematic break, drawn as a hairline.
    Rule,
    /// Never split; `cover` fills the page box instead of the column.
    Image {
        picture: Arc<Picture>,
        cover: bool,
        /// The width the book asked for, if any.
        width: Option<Length>,
    },
    Table(Table),
    /// Blank height in body ems; only its boxes draw, as a book's rules.
    Space(f32),
}

/// A picture's width as a book sets it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// Ems of the reader's text size.
    Em(f32),
    /// A share of the column, 0 to 100.
    Percent(f32),
}

/// Line alignment within the block's column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

/// How a designed chapter's book styles a block (`docs/epub-layout.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// Its chapter is designed: in Auto, the book's look applies.
    pub designed: bool,
    /// Text size in body ems.
    pub size: f32,
    /// A multiple of `size`; none keeps the reader's.
    pub line_height: Option<f32>,
    /// Extra advance per letter and per word, in body ems.
    pub letter: f32,
    pub word: f32,
    /// The first line's indent, in body ems.
    pub indent: f32,
    pub weight: f32,
    /// Families tried before the reader's, the book's own included.
    pub faces: Vec<String>,
    pub justify: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            designed: false,
            size: 1.0,
            line_height: None,
            letter: 0.0,
            word: 0.0,
            indent: 0.0,
            weight: 400.0,
            faces: Vec::new(),
            justify: false,
        }
    }
}

/// A border's line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Edge {
    #[default]
    None,
    Solid,
    Double,
    Dashed,
    Dotted,
}

/// A book's box around blocks, in body ems; sides go top, right, bottom, left.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Boxed {
    /// Left and right margins; none is auto, which centres a set width.
    pub margin: [Option<Length>; 2],
    pub border: [f32; 4],
    pub edges: [Edge; 4],
    pub padding: [f32; 4],
    /// The content's width; none fills what holds it.
    pub width: Option<Length>,
    /// From the top border to its first block, and its last to the bottom.
    pub top: f32,
    pub bottom: f32,
    /// A share of the page's height its content is centred in.
    pub centre: Option<f32>,
}

/// Where a cell's content sits in a taller row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VAlign {
    Top,
    #[default]
    Middle,
    Bottom,
}

/// A row of a book's layout grid: its cells' widths and alignment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridRow {
    /// Each cell's width as set; none shares what is left.
    pub widths: Vec<Option<Length>>,
    pub valign: Vec<VAlign>,
    /// The space above the row, in body ems.
    pub before: f32,
}

/// A block's place in a grid row; the row's blocks sit side by side.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub row: Arc<GridRow>,
    pub column: u16,
    /// How many of the frame's boxes hold the grid; the rest are the cell's.
    pub depth: u8,
}

/// Where a block sits: nesting from lists and quotes, and alignment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    /// Left indent in body ems.
    pub inset: f32,
    /// Quote bars, each at this many body ems from the column's left.
    pub bars: Vec<f32>,
    /// A list marker hung left of the first line, like "•" or "3.".
    pub marker: Option<String>,
    pub align: Align,
    /// A small gap above, as between list items.
    pub tight: bool,
    /// Quoted text, drawn in a softer ink.
    pub soft: bool,
    /// The book's look, used as the Book design setting says.
    pub look: Option<Arc<Look>>,
    /// With a look: the space above, in body ems, margins collapsed.
    pub before: f32,
    /// With a look: the boxes around it, outermost first.
    pub boxes: Vec<Arc<Boxed>>,
    /// With a look: its place in a grid row.
    pub cell: Option<Cell>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub content: Content,
    pub frame: Frame,
    /// Starts a new page, as each chapter of a book does.
    pub break_before: bool,
}

/// `range` of `text`, held to its length and to character edges.
pub fn clip(text: &str, range: Range<usize>) -> Range<usize> {
    let edge = |mut i: usize| {
        i = i.min(text.len());
        while !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let (start, end) = (edge(range.start), edge(range.end));
    start..end.max(start)
}

impl Text {
    /// Bytes `range` of the text, its styles and links kept.
    pub fn cut(&self, range: Range<usize>) -> Self {
        let range = clip(&self.text, range);
        if range == (0..self.text.len()) {
            return self.clone();
        }
        let shift = |r: &Range<usize>| {
            let start = r.start.max(range.start);
            let end = r.end.min(range.end);
            (start < end).then(|| start - range.start..end - range.start)
        };
        let mut out = Self::new(self.role);
        out.text = self.text.get(range.clone()).unwrap_or_default().to_owned();
        out.spans = (self.spans.iter())
            .filter_map(|s| {
                Some(Span {
                    range: shift(&s.range)?,
                    emphasis: s.emphasis,
                })
            })
            .collect();
        out.links = (self.links.iter())
            .filter_map(|l| {
                Some(Link {
                    range: shift(&l.range)?,
                    target: l.target,
                })
            })
            .collect();
        out
    }
}

impl Block {
    /// Bytes of its text or rows of its table; anything else whole.
    pub fn cut(&self, range: Range<usize>) -> Self {
        let content = match &self.content {
            Content::Text(t) => Content::Text(t.cut(range)),
            Content::Table(t) => {
                let rows = t.rows.get(range).map(<[_]>::to_vec).unwrap_or_default();
                Content::Table(Table { rows })
            }
            other => other.clone(),
        };
        Self {
            content,
            frame: self.frame.clone(),
            break_before: false,
        }
    }

    pub fn new(content: Content) -> Self {
        Self {
            content,
            frame: Frame::default(),
            break_before: false,
        }
    }

    /// A text block with an unframed role, for simple callers.
    pub fn text(role: Role, text: &str) -> Self {
        let mut t = Text::new(role);
        t.push(text, Emphasis::default());
        Self::new(Content::Text(t))
    }

    /// Blank text, empty tables: nothing to set.
    pub fn is_blank(&self) -> bool {
        match &self.content {
            Content::Text(t) => t.is_blank(),
            Content::Table(t) => t.rows.iter().all(|r| r.cells.iter().all(Text::is_blank)),
            Content::Rule | Content::Image { .. } | Content::Space(_) => false,
        }
    }

    pub fn is_heading(&self) -> bool {
        matches!(&self.content, Content::Text(t) if t.role.is_heading())
    }

    /// Bytes its layouts are likely to hold once set (`docs/memory.md`).
    pub fn layout_cost(&self) -> usize {
        let cost = |t: &Text| LAYOUT_BYTES.saturating_add(t.text.len().saturating_mul(CHAR_BYTES));
        match &self.content {
            Content::Text(t) => cost(t),
            Content::Table(t) => t.rows.iter().flat_map(|r| &r.cells).map(cost).sum(),
            Content::Rule | Content::Image { .. } | Content::Space(_) => 0,
        }
    }
}

/// What one set paragraph or cell holds before its text, measured.
pub const LAYOUT_BYTES: usize = 3000;
/// What each byte of set text adds, measured.
pub const CHAR_BYTES: usize = 48;

#[cfg(test)]
mod tests {
    use super::*;

    const ITALIC: Emphasis = Emphasis {
        italic: true,
        bold: false,
        code: false,
        size: 0,
    };

    #[test]
    fn neighbouring_runs_of_one_emphasis_share_a_span() {
        let mut b = Text::new(Role::Paragraph);
        b.push("a ", Emphasis::default());
        b.push("bc", ITALIC);
        b.push("d", ITALIC);
        b.push(" e", Emphasis::default());
        assert_eq!(b.text, "a bcd e");
        assert_eq!(
            b.spans,
            [Span {
                range: 2..5,
                emphasis: ITALIC
            }]
        );
    }

    #[test]
    fn trimming_clips_and_drops_spans() {
        let mut b = Text::new(Role::Paragraph);
        b.push("a", Emphasis::default());
        b.push(" b ", ITALIC);
        b.push("  ", ITALIC);
        b.trim_end();
        assert_eq!(b.text, "a b");
        assert_eq!(b.spans.len(), 1);
        assert_eq!(b.spans.first().map(|s| s.range.clone()), Some(1..3));
    }

    #[test]
    fn tables_of_blank_cells_are_blank() {
        let row = |s: &str| Row {
            cells: vec![{
                let mut t = Text::new(Role::Paragraph);
                t.push(s, Emphasis::default());
                t
            }],
            head: false,
        };
        let table = |rows| Block::new(Content::Table(Table { rows }));
        assert!(table(vec![row(" ")]).is_blank());
        assert!(table(vec![]).is_blank());
        assert!(!table(vec![row(" "), row("x")]).is_blank());
        assert!(!Block::new(Content::Rule).is_blank());
    }

    #[test]
    fn each_paragraph_and_cell_costs_a_layout_plus_its_text() {
        let para = Block::text(Role::Paragraph, "four");
        assert_eq!(para.layout_cost(), LAYOUT_BYTES + 4 * CHAR_BYTES);
        let cell = |s: &str| {
            let mut t = Text::new(Role::Paragraph);
            t.push(s, Emphasis::default());
            t
        };
        let row = Row {
            cells: vec![cell("ab"), cell("")],
            head: false,
        };
        let table = Block::new(Content::Table(Table { rows: vec![row] }));
        assert_eq!(table.layout_cost(), 2 * LAYOUT_BYTES + 2 * CHAR_BYTES);
        assert_eq!(Block::new(Content::Rule).layout_cost(), 0);
    }
}
