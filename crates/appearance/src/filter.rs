//! The page colour pipeline as matrices; the GPU pass is tested against it.

use crate::model::{Appearance, BaseMode};

/// The page shader's uniform array has exactly this many slots.
pub const MAX_FILTER_OPS: usize = 7;

/// One CSS filter function, with the Filter Effects spec semantics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterOp {
    Invert(f32),
    /// Degrees.
    HueRotate(f32),
    Saturate(f32),
    Brightness(f32),
    Contrast(f32),
    Sepia(f32),
}

/// Rows are output r, g, b; columns are input r, g, b and a constant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorMatrix(pub [[f32; 4]; 3]);

impl ColorMatrix {
    fn linear(m: [[f32; 3]; 3]) -> Self {
        let [r, g, b] = m;
        Self([
            [r[0], r[1], r[2], 0.0],
            [g[0], g[1], g[2], 0.0],
            [b[0], b[1], b[2], 0.0],
        ])
    }

    fn scale_offset(scale: f32, offset: f32) -> Self {
        Self([
            [scale, 0.0, 0.0, offset],
            [0.0, scale, 0.0, offset],
            [0.0, 0.0, scale, offset],
        ])
    }

    /// Applies the matrix and clamps, as CSS does after every function.
    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        self.0
            .map(|row| (row[0] * c[0] + row[1] * c[1] + row[2] * c[2] + row[3]).clamp(0.0, 1.0))
    }
}

impl FilterOp {
    pub fn matrix(self) -> ColorMatrix {
        match self {
            Self::Invert(a) => {
                let a = a.clamp(0.0, 1.0);
                ColorMatrix::scale_offset(1.0 - 2.0 * a, a)
            }
            Self::Brightness(b) => ColorMatrix::scale_offset(b.max(0.0), 0.0),
            Self::Contrast(c) => {
                let c = c.max(0.0);
                ColorMatrix::scale_offset(c, 0.5 - 0.5 * c)
            }
            Self::Saturate(s) => {
                let s = s.max(0.0);
                ColorMatrix::linear([
                    [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
                    [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
                    [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
                ])
            }
            Self::HueRotate(deg) => {
                let (sin, cos) = deg.to_radians().sin_cos();
                ColorMatrix::linear([
                    [
                        0.213 + cos * 0.787 - sin * 0.213,
                        0.715 - cos * 0.715 - sin * 0.715,
                        0.072 - cos * 0.072 + sin * 0.928,
                    ],
                    [
                        0.213 - cos * 0.213 + sin * 0.143,
                        0.715 + cos * 0.285 + sin * 0.140,
                        0.072 - cos * 0.072 - sin * 0.283,
                    ],
                    [
                        0.213 - cos * 0.213 - sin * 0.787,
                        0.715 - cos * 0.715 + sin * 0.715,
                        0.072 + cos * 0.928 + sin * 0.072,
                    ],
                ])
            }
            Self::Sepia(a) => {
                let k = 1.0 - a.clamp(0.0, 1.0);
                ColorMatrix::linear([
                    [0.393 + 0.607 * k, 0.769 - 0.769 * k, 0.189 - 0.189 * k],
                    [0.349 - 0.349 * k, 0.686 + 0.314 * k, 0.168 - 0.168 * k],
                    [0.272 - 0.272 * k, 0.534 - 0.534 * k, 0.131 + 0.869 * k],
                ])
            }
        }
    }
}

/// Runs a chain on one sRGB-encoded colour, clamping between steps.
pub fn apply_filter(ops: &[FilterOp], c: [f32; 3]) -> [f32; 3] {
    ops.iter().fold(c, |c, op| op.matrix().apply(c))
}

/// How the themed page composites over its paper and texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Multiply,
    Screen,
    SoftLight,
}

impl Blend {
    /// W3C compositing formulas; `b` is the backdrop, `s` the page.
    pub fn apply(self, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
        let one = |b: f32, s: f32| match self {
            Self::Multiply => b * s,
            Self::Screen => b + s - b * s,
            Self::SoftLight => {
                if s <= 0.5 {
                    b - (1.0 - 2.0 * s) * b * (1.0 - b)
                } else {
                    let d = if b <= 0.25 {
                        ((16.0 * b - 12.0) * b + 4.0) * b
                    } else {
                        b.sqrt()
                    };
                    b + (2.0 * s - 1.0) * (d - b)
                }
            }
        };
        [one(b[0], s[0]), one(b[1], s[1]), one(b[2], s[2])]
    }
}

impl Appearance {
    /// The page chain, in mareader's order: base first, tint after.
    pub fn page_filter(&self) -> Vec<FilterOp> {
        let mut ops = match self.base {
            BaseMode::Light => Vec::new(),
            BaseMode::Dark => vec![
                FilterOp::Invert(0.92),
                FilterOp::HueRotate(180.0),
                FilterOp::Saturate(0.85),
                FilterOp::Brightness(1.02),
            ],
            BaseMode::Dim => vec![
                FilterOp::Brightness(0.8),
                FilterOp::Saturate(0.75),
                FilterOp::Contrast(0.9),
            ],
        };
        if self.has_tint() {
            let t = self.tint_amount() as f32;
            ops.push(FilterOp::Sepia((t * 0.55).clamp(0.0, 0.55)));
            ops.push(FilterOp::Saturate(1.0 + t * 0.6));
            // sepia() lands near 34°, so hue is measured from there.
            ops.push(FilterOp::HueRotate(f32::from(self.tint_hue) - 34.0));
        }
        ops
    }

    /// Everything the page pass needs to theme a page.
    pub fn page_tone(&self) -> PageTone {
        let ops = self.page_filter();
        let mut matrices = [ColorMatrix::linear(IDENTITY); MAX_FILTER_OPS];
        for (slot, op) in matrices.iter_mut().zip(&ops) {
            *slot = op.matrix();
        }
        let p = self.ui_tokens().paper;
        PageTone {
            matrices,
            count: ops.len().min(MAX_FILTER_OPS),
            blend: Some(self.page_blend()),
            paper: [p.0, p.1, p.2].map(|c| f32::from(c) / 255.0),
        }
    }

    pub fn page_blend(&self) -> Blend {
        match self.base {
            BaseMode::Light => Blend::Multiply,
            BaseMode::Dark => Blend::Screen,
            BaseMode::Dim => Blend::SoftLight,
        }
    }
}

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// A themed page: its filter chain, then blended over the paper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageTone {
    /// The chain; slots past `count` are unused.
    pub matrices: [ColorMatrix; MAX_FILTER_OPS],
    pub count: usize,
    /// None draws the raster as it is: flat fills, or no theme.
    pub blend: Option<Blend>,
    /// sRGB-encoded, 0..1.
    pub paper: [f32; 3],
}

impl PageTone {
    /// Leaves colours alone.
    pub const NONE: Self = Self {
        matrices: [ColorMatrix([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]); MAX_FILTER_OPS],
        count: 0,
        blend: None,
        paper: [1.0; 3],
    };

