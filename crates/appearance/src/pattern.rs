//! Paper textures: mareader's five patterns, laid over the themed page.

use crate::filter::Blend;
use crate::model::{Appearance, TextureMode};

// A unit draws as zoom times the user's scale, in points.

/// Lines repeat every 26 units (mareader's `--tex-unit`).
pub const LINE_PITCH: f32 = 26.0;
/// Dots repeat every 8 units.
pub const DOT_PITCH: f32 = 8.0;
/// Grain noise per unit (`feTurbulence baseFrequency`).
pub const GRAIN_FREQUENCY: f32 = 0.65;
/// The noise layer's own opacity in mareader's grain tile.
pub const GRAIN_ALPHA: f32 = 0.055;

/// One page's texture: which pattern, and its colours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pattern {
    pub mode: TextureMode,
    /// The whole layer's opacity, 0..1.
    pub opacity: f32,
    /// The user's scale: 1 draws a unit as one point at zoom 1.
    pub scale: f32,
    /// Lines and dots: sRGB with straight alpha.
    pub stroke: [f32; 4],
    /// The flat tint under the grain noise.
    pub grain: [f32; 4],
    /// How the noise blends into its tint: multiply, or screen when dark.
    pub grain_blend: Blend,
}

impl Pattern {
    pub const NONE: Self = Self {
        mode: TextureMode::None,
        opacity: 0.0,
        scale: 1.0,
        stroke: [0.0; 4],
        grain: [0.0; 4],
        grain_blend: Blend::Multiply,
    };

    /// Stroke width in units on a page at `zoom` points per page point.
    pub fn line_width(&self, zoom: f32) -> f32 {
        // mareader: clamp(0.5px, 1px * scale, 2.5px), in screen pixels.
        let px = (zoom * self.scale).max(1e-3);
        px.clamp(0.5, 2.5) / px
    }

    /// The grain layer where the noise reads grey `g` and alpha `n`.
    pub fn grain_layer(&self, g: f32, n: f32) -> [f32; 4] {
        let [r, gr, b, ab] = self.grain;
        let tint = [r, gr, b];
        let ai = GRAIN_ALPHA * n;
        // background-blend-mode: the noise blends into the tint beneath it.
        let mixed = self.grain_blend.apply(tint, [g; 3]);
        let a = ai + ab * (1.0 - ai);
        if a <= 0.0 {
            return [0.0; 4];
        }
        let c = |i: usize| {
            let (t, m) = (tint.get(i).copied(), mixed.get(i).copied());
            let (t, m) = (t.unwrap_or(0.0), m.unwrap_or(0.0));
            let src = (1.0 - ab) * g + ab * m;
            (ai * src + ab * (1.0 - ai) * t) / a
        };
        [c(0), c(1), c(2), a]
    }

    /// `layer` over the opaque page colour `c`, at the layer's opacity.
    pub fn over(&self, c: [f32; 3], layer: [f32; 4]) -> [f32; 3] {
        let a = layer[3] * self.opacity;
        [0, 1, 2].map(|i| {
            let (c, l) = (c.get(i).copied(), layer.get(i).copied());
            c.unwrap_or(0.0) * (1.0 - a) + l.unwrap_or(0.0) * a
        })
    }
}

impl Appearance {
    /// The page texture, in mareader's colours for this base.
    pub fn pattern(&self) -> Pattern {
        let dark = self.base.is_dark();
        let slate = [15.0, 23.0, 42.0].map(|c| c / 255.0);
        let (stroke, grain, grain_blend) = if dark {
            ([1.0, 1.0, 1.0, 0.22], [1.0, 1.0, 1.0, 0.08], Blend::Screen)
        } else {
            let [r, g, b] = slate;
            ([r, g, b, 0.16], [r, g, b, 0.05], Blend::Multiply)
        };
        Pattern {
            mode: self.texture,
            opacity: f32::from(self.texture_opacity.min(100)) / 100.0,
            scale: f32::from(self.texture_scale.clamp(25, 400)) / 100.0,
            stroke,
            grain,
            grain_blend,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BaseMode;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.004)
    }

    #[test]
    fn strokes_stay_between_half_a_pixel_and_two_and_a_half() {
        let p = Appearance::default().pattern();
        // At zoom 1 a unit is a point, so the stroke is one point.
        assert!((p.line_width(1.0) - 1.0).abs() < 1e-6);
        // Zoomed far out, a hairline would vanish: half a pixel holds.
        assert!((p.line_width(0.25) * 0.25 - 0.5).abs() < 1e-6);
        // Zoomed far in, strokes stop thickening at 2.5 pixels.
        assert!((p.line_width(4.0) * 4.0 - 2.5).abs() < 1e-6);
    }

    #[test]
    fn a_stroke_darkens_light_paper_and_lightens_dark_paper() {
        let light = Appearance {
            texture: TextureMode::Lined,
            ..Default::default()
        };
        let p = light.pattern();
        let on = p.over([1.0; 3], p.stroke);
        // 0.16 slate at 0.9 opacity over white.
        let want = [15.0, 23.0, 42.0].map(|c: f32| 1.0 - 0.144 + c / 255.0 * 0.144);
        assert!(close(on, want), "{on:?}");
        let dark = Appearance {
            base: BaseMode::Dark,
            ..light
        };
        let p = dark.pattern();
        let on = p.over([0.1; 3], p.stroke);
        // 0.22 white at 0.9 opacity over 0.1 grey.
        assert!(close(on, [0.1 * (1.0 - 0.198) + 0.198; 3]), "{on:?}");
    }

    #[test]
    fn no_texture_or_no_opacity_leaves_the_page() {
        let none = Appearance::default().pattern();
        assert_eq!(none.mode, TextureMode::None);
        let clear = Appearance {
            texture: TextureMode::Grid,
            texture_opacity: 0,
            ..Default::default()
        }
        .pattern();
        assert!(close(clear.over([0.3; 3], clear.stroke), [0.3; 3]));
    }

    #[test]
    fn grain_tints_light_paper_faintly_darker() {
        let p = Appearance {
            texture: TextureMode::Paper,
            ..Default::default()
        }
        .pattern();
        let mid = p.over([1.0; 3], p.grain_layer(0.5, 0.5));
        // A faint slate cast, about 14 levels, bluest in blue.
        assert!(mid.iter().all(|&c| c < 0.96 && c > 0.93), "{mid:?}");
        assert!(mid[2] > mid[0]);
        // Brighter noise darkens less under multiply.
        let bright = p.over([1.0; 3], p.grain_layer(1.0, 0.5));
        assert!(bright[0] > mid[0]);
    }
}
