//! What a book says, format-free: blocks of text with inline emphasis.
use std::ops::Range;

/// A block's part in the text; it picks the face, size and spacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Level 1 to 6, as in HTML.
    Heading(u8),
    Paragraph,
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
}

/// `range` is in bytes of the block's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub emphasis: Emphasis,
}

/// One paragraph or heading; spans cover only emphasised text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub role: Role,
    pub text: String,
    pub spans: Vec<Span>,
    /// Starts a new page, as each chapter of a book does.
    pub break_before: bool,
}

impl Block {
    pub fn new(role: Role) -> Self {
        Self {
            role,
            text: String::new(),
            spans: Vec::new(),
            break_before: false,
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
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITALIC: Emphasis = Emphasis {
        italic: true,
        bold: false,
    };

    #[test]
    fn neighbouring_runs_of_one_emphasis_share_a_span() {
        let mut b = Block::new(Role::Paragraph);
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
        let mut b = Block::new(Role::Paragraph);
        b.push("a", Emphasis::default());
        b.push(" b ", ITALIC);
        b.push("  ", ITALIC);
        b.trim_end();
        assert_eq!(b.text, "a b");
        assert_eq!(b.spans.len(), 1);
        assert_eq!(b.spans.first().map(|s| s.range.clone()), Some(1..3));
    }
}