    /// One opaque page colour themed: the oracle for the shader.
    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        let Some(blend) = self.blend else {
            return c;
        };
        let chain = self.matrices.iter().take(self.count);
        let s = chain.fold(c, |c, m| m.apply(c));
        blend.apply(self.paper, s)
    }

    /// A blank page themed: what a page shows before it is drawn.
    pub fn blank(&self) -> [f32; 3] {
        self.apply([1.0; 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.004)
    }

    #[test]
    fn plain_light_has_no_filter() {
        assert!(Appearance::default().page_filter().is_empty());
    }

    #[test]
    fn the_chain_never_exceeds_the_shader_slots() {
        for base in BaseMode::ALL {
            let a = Appearance {
                base,
                tint_strength: 100,
                ..Default::default()
            };
            assert!(a.page_filter().len() <= MAX_FILTER_OPS);
        }
    }

    #[test]
    fn dark_turns_white_paper_near_black() {
        let a = Appearance {
            base: BaseMode::Dark,
            ..Default::default()
        };
        let out = apply_filter(&a.page_filter(), [1.0, 1.0, 1.0]);
        assert!(out.iter().all(|c| *c < 0.1), "{out:?}");
    }

    #[test]
    fn identity_amounts_are_identity() {
        let c = [0.2, 0.5, 0.9];
        for op in [
            FilterOp::Invert(0.0),
            FilterOp::HueRotate(0.0),
            FilterOp::Saturate(1.0),
            FilterOp::Sepia(0.0),
        ] {
            assert!(close(op.matrix().apply(c), c), "{op:?}");
        }
    }

    #[test]
    fn the_tint_runs_after_the_inversion() {
        let a = Appearance {
            base: BaseMode::Dark,
            tint_hue: 200,
            tint_strength: 60,
            ..Default::default()
        };
        let ops = a.page_filter();
        assert_eq!(ops.first(), Some(&FilterOp::Invert(0.92)));
        assert!(matches!(ops.last(), Some(FilterOp::HueRotate(_))));
    }

    #[test]
    fn sepia_caps_at_055() {
        let a = Appearance {
            tint_strength: 100,
            ..Default::default()
        };
        assert!(a.page_filter().contains(&FilterOp::Sepia(0.55)));
    }

    #[test]
    fn a_tone_runs_the_chain_then_blends_over_the_paper() {
        for base in BaseMode::ALL {
            let a = Appearance {
                base,
                tint_strength: 40,
                ..Default::default()
            };
            let tone = a.page_tone();
            assert_eq!(tone.count, a.page_filter().len());
            let ink = [0.1, 0.3, 0.2];
            let want = a
                .page_blend()
                .apply(tone.paper, apply_filter(&a.page_filter(), ink));
            assert!(close(tone.apply(ink), want), "{base:?}");
        }
    }

    #[test]
    fn a_blank_light_page_shows_the_paper() {
        let tone = Appearance::default().page_tone();
        assert!(close(tone.blank(), tone.paper));
        assert_eq!(PageTone::NONE.apply([0.2; 3]), [0.2; 3]);
    }

    #[test]
    fn blends_match_their_formulas() {
        let (b, s) = ([0.5, 0.2, 1.0], [0.5, 1.0, 0.0]);
        assert!(close(Blend::Multiply.apply(b, s), [0.25, 0.2, 0.0]));
        assert!(close(Blend::Screen.apply(b, s), [0.75, 1.0, 1.0]));
        assert!(
            close(Blend::SoftLight.apply(b, [0.5; 3]), b),
            "s = 0.5 is identity"
        );
    }
}
