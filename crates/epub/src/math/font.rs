//! STIX Two Math: glyphs, their metrics and the MATH table's rules.
use std::fmt::Write as _;
use std::sync::OnceLock;

use ttf_parser::math::{Constants, GlyphAssembly, GlyphConstruction, MathValue};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

static DATA: &[u8] = include_bytes!("../../../../assets/fonts/STIXTwoMath-Regular.otf");

/// Extenders one stretched glyph may repeat.
const MAX_REPEATS: usize = 64;

/// The math face, parsed once.
pub(crate) struct Font {
    face: Face<'static>,
    upem: f32,
}

/// A glyph grown to a size: one larger variant, or parts laid in a line.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stretched {
    One(GlyphId),
    /// Each part and where it starts along the line, in ems; then the length.
    Parts(Vec<(GlyphId, f32)>, f32),
}

/// The math face, or `None` if it won't parse.
pub(crate) fn font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| {
        let face = Face::parse(DATA, 0).ok()?;
        face.tables().math?;
        let upem = f32::from(face.units_per_em()).max(1.0);
        Some(Font { face, upem })
    })
    .as_ref()
}

impl Font {
    fn em(&self, units: f32) -> f32 {
        units / self.upem
    }

    pub(crate) fn glyph(&self, c: char) -> Option<GlyphId> {
        self.face.glyph_index(c).filter(|g| g.0 != 0)
    }

    /// Advance in ems.
    pub(crate) fn advance(&self, g: GlyphId) -> f32 {
        self.em(f32::from(self.face.glyph_hor_advance(g).unwrap_or(0)))
    }

    /// Ink above and below the baseline, in ems.
    pub(crate) fn extent(&self, g: GlyphId) -> (f32, f32) {
        match self.face.glyph_bounding_box(g) {
            Some(r) => (self.em(f32::from(r.y_max)), self.em(-f32::from(r.y_min))),
            None => (0.0, 0.0),
        }
    }

    pub(crate) fn italic(&self, g: GlyphId) -> f32 {
        let info = self.math().and_then(|m| m.glyph_info);
        let v = info.and_then(|i| i.italic_corrections?.get(g));
        v.map_or(0.0, |v| self.em(f32::from(v.value)))
    }

    /// Where an accent above it centres, in ems from its left.
    pub(crate) fn accent_at(&self, g: GlyphId) -> Option<f32> {
        let info = self.math().and_then(|m| m.glyph_info);
        let v = info.and_then(|i| i.top_accent_attachments?.get(g))?;
        Some(self.em(f32::from(v.value)))
    }

