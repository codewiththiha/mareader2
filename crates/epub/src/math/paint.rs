//! A laid-out formula as SVG: glyphs as paths, so no font is needed.
use std::collections::HashMap;
use std::fmt::Write as _;

use super::font::Font;
use super::layout::{Bx, Mark};
use crate::css::Rgb;

/// Space round the ink, in ems, so edges aren't clipped.
pub(crate) const PAD: f32 = 0.05;
/// User units per em in the drawing.
const UNIT: f32 = 1000.0;
/// Pixels per em for the drawing's own size.
const PX: f32 = 16.0;
/// The most markup one formula may write.
const MAX_SVG: usize = 4 << 20;

fn n(v: f32) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r:.1}")
    }
}

fn paint(colour: Option<Rgb>) -> String {
    match colour {
        Some([r, g, b]) => format!("#{r:02x}{g:02x}{b:02x}"),
        None => "#000".to_owned(),
    }
}

/// `b` drawn in ems; black unless the formula gave a colour.
pub(crate) fn svg(b: &Bx, f: &Font) -> Option<String> {
    let (w, h) = (b.w + 2.0 * PAD, b.a + b.d + 2.0 * PAD);
    if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
        return None;
    }
    let top = b.a + PAD;
    let at = |x: f32, y: f32| ((x + PAD) * UNIT, (top - y) * UNIT);
    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        n(w * PX),
        n(h * PX),
        n(w * UNIT),
        n(h * UNIT)
    );
    let mut paths: HashMap<u16, String> = HashMap::new();
    let scale = UNIT / f.upem();
    for item in &b.items {
        let (x, y) = at(item.x, item.y);
        let fill = paint(item.colour);
        let _ = match item.mark {
            Mark::Glyph { g, size } => {
                let d = paths.entry(g.0).or_insert_with(|| f.outline(g));
                if d.is_empty() {
                    continue;
                }
                let s = size * scale;
                write!(
                    out,
                    "<path fill=\"{fill}\" transform=\"matrix({} 0 0 {} {} {})\" d=\"{d}\"/>",
                    s,
                    -s,
                    n(x),
                    n(y)
                )
            }
            Mark::Rect { w, h } => write!(
                out,
                "<rect fill=\"{fill}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                n(x),
                n(y - h * UNIT),
                n(w * UNIT),
                n(h * UNIT)
            ),
            Mark::Line { dx, dy, t } => write!(
                out,
                "<line stroke=\"{fill}\" stroke-width=\"{}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
                n(t * UNIT),
                n(x),
                n(y),
                n(x + dx * UNIT),
                n(y - dy * UNIT)
            ),
            Mark::Oval { rx, ry, t } => write!(
                out,
                "<ellipse fill=\"none\" stroke=\"{fill}\" stroke-width=\"{}\" cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"/>",
                n(t * UNIT),
                n(x),
                n(y),
                n(rx * UNIT),
                n(ry * UNIT)
            ),
            Mark::Frame { w, h, t, r } => write!(
                out,
                "<rect fill=\"none\" stroke=\"{fill}\" stroke-width=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"/>",
                n(t * UNIT),
                n(x),
                n(y - h * UNIT),
                n(w * UNIT),
                n(h * UNIT),
                n(r * UNIT)
            ),
        };
        if out.len() > MAX_SVG {
            return None;
        }
    }
    out.push_str("</svg>");
    Some(out)
}
