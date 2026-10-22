//! Highlighter fills that sit under the ink, as a pen on paper does.

use crate::color::Rgb;
use crate::filter::{Blend, PageTone};

/// Ink against a mark keeps this contrast, or a share of the page's own.
pub const MARK_CONTRAST: f64 = 4.5;
/// That share, for pages already softer than [`MARK_CONTRAST`].
const MARK_SHARE: f64 = 0.6;
/// Screened fills glow on dark paper, so they start weaker.
const DARK_STRENGTH: f32 = 0.8;

/// A highlighter's fill as the page pass draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkFill {
    /// Multiply on light paper, screen on dark: either keeps the ink.
    pub blend: Blend,
    /// sRGB-encoded, 0..1.
    pub color: [f32; 3],
    /// How far the fill goes, 0..1.
    pub strength: f32,
}

impl MarkFill {
    /// What `backdrop` turns into under the fill: the shader's oracle.
    pub fn over(&self, backdrop: [f32; 3]) -> [f32; 3] {
        let s = self.strength;
        let one = |b: f32, c: f32| match self.blend {
            Blend::Screen => b + c * s * (1.0 - b),
            _ => b * (1.0 - s + c * s),
        };
        let ([br, bg, bb], [cr, cg, cb]) = (backdrop, self.color);
        [one(br, cr), one(bg, cg), one(bb, cb)]
    }
}

impl PageTone {
    /// The paper and ink this tone shows a plain page in.
    pub fn paper_and_ink(&self) -> ([f32; 3], [f32; 3]) {
        (self.apply([1.0; 3]), self.apply([0.0; 3]))
    }

    /// A fill of `color` at `opacity` on this page, the ink kept legible.
    pub fn mark(&self, color: [f32; 3], opacity: f32) -> MarkFill {
        let (paper, ink) = self.paper_and_ink();
        let dark = luminance(paper) < luminance(ink);
        let (blend, top) = match dark {
            true => (Blend::Screen, opacity * DARK_STRENGTH),
            false => (Blend::Multiply, opacity),
        };
        let fill = |strength| MarkFill {
            blend,
            color,
            strength,
        };
        let floor = MARK_CONTRAST.min(contrast(paper, ink) * MARK_SHARE);
        let legible = |s| {
            let f = fill(s);
            contrast(f.over(paper), f.over(ink)) >= floor
        };
        let top = top.clamp(0.0, 1.0);
        if legible(top) {
            return fill(top);
        }
        // Contrast falls as the fill grows: the strongest that holds.
        let (mut lo, mut hi) = (0.0, top);
        for _ in 0..14 {
            let mid = (lo + hi) * 0.5;
            if legible(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        fill(lo)
    }
}

fn luminance(c: [f32; 3]) -> f64 {
    Rgb::from_unit(c).luminance()
}

fn contrast(a: [f32; 3], b: [f32; 3]) -> f64 {
    Rgb::from_unit(a).contrast(Rgb::from_unit(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Appearance, BaseMode};

    const PENS: [[u8; 3]; 8] = [
        [0xff, 0xff, 0x00],
        [0xff, 0xbe, 0x00],
        [0xff, 0x00, 0x00],
        [0x83, 0xd3, 0x3c],
        [0x00, 0xb2, 0xf4],
        [0x16, 0x77, 0xff],
        [0xa5, 0x8a, 0xf0],
        [0xff, 0x00, 0xff],
    ];

    fn tones() -> Vec<(String, PageTone)> {
        let mut out = Vec::new();
        for base in BaseMode::ALL {
            let a = Appearance {
                base,
                ..Default::default()
            };
            out.push((format!("{base:?} book"), a.text_tone()));
            out.push((format!("{base:?} pdf"), a.page_tone()));
        }
        out
    }

    fn unit([r, g, b]: [u8; 3]) -> [f32; 3] {
        Rgb(r, g, b).to_unit()
    }

    #[test]
    fn ink_stays_legible_on_every_mark_in_every_look() {
        for (name, tone) in tones() {
            let (paper, ink) = tone.paper_and_ink();
            let floor = MARK_CONTRAST.min(contrast(paper, ink) * MARK_SHARE);
            for pen in PENS {
                for opacity in [0.2, 0.5, 0.8] {
                    let f = tone.mark(unit(pen), opacity);
                    let c = contrast(f.over(paper), f.over(ink));
                    assert!(c >= floor - 0.05, "{name} {pen:?} {opacity}: {c}");
                }
            }
        }
    }

    #[test]
    fn marks_multiply_on_light_paper_and_screen_on_dark() {
        for (name, tone) in tones() {
            let (paper, ink) = tone.paper_and_ink();
            let f = tone.mark(unit(PENS[0]), 0.5);
            let dark = luminance(paper) < luminance(ink);
            let want = if dark { Blend::Screen } else { Blend::Multiply };
            assert_eq!(f.blend, want, "{name}");
            // Still a mark: the paper visibly takes its colour.
            let lit = f.over(paper);
            let moved = lit.iter().zip(paper).map(|(a, b)| (a - b).abs());
            assert!(moved.fold(0.0, f32::max) > 0.08, "{name}");
        }
    }

    #[test]
    fn a_light_page_keeps_black_ink_black_under_a_mark() {
        let tone = Appearance::default().page_tone();
        let f = tone.mark(unit(PENS[0]), 0.8);
        assert_eq!(f.blend, Blend::Multiply);
        assert!((f.strength - 0.8).abs() < 1e-6);
        assert_eq!(f.over([0.0; 3]), [0.0; 3]);
        let lit = f.over([1.0; 3]);
        assert!(lit[0] > 0.99 && lit[2] < 0.25);
    }
}
