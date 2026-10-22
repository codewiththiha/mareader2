//! EPUB typography: sizes, spacing and the four font slots.

use serde::{Deserialize, Serialize};

use crate::clamp;

/// The generic family a face belongs to; drives fallbacks and grouping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontFamily {
    Serif,
    Sans,
    Mono,
}

macro_rules! system_fonts {
    ($($v:ident, $id:literal, $label:literal, $fam:ident, [$($name:literal),+];)+) => {
        /// A face the OS usually has; the id is what settings store.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum SystemFont { $($v),+ }

        impl SystemFont {
            pub const ALL: &'static [SystemFont] = &[$(SystemFont::$v),+];

            pub fn id(self) -> &'static str {
                match self { $(SystemFont::$v => $id),+ }
            }

            pub fn label(self) -> &'static str {
                match self { $(SystemFont::$v => $label),+ }
            }

            pub fn family(self) -> FontFamily {
                match self { $(SystemFont::$v => FontFamily::$fam),+ }
            }

            /// Family names to try in order; CSS generics like `serif` allowed.
            pub fn names(self) -> &'static [&'static str] {
                match self { $(SystemFont::$v => &[$($name),+]),+ }
            }
        }
    };
}

system_fonts! {
    UiSerif, "ui-serif", "System Serif", Serif, ["serif"];
    UiSans, "ui-sans", "System Sans", Sans, ["sans-serif"];
    UiMono, "ui-mono", "System Mono", Mono, ["monospace"];
    Georgia, "georgia", "Georgia", Serif, ["Georgia"];
    TimesNewRoman, "times-new-roman", "Times New Roman", Serif, ["Times New Roman"];
    Palatino, "palatino", "Palatino", Serif, ["Palatino", "Palatino Linotype"];
    Garamond, "garamond", "Garamond", Serif, ["Garamond", "EB Garamond"];
    Baskerville, "baskerville", "Baskerville", Serif, ["Baskerville", "Baskerville Old Face"];
    Charter, "charter", "Charter", Serif, ["Charter", "Bitstream Charter"];
    Arial, "arial", "Arial", Sans, ["Arial"];
    Helvetica, "helvetica", "Helvetica", Sans, ["Helvetica", "Helvetica Neue"];
    Verdana, "verdana", "Verdana", Sans, ["Verdana"];
    TrebuchetMs, "trebuchet-ms", "Trebuchet MS", Sans, ["Trebuchet MS"];
    Tahoma, "tahoma", "Tahoma", Sans, ["Tahoma"];
    GillSans, "gill-sans", "Gill Sans", Sans, ["Gill Sans", "Gill Sans MT"];
    CourierNew, "courier-new", "Courier New", Mono, ["Courier New"];
    Menlo, "menlo", "Menlo", Mono, ["Menlo"];
    Consolas, "consolas", "Consolas", Mono, ["Consolas"];
    Monaco, "monaco", "Monaco", Mono, ["Monaco"];
}

impl SystemFont {
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|f| f.id() == id)
    }
}

/// One font slot's pick; stored as `default`, `system:<id>` or `bundled:<id>`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FontChoice {
    /// The book's own CSS decides.
    #[default]
    Default,
    System(SystemFont),
    /// A face shipped with the app; unknown ids render as `Default`.
    Bundled(String),
}

impl FontChoice {
    /// Unknown or malformed values fall back to `Default`, never fail.
    pub fn parse(s: &str) -> Self {
        if let Some(id) = s.strip_prefix("system:") {
            return SystemFont::from_id(id).map_or(Self::Default, Self::System);
        }
        // mareader wrote `builtin:`; both mean a bundled face.
        let bundled = s.strip_prefix("bundled:").or(s.strip_prefix("builtin:"));
        match bundled {
            Some(id) if !id.is_empty() => Self::Bundled(id.to_owned()),
            _ => Self::Default,
        }
    }

    pub fn store(&self) -> String {
        match self {
            Self::Default => "default".to_owned(),
            Self::System(f) => format!("system:{}", f.id()),
            Self::Bundled(id) => format!("bundled:{id}"),
        }
    }
}

impl Serialize for FontChoice {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.store())
    }
}

impl<'de> Deserialize<'de> for FontChoice {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::parse(&String::deserialize(d)?))
    }
}

/// Where the text column sits when it's narrower than the pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnAlign {
    Left,
    #[default]
    Center,
    Right,
}

/// When a book's own fonts, sizes and layout show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BookDesign {
    /// The book's type everywhere; its layout where it designs a page.
    #[default]
    Auto,
    On,
    Off,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextSettings {
    /// Two-page spreads instead of one scrolling column.
    pub book_layout: bool,
    /// Space between paragraphs, in ems.
    pub paragraph_margin: f32,
    pub line_height: f32,
    /// Extra space between words, in points.
    pub word_spacing: f32,
    /// Extra space between letters, in ems.
    pub letter_spacing: f32,
    /// First-line indent, in ems.
    pub text_indent: f32,
    pub justify: bool,
    pub hyphenation: bool,
    /// The standard text size in points, shown as a share of the base.
    pub font_size: f32,
    /// The smallest a book's text sets, in points.
    pub min_font_size: f32,
    /// The reader's fonts and size for all of a book's text, over its own.
    pub override_font: bool,
    /// The reader's spacing, indent and justification over a book's own.
    pub override_layout: bool,
    pub font_weight: f32,
    pub default_font: FontChoice,
    pub serif_font: FontChoice,
    pub sans_font: FontChoice,
    pub mono_font: FontChoice,
    pub column_align: ColumnAlign,
    /// A book's text column, in percent of its usual width.
    pub column_width_pct: f32,
    /// A paged book's margins above and below its text, in points.
    pub page_margin_y: f32,
    /// Its margins beside the text, in points.
    pub page_margin_x: f32,
    /// The longest a paged book's line runs, in points.
    pub max_line_width: f32,
    /// Percent of full ink contrast; lower softens text toward the paper.
    pub ink_contrast: f32,
    /// A book's pictures take the page's theme; off, their own colours.
    pub theme_pictures: bool,
    /// Emoji take the page's theme too; off, their own colours.
    pub theme_emojis: bool,
    pub book_design: BookDesign,
}

