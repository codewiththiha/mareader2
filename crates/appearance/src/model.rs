//! Field names and defaults match mareader, so its saved presets load.

use serde::{Deserialize, Serialize};

/// The structural half of a look: the filter family and blend direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BaseMode {
    /// Page untouched, textures multiply.
    #[default]
    Light,
    /// Page inverted, textures screen.
    Dark,
    /// Page dimmed but not inverted: figures keep their real colours.
    Dim,
}

impl BaseMode {
    pub const ALL: [Self; 3] = [Self::Light, Self::Dark, Self::Dim];

    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::Dim => "Dim",
        }
    }

    /// Dim reads the dark UI palette even though its page is not inverted.
    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark | Self::Dim)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextureMode {
    #[default]
    None,
    Paper,
    Lined,
    Grid,
    Dotted,
    Cross,
}

impl TextureMode {
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Paper,
        Self::Lined,
        Self::Grid,
        Self::Dotted,
        Self::Cross,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Paper => "Real paper",
            Self::Lined => "Lined",
            Self::Grid => "Grid",
            Self::Dotted => "Dotted",
            Self::Cross => "Cross",
        }
    }
}

/// Film grain: off, still, or crawling in eight steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoiseMode {
    #[default]
    Off,
    Static,
    Animated,
}

/// A complete look: what a preset stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub base: BaseMode,
    /// Degrees on the sRGB hue circle, 0..360.
    pub tint_hue: u16,
    /// 0..=100; 0 short-circuits every tint path.
    pub tint_strength: u8,
    pub texture: TextureMode,
    /// 0..=100.
    pub texture_opacity: u8,
    /// Percent of the natural pitch, 25..=400.
    pub texture_scale: u16,
    pub noise: NoiseMode,
    /// 0..=100.
    pub noise_intensity: u8,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            base: BaseMode::Light,
            tint_hue: 34,
            tint_strength: 0,
            texture: TextureMode::None,
            texture_opacity: 90,
            texture_scale: 100,
            noise: NoiseMode::Off,
            noise_intensity: 25,
        }
    }
}

impl Appearance {
    /// Clamps every dial into range; call after deserialising user data.
    pub fn sanitize(&mut self) {
        self.tint_hue %= 360;
        self.tint_strength = self.tint_strength.min(100);
        self.texture_opacity = self.texture_opacity.min(100);
        self.texture_scale = self.texture_scale.clamp(25, 400);
        self.noise_intensity = self.noise_intensity.min(100);
    }

    pub fn has_tint(&self) -> bool {
        self.tint_strength > 0
    }

    /// The tint's effect, 0..=1. Full effect lands at strength 50.
    pub fn tint_amount(&self) -> f64 {
        (f64::from(self.tint_strength) / 50.0).min(1.0)
    }

    /// Whether going to `to` is a jump worth easing, not a dial's step.
    pub fn leaps_to(&self, to: &Self) -> bool {
        let hue = self.tint_hue.abs_diff(to.tint_hue) % 360;
        let tinted = self.has_tint() && to.has_tint();
        self.base != to.base
            || self.texture != to.texture
            || self.noise != to.noise
            || self.tint_strength.abs_diff(to.tint_strength) >= LEAP_STRENGTH
            || (tinted && hue.min(360 - hue) >= LEAP_HUE)
    }
}

/// Smaller moves than these are a dial's steps.
const LEAP_STRENGTH: u8 = 15;
const LEAP_HUE: u16 = 30;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_leap_and_dial_steps_do_not() {
        let a = Appearance {
            tint_strength: 30,
            ..Appearance::default()
        };
        let step = Appearance {
            tint_hue: a.tint_hue + 5,
            tint_strength: 32,
            texture_scale: 150,
            ..a
        };
        assert!(!a.leaps_to(&step), "a slider's step");
        let dark = Appearance {
            base: BaseMode::Dark,
            ..a
        };
        let swatch = Appearance { tint_hue: 200, ..a };
        let grid = Appearance {
            texture: TextureMode::Grid,
            ..a
        };
        for to in [dark, swatch, grid] {
            assert!(a.leaps_to(&to), "{to:?}");
        }
        let across_zero = Appearance { tint_hue: 350, ..a };
        let near = Appearance { tint_hue: 5, ..a };
        assert!(!across_zero.leaps_to(&near), "15 degrees round the circle");
    }

    #[test]
    fn sanitize_clamps_every_range() {
        let mut a = Appearance {
            tint_hue: 725,
            tint_strength: 200,
            texture_opacity: 240,
            texture_scale: 5000,
            noise_intensity: 199,
            ..Default::default()
        };
        a.sanitize();
        assert_eq!(
            (a.tint_hue, a.tint_strength, a.texture_opacity),
            (5, 100, 100)
        );
        assert_eq!((a.texture_scale, a.noise_intensity), (400, 100));
        let mut small = Appearance {
            texture_scale: 1,
            ..Default::default()
        };
        small.sanitize();
        assert_eq!(small.texture_scale, 25);
    }

    #[test]
    fn mareader_json_reads_unchanged() {
        let json = r#"{"base":"dim","tint_hue":220,"tint_strength":15,"noise":"animated"}"#;
        let a: Appearance = serde_json::from_str(json).unwrap();
        assert_eq!(a.base, BaseMode::Dim);
        assert_eq!(a.noise, NoiseMode::Animated);
        assert_eq!(a.texture_opacity, 90, "missing fields default");
    }

    #[test]
    fn the_dial_saturates_half_way() {
        let at = |s| {
            Appearance {
                tint_strength: s,
                ..Default::default()
            }
            .tint_amount()
        };
        assert_eq!(at(0), 0.0);
        assert_eq!(at(50), 1.0);
        assert_eq!(at(100), 1.0);
    }
}
