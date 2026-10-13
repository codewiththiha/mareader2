//! The shared colour palette for outlines and gloss highlights.

use mareader_appearance::Rgb;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Swatch {
    /// Follows the accent, so it tracks the theme's tint.
    #[default]
    #[serde(alias = "accent")]
    Auto,
    Red,
    Yellow,
    Green,
    Blue,
    #[serde(alias = "violet")]
    Custom,
}

impl Swatch {
    /// The fixed swatches, in menu order; `Custom` has its own picker.
    pub const PALETTE: [Swatch; 5] = [
        Swatch::Auto,
        Swatch::Red,
        Swatch::Yellow,
        Swatch::Green,
        Swatch::Blue,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Swatch::Auto => "Auto",
            Swatch::Red => "Red",
            Swatch::Yellow => "Yellow",
            Swatch::Green => "Green",
            Swatch::Blue => "Blue",
            Swatch::Custom => "Custom",
        }
    }

    /// The colour, or `None` to follow the theme's accent.
    pub fn rgb(self, custom: Rgb) -> Option<Rgb> {
        Some(match self {
            Swatch::Auto => return None,
            Swatch::Red => Rgb(0xe5, 0x6b, 0x64),
            Swatch::Yellow => Rgb(0xe8, 0xc4, 0x49),
            Swatch::Green => Rgb(0x6f, 0xd5, 0x8c),
            Swatch::Blue => Rgb(0x6b, 0xa3, 0xf5),
            Swatch::Custom => custom,
        })
    }
}