impl TextSettings {
    pub const PARAGRAPH: (f32, f32) = (0.0, 3.0);
    pub const LINE_HEIGHT: (f32, f32) = (1.0, 3.0);
    pub const WORD_SPACING: (f32, f32) = (-2.0, 10.0);
    pub const LETTER_SPACING: (f32, f32) = (-0.02, 0.3);
    pub const INDENT: (f32, f32) = (0.0, 4.0);
    pub const FONT_SIZE: (f32, f32) = (8.5, 34.0);
    /// The size shown as 100 %.
    pub const BASE_SIZE: f32 = 17.0;
    /// Text size as a share of the base, in percent.
    pub const SIZE_PCT: (f32, f32) = (50.0, 200.0);
    pub const MIN_FONT: (f32, f32) = (6.0, 24.0);
    pub const FONT_WEIGHT: (f32, f32) = (100.0, 900.0);
    /// The ink dial's floor is mareader's: text never fades out.
    pub const INK: (f32, f32) = (10.0, 100.0);
    pub const COLUMN_PCT: (f32, f32) = (60.0, 140.0);
    pub const PAGE_MARGIN: (f32, f32) = (0.0, 160.0);
    pub const MAX_LINE: (f32, f32) = (320.0, 1600.0);

    /// The text size in percent of the base, to the nearest step.
    pub fn size_pct(&self) -> f32 {
        (self.font_size / Self::BASE_SIZE * 100.0).round()
    }

    pub fn set_size_pct(&mut self, pct: f32) {
        let (lo, hi) = Self::SIZE_PCT;
        self.font_size = pct.clamp(lo, hi) * Self::BASE_SIZE / 100.0;
    }

    pub fn sanitize(&mut self) {
        let d = Self::default();
        let fit = |v: f32, (lo, hi): (f32, f32), fallback| clamp(v, lo, hi, fallback);
        self.paragraph_margin = fit(self.paragraph_margin, Self::PARAGRAPH, d.paragraph_margin);
        self.line_height = fit(self.line_height, Self::LINE_HEIGHT, d.line_height);
        self.word_spacing = fit(self.word_spacing, Self::WORD_SPACING, d.word_spacing);
        self.letter_spacing = fit(self.letter_spacing, Self::LETTER_SPACING, d.letter_spacing);
        self.text_indent = fit(self.text_indent, Self::INDENT, d.text_indent);
        self.font_size = fit(self.font_size, Self::FONT_SIZE, d.font_size);
        self.min_font_size = fit(self.min_font_size, Self::MIN_FONT, d.min_font_size);
        self.font_weight = fit(self.font_weight, Self::FONT_WEIGHT, d.font_weight);
        self.ink_contrast = fit(self.ink_contrast, Self::INK, d.ink_contrast);
        let column = self.column_width_pct;
        self.column_width_pct = fit(column, Self::COLUMN_PCT, d.column_width_pct);
        let margin = Self::PAGE_MARGIN;
        self.page_margin_y = fit(self.page_margin_y, margin, d.page_margin_y);
        self.page_margin_x = fit(self.page_margin_x, margin, d.page_margin_x);
        self.max_line_width = fit(self.max_line_width, Self::MAX_LINE, d.max_line_width);
    }
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            book_layout: false,
            paragraph_margin: 1.0,
            line_height: 1.7,
            word_spacing: 0.0,
            letter_spacing: 0.0,
            text_indent: 0.0,
            justify: false,
            hyphenation: false,
            font_size: 17.0,
            min_font_size: 8.0,
            override_font: false,
            override_layout: false,
            font_weight: 400.0,
            default_font: FontChoice::Default,
            serif_font: FontChoice::Default,
            sans_font: FontChoice::Default,
            mono_font: FontChoice::Default,
            column_align: ColumnAlign::Center,
            column_width_pct: 100.0,
            page_margin_y: 44.0,
            page_margin_x: 32.0,
            max_line_width: 720.0,
            ink_contrast: 100.0,
            theme_pictures: false,
            theme_emojis: false,
            book_design: BookDesign::Auto,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_size_reads_as_a_share_of_the_base() {
        let mut t = TextSettings::default();
        assert_eq!(t.size_pct(), 100.0);
        t.set_size_pct(150.0);
        assert_eq!((t.font_size, t.size_pct()), (25.5, 150.0));
        // Old sizes in points read as the nearest percent.
        t.font_size = 21.0;
        assert_eq!(t.size_pct(), 124.0);
        t.set_size_pct(900.0);
        assert_eq!(t.size_pct(), 200.0);
    }

    #[test]
    fn font_choices_parse_leniently() {
        let georgia = FontChoice::System(SystemFont::Georgia);
        assert_eq!(FontChoice::parse("system:georgia"), georgia);
        assert_eq!(FontChoice::parse("system:comic-sans"), FontChoice::Default);
        assert_eq!(FontChoice::parse("builtin:lora").store(), "bundled:lora");
        assert_eq!(FontChoice::parse("bundled:"), FontChoice::Default);
        assert_eq!(FontChoice::parse("whatever"), FontChoice::Default);
    }
}
