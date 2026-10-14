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
    },
    Table(Table),
}

/// Line alignment within the block's column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub content: Content,
    pub frame: Frame,
    /// Starts a new page, as each chapter of a book does.
    pub break_before: bool,
}

impl Block {
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
            Content::Rule | Content::Image { .. } => false,
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
            Content::Rule | Content::Image { .. } => 0,
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