    fn math(&self) -> Option<ttf_parser::math::Table<'static>> {
        self.face.tables().math
    }

    /// A MATH constant in ems.
    pub(crate) fn k(&self, f: fn(&Constants<'static>) -> MathValue<'static>) -> f32 {
        let c = self.math().and_then(|m| m.constants);
        c.map_or(0.0, |c| self.em(f32::from(f(&c).value)))
    }

    /// A MATH constant that is a plain number of font units, in ems.
    pub(crate) fn units(&self, f: fn(&Constants<'static>) -> u16) -> f32 {
        let c = self.math().and_then(|m| m.constants);
        c.map_or(0.0, |c| self.em(f32::from(f(&c))))
    }

    /// A MATH constant given in percent, as a share.
    pub(crate) fn share(&self, f: fn(&Constants<'static>) -> i16, or: f32) -> f32 {
        let c = self.math().and_then(|m| m.constants);
        c.map_or(or, |c| f32::from(f(&c)) / 100.0)
    }

    /// `g` grown to at least `length` ems, up or across.
    pub(crate) fn stretch(&self, g: GlyphId, length: f32, vertical: bool) -> Option<Stretched> {
        let variants = self.math()?.variants?;
        let set = match vertical {
            true => variants.vertical_constructions,
            false => variants.horizontal_constructions,
        };
        let built: GlyphConstruction<'static> = set.get(g)?;
        let goal = length * self.upem;
        let mut largest = None;
        for v in built.variants {
            largest = Some(v.variant_glyph);
            if f32::from(v.advance_measurement) >= goal {
                return Some(Stretched::One(v.variant_glyph));
            }
        }
        let overlap = f32::from(variants.min_connector_overlap);
        if let Some(parts) = built.assembly.and_then(|a| self.assemble(a, goal, overlap)) {
            return Some(parts);
        }
        largest.map(Stretched::One)
    }

    fn assemble(&self, a: GlyphAssembly<'static>, goal: f32, overlap: f32) -> Option<Stretched> {
        let parts: Vec<_> = a.parts.into_iter().take(16).collect();
        let advance = |extender: bool| -> (f32, usize) {
            let some = parts.iter().filter(|p| p.part_flags.extender() == extender);
            let n = some.clone().count();
            (some.map(|p| f32::from(p.full_advance)).sum(), n)
        };
        let ((fixed, n_fixed), (grown, n_grown)) = (advance(false), advance(true));
        let length = |reps: usize| {
            let count = n_fixed + reps * n_grown;
            fixed + reps as f32 * grown - count.saturating_sub(1) as f32 * overlap
        };
        let mut reps = 0;
        while n_grown > 0 && reps < MAX_REPEATS && length(reps) < goal {
            reps += 1;
        }
        let mut line = Vec::new();
        for p in &parts {
            let times = if p.part_flags.extender() { reps } else { 1 };
            line.extend(std::iter::repeat_n(*p, times));
        }
        if line.is_empty() {
            return None;
        }
        // Long enough already: the joins take up what is left over.
        let joins = line.len().saturating_sub(1).max(1) as f32;
        let spare = ((length(reps) - goal) / joins).max(0.0);
        let mut at = 0.0;
        let mut out = Vec::with_capacity(line.len());
        for (i, p) in line.iter().enumerate() {
            out.push((p.glyph_id, self.em(at)));
            if let Some(next) = line.get(i + 1) {
                let most = f32::from(p.end_connector_length.min(next.start_connector_length));
                let lap = (overlap + spare).min(most.max(overlap));
                at += f32::from(p.full_advance) - lap;
            }
        }
        let end = line.last().map_or(0.0, |p| f32::from(p.full_advance));
        Some(Stretched::Parts(out, self.em(at + end)))
    }

    /// SVG path data for `g` in font units, y up.
    pub(crate) fn outline(&self, g: GlyphId) -> String {
        let mut pen = Pen(String::new());
        let _ = self.face.outline_glyph(g, &mut pen);
        pen.0
    }

    pub(crate) fn upem(&self) -> f32 {
        self.upem
    }
}

struct Pen(String);

impl OutlineBuilder for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.0, "M{} {}", x.round(), y.round());
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.0, "L{} {}", x.round(), y.round());
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (a, b, c, d) = (x1.round(), y1.round(), x.round(), y.round());
        let _ = write!(self.0, "Q{a} {b} {c} {d}");
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p = [x1, y1, x2, y2, x, y].map(f32::round);
        let [a, b, c, d, e, f] = p;
        let _ = write!(self.0, "C{a} {b} {c} {d} {e} {f}");
    }

    fn close(&mut self) {
        self.0.push('Z');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_face_has_math_rules() {
        let f = font().expect("the math face");
        assert!(f.k(Constants::axis_height) > 0.2);
        assert!(f.glyph('∑').is_some() && f.glyph('𝑥').is_some());
    }

    #[test]
    fn a_paren_grows_by_variants_then_parts() {
        let f = font().expect("the math face");
        let paren = f.glyph('(').expect("a paren");
        assert!(matches!(
            f.stretch(paren, 1.5, true),
            Some(Stretched::One(_))
        ));
        match f.stretch(paren, 12.0, true) {
            Some(Stretched::Parts(parts, length)) => {
                assert!(parts.len() >= 3);
                assert!(length >= 11.9, "{length}");
            }
            other => assert!(matches!(other, Some(Stretched::Parts(..))), "{other:?}"),
        }
    }
}
