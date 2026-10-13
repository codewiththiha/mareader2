//! Reading aids: word glosses, CEFR highlighting and dictionaries.

use mareader_appearance::Rgb;
use serde::{Deserialize, Serialize};

use crate::{Swatch, clamp};

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

/// How glossed words are highlighted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlossSettings {
    pub color: Swatch,
    pub custom: Rgb,
    pub opacity: f32,
    pub density: GlossDensity,
}

impl GlossSettings {
    pub const OPACITY: (f32, f32) = (0.1, 1.0);

    pub fn sanitize(&mut self) {
        let (lo, hi) = Self::OPACITY;
        self.opacity = clamp(self.opacity, lo, hi, Self::default().opacity);
    }
}

impl Default for GlossSettings {
    fn default() -> Self {
        Self {
            color: Swatch::Auto,
            custom: Rgb(0xa5, 0x8a, 0xf0),
            opacity: 0.4,
            density: GlossDensity::Compact,
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
    pub cefr: CefrSettings,
}

impl StudySettings {
    pub fn sanitize(&mut self) {
        self.gloss.sanitize();
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
