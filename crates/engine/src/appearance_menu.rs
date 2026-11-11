//! The quick text dials the appearance popover offers.

use mareader_settings::TextSettings;

/// The text dials the popover offers for a book, from its text settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuickText {
    pub size_pct: f32,
    pub line_height: f32,
    pub paragraph_margin: f32,
    pub override_font: bool,
    pub override_layout: bool,
}

impl QuickText {
    pub fn of(t: &TextSettings) -> Self {
        Self {
            size_pct: t.size_pct(),
            line_height: t.line_height,
            paragraph_margin: t.paragraph_margin,
            override_font: t.override_font,
            override_layout: t.override_layout,
        }
    }

    /// Writes the dials back; an untouched size keeps its exact points.
    pub fn apply(self, t: &mut TextSettings) {
        if self.size_pct != t.size_pct() {
            t.set_size_pct(self.size_pct);
        }
        t.line_height = self.line_height;
        t.paragraph_margin = self.paragraph_margin;
        t.override_font = self.override_font;
        t.override_layout = self.override_layout;
    }
}
