//! Reading aids: word glosses, CEFR highlighting and dictionaries.

use mareader_appearance::Rgb;
use serde::{Deserialize, Serialize};

pub use mareader_library::Pen;

use crate::clamp;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlossDensity {
    #[default]
    Compact,
    Comfortable,
}

impl GlossDensity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Comfortable => "Comfortable",
        }
    }
}

/// The word card's spacing; AI marks are drawn as `HighlightSettings::ai`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlossSettings {
    pub density: GlossDensity,
}

/// A button the selection menu can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarItem {
    Copy,
    Explain,
    Highlight(Pen),
}

impl BarItem {
    /// Every button, in the order a new menu lists them.
    pub const ALL: [BarItem; 5] = [
        BarItem::Copy,
        BarItem::Explain,
        BarItem::Highlight(Pen::Full),
        BarItem::Highlight(Pen::Underline),
        BarItem::Highlight(Pen::Half),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::Explain => "Explain",
            Self::Highlight(pen) => pen.label(),
        }
    }
}

/// What a highlighter button does when pressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickColor {
    /// Highlights at once in the pen's colour, which later edits follow.
    #[default]
    Default,
    /// Shows a row of colours to pick from each time.
    Ask,
}

/// A look kept for one purpose, so no highlight can pass for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reserved {
    pub pen: Pen,
    pub color: Rgb,
}

/// The three highlighters, the selection menu and the reserved looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HighlightSettings {
    pub full: Rgb,
    pub half: Rgb,
    pub underline: Rgb,
    /// The fills' strength; underlines are drawn solid.
    pub opacity: f32,
    /// The selection menu's buttons, in order; the rest are hidden.
    pub bar: Vec<BarItem>,
    pub pick: PickColor,
    /// How words the AI explained are marked.
    pub ai: Reserved,
}

impl HighlightSettings {
    pub const OPACITY: (f32, f32) = (0.2, 0.8);
    /// The colours offered.
    pub const PALETTE: [Rgb; 8] = [
        Rgb(0xff, 0xff, 0x00),
        Rgb(0xff, 0xbe, 0x00),
        Rgb(0xff, 0x00, 0x00),
        Rgb(0x83, 0xd3, 0x3c),
        Rgb(0x00, 0xb2, 0xf4),
        Rgb(0x16, 0x77, 0xff),
        Rgb(0xa5, 0x8a, 0xf0),
        Rgb(0xff, 0x00, 0xff),
    ];

    /// `pen`'s own colour.
    pub fn color(&self, pen: Pen) -> Rgb {
        match pen {
            Pen::Full => self.full,
            Pen::Half => self.half,
            Pen::Underline => self.underline,
        }
    }

    pub fn color_mut(&mut self, pen: Pen) -> &mut Rgb {
        match pen {
            Pen::Full => &mut self.full,
            Pen::Half => &mut self.half,
            Pen::Underline => &mut self.underline,
        }
    }

    /// Looks kept for other purposes: the AI's now, CEFR's in slice 5.
    pub fn reserved(&self) -> [Reserved; 1] {
        [self.ai]
    }

    /// Whether the reader may highlight with `pen` in `color`.
    pub fn allowed(&self, pen: Pen, color: Rgb) -> bool {
        !self.reserved().contains(&Reserved { pen, color })
    }

    /// The colours `pen` may take: its own first, then the palette's.
    pub fn colors(&self, pen: Pen) -> Vec<Rgb> {
        let own = self.color(pen);
        let mut out = vec![own];
        out.extend(Self::PALETTE.into_iter().filter(|c| *c != own));
        out.retain(|c| self.allowed(pen, *c));
        out
    }

    pub fn sanitize(&mut self) {
        let (lo, hi) = Self::OPACITY;
        self.opacity = clamp(self.opacity, lo, hi, Self::default().opacity);
        let mut seen = Vec::with_capacity(self.bar.len());
        self.bar.retain(|item| {
            let new = !seen.contains(item);
            seen.push(*item);
            new
        });
        // A pen whose colour became reserved moves to the next one free.
        for pen in Pen::ALL {
            if !self.allowed(pen, self.color(pen))
                && let Some(free) = self.colors(pen).first().copied()
            {
                *self.color_mut(pen) = free;
            }
        }
    }
}

impl Default for HighlightSettings {
    fn default() -> Self {
        Self {
            full: Rgb(0xff, 0xff, 0x00),
            half: Rgb(0x83, 0xd3, 0x3c),
            underline: Rgb(0x16, 0x77, 0xff),
            opacity: 0.5,
            bar: vec![
                BarItem::Copy,
                BarItem::Explain,
                BarItem::Highlight(Pen::Full),
                BarItem::Highlight(Pen::Underline),
            ],
            pick: PickColor::Default,
            ai: Reserved {
                pen: Pen::Full,
                color: Rgb(0xa5, 0x8a, 0xf0),
            },
        }
    }
}

/// The reader's level; words above it get highlighted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CefrLevel {
    A2,
    B1,
    #[default]
    B2,
    C1,
    C2,
}

impl CefrLevel {
    pub const ALL: [CefrLevel; 5] = [Self::A2, Self::B1, Self::B2, Self::C1, Self::C2];

    /// The word-list band at this level; words above it light up.
    pub fn band(self) -> u8 {
        match self {
            Self::A2 => 2,
            Self::B1 => 3,
            Self::B2 => 4,
            Self::C1 => 5,
            Self::C2 => 6,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::A2 => "A2",
            Self::B1 => "B1",
            Self::B2 => "B2",
            Self::C1 => "C1",
            Self::C2 => "C2",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::A2 => "Highlights B1 and above",
            Self::B1 => "Highlights B2 and above",
            Self::B2 => "Highlights C1 and above",
            Self::C1 => "Highlights C2",
            Self::C2 => "Nothing left above C2",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CefrSettings {
    pub enabled: bool,
    pub level: CefrLevel,
    /// Clicking a highlighted word explains it.
    pub click_explain: bool,
}

impl Default for CefrSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            level: CefrLevel::B2,
            click_explain: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudySettings {
    pub gloss: GlossSettings,
    pub highlights: HighlightSettings,
    pub cefr: CefrSettings,
}

impl StudySettings {
    pub fn sanitize(&mut self) {
        self.highlights.sanitize();
    }
}

/// Which dictionaries look words up, and how.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DictionarySettings {
    /// Looks a word up on hover rather than only on click.
    pub hover: bool,
    pub default_lang: Option<String>,
    /// Installed dictionary ids, in lookup order.
    pub langs: Vec<String>,
    /// Translate between a fixed `from` and `to` language.
    pub pair: bool,
    pub from: Option<String>,
    pub to: Option<String>,
}

impl Default for DictionarySettings {
    fn default() -> Self {
        Self {
            hover: true,
            default_lang: None,
            langs: Vec::new(),
            pair: false,
            from: None,
            to: None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_ai_look_is_never_offered_to_its_pen() {
        let h = HighlightSettings::default();
        let violet = h.ai.color;
        assert!(!h.colors(Pen::Full).contains(&violet));
        assert!(!h.allowed(Pen::Full, violet));
        // Another pen may still use the colour: the pair is what is kept.
        assert!(h.colors(Pen::Underline).contains(&violet));
    }

    #[test]
    fn a_pen_on_a_newly_reserved_colour_moves_to_a_free_one() {
        let mut h = HighlightSettings::default();
        h.ai = Reserved {
            pen: Pen::Full,
            color: h.full,
        };
        h.sanitize();
        assert_ne!(h.full, h.ai.color);
        assert!(h.allowed(Pen::Full, h.full));
    }

    #[test]
    fn the_menu_keeps_each_button_once_and_bad_values_are_bounded() {
        let mut h = HighlightSettings {
            bar: vec![BarItem::Copy, BarItem::Highlight(Pen::Half), BarItem::Copy],
            opacity: f32::NAN,
            ..HighlightSettings::default()
        };
        h.sanitize();
        assert_eq!(h.bar, [BarItem::Copy, BarItem::Highlight(Pen::Half)]);
        assert!((h.opacity - 0.5).abs() < f32::EPSILON);
        let saved = serde_json::to_string(&h).unwrap();
        assert!(saved.contains(r#"{"highlight":"half"}"#), "{saved}");
        let back: HighlightSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!(back, h);
    }
}
