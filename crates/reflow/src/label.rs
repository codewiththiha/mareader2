//! Short text shaped by parley: for scripts the UI's own text can't draw.
use std::borrow::Cow;
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use parley::{
    Alignment, AlignmentOptions, FontFamily, FontFamilyName, Layout, PositionedLayoutItem,
    StyleProperty,
};
use vello_cpu::color::palette::css::WHITE;
use vello_cpu::{Glyph, Pixmap, RenderContext, Resources};

use crate::fonts::{BUNDLED, Fonts, Ink, SANS};
use crate::raster::{MAX_SIDE, Pixels};

/// A face of this machine that draws text the bundled faces can't.
#[derive(Clone, PartialEq, Eq)]
pub struct Fallback {
    /// Unique per face and file while the app runs.
    pub key: String,
    /// The whole font file; `index` picks the face in a collection.
    pub data: Vec<u8>,
    pub index: u32,
}

impl std::fmt::Debug for Fallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fallback({}, {} bytes)", self.key, self.data.len())
    }
}

/// The machine faces parley picks to draw `text`, beyond the bundled ones.
pub fn fallbacks(text: &str) -> Vec<Fallback> {
    if text.is_empty() {
        return Vec::new();
    }
    catch_unwind(AssertUnwindSafe(|| pick(text))).unwrap_or_default()
}

fn pick(text: &str) -> Vec<Fallback> {
    let mut fonts = Fonts::system();
    let layout = lay_out(&mut fonts, text, 16.0, None);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let font = run.run().font();
            let bytes = font.data.data();
            let ours = BUNDLED.iter().any(|b| b.as_ptr() == bytes.as_ptr());
            if ours || !seen.insert((font.data.id(), font.index)) {
                continue;
            }
            out.push(Fallback {
                key: format!("machine-{}-{}", font.data.id(), font.index),
                data: bytes.to_vec(),
                index: font.index,
            });
        }
    }
    out
}

/// `text` wrapped at `width`, as white glyphs to tint; `None` if empty or huge.
pub fn label(text: &str, size: f32, width: f32) -> Option<Pixels> {
    label_lines(text, size, width, usize::MAX).map(|l| l.pixels)
}

/// A label drawn to at most a number of lines.
pub struct Lines {
    pub pixels: Pixels,
    /// Whether lines past the limit were left out.
    pub cut: bool,
}

/// `text` as `label` draws it, stopping after `lines` lines.
pub fn label_lines(text: &str, size: f32, width: f32, lines: usize) -> Option<Lines> {
    let fine = |v: f32| v.is_finite() && v > 0.0;
    if text.trim().is_empty() || !fine(size) || !fine(width) || lines == 0 {
        return None;
    }
    catch_unwind(AssertUnwindSafe(|| draw(text, size, width, lines))).ok()?
}

/// The size `label_lines` would draw, in pixels, and whether it cuts.
pub fn measure_lines(text: &str, size: f32, width: f32, lines: usize) -> Option<(u32, u32, bool)> {
    let fine = |v: f32| v.is_finite() && v > 0.0;
    if text.trim().is_empty() || !fine(size) || !fine(width) || lines == 0 {
        return None;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let layout = lay_out(&mut Fonts::system(), text, size, Some(width));
        extent(&layout, lines)
    }))
    .ok()
}

/// Width and height of the first `lines` lines, and whether more follow.
fn extent(layout: &Layout<Ink>, lines: usize) -> (u32, u32, bool) {
    let cut = layout.len() > lines;
    let last = layout.len().min(lines).checked_sub(1);
    let line = last.and_then(|i| layout.get(i));
    let bottom = line.map_or(0.0, |l| l.metrics().block_max_coord);
    let w = layout.width().ceil().max(1.0) as u32;
    (w, bottom.ceil().max(1.0) as u32, cut)
}

fn draw(text: &str, size: f32, width: f32, lines: usize) -> Option<Lines> {
    let mut fonts = Fonts::system();
    let layout = lay_out(&mut fonts, text, size, Some(width));
    let (w, h, cut) = extent(&layout, lines);
    if w > MAX_SIDE || h > MAX_SIDE {
        return None;
    }
    let (w16, h16) = (u16::try_from(w).ok()?, u16::try_from(h).ok()?);
    let mut ctx = RenderContext::new(w16, h16);
    let mut resources = Resources::new();
    ctx.set_paint(WHITE);
    let mut coords = Vec::new();
    for line in layout.lines().take(lines) {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let font = run.run();
            coords.clear();
            coords.extend(font.normalized_coords().iter().map(|c| c.to_bits()));
            let placed = run.positioned_glyphs().map(|g| Glyph {
                id: g.id,
                x: g.x,
                y: g.y,
            });
            // A glyph the font can't draw is left blank, not fatal.
            let _ = ctx
                .glyph_run(&mut resources, font.font())
                .font_size(font.font_size())
                .normalized_coords(&coords)
                .fill_glyphs(placed);
        }
    }
    let mut pixmap = Pixmap::new(w16, h16);
    ctx.flush();
    ctx.render(&mut pixmap, &mut resources);
    let pixels = Pixels {
        width: w,
        height: h,
        rgba: pixmap.data_as_u8_slice().to_vec(),
    };
    Some(Lines { pixels, cut })
}

/// Inter first; parley finds the machine's face for every other script.
fn lay_out(fonts: &mut Fonts, text: &str, size: f32, width: Option<f32>) -> Layout<Ink> {
    let Fonts { fonts, layout } = fonts;
    let mut b = layout.ranged_builder(fonts, text, 1.0, false);
    b.push_default(StyleProperty::FontFamily(FontFamily::Single(
        FontFamilyName::Named(Cow::Borrowed(SANS)),
    )));
    b.push_default(StyleProperty::FontSize(size));
    b.push_default(StyleProperty::Brush(Ink::default()));
    let mut laid = b.build(text);
    laid.break_all_lines(width);
    laid.align(Alignment::Start, AlignmentOptions::default());
    laid
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn latin_needs_no_machine_face() {
        assert!(fallbacks("Once upon a midnight dreary").is_empty());
    }

    #[test]
    fn a_label_is_drawn_and_wraps() {
        let one = label("Nevermore", 16.0, 1000.0).expect("drawn");
        let wrapped = label("Nevermore nevermore nevermore", 16.0, 60.0).expect("drawn");
        assert!(one.width > 0 && one.height > 0);
        assert!(wrapped.height > one.height, "it wraps onto more lines");
        assert!(one.rgba.iter().any(|&b| b > 0), "some ink");
    }

    #[test]
    fn a_label_stops_at_its_last_line() {
        let text = "မြန်မာဘာသာစကားသည်လှပသည် ".repeat(6);
        let all = label_lines(&text, 16.0, 80.0, usize::MAX).expect("drawn");
        let two = label_lines(&text, 16.0, 80.0, 2).expect("drawn");
        assert!(!all.cut && two.cut);
        assert!(two.pixels.height < all.pixels.height);
        assert_eq!(
            two.pixels.rgba.len(),
            (two.pixels.width * two.pixels.height * 4) as usize
        );
        let measured = measure_lines(&text, 16.0, 80.0, 2).expect("measured");
        assert_eq!(measured, (two.pixels.width, two.pixels.height, true));
    }

    #[test]
    fn nothing_to_draw_is_none() {
        assert!(label("  ", 16.0, 100.0).is_none());
        assert!(label("x", f32::NAN, 100.0).is_none());
        assert!(label("x", 16.0, 0.0).is_none());
    }

    #[test]
    fn scripts_without_spaces_break_between_words() {
        // Thai and Burmese leave out spaces; a dictionary finds the breaks.
        for text in ["ภาษาไทยเป็นภาษาที่ไม่เว้นวรรคระหว่างคำ", "မြန်မာဘာသာစကားသည်လှပသည်"]
        {
            let laid = lay_out(&mut Fonts::system(), text, 16.0, Some(80.0));
            assert!(laid.len() > 1, "{text}: no break found");
        }
    }
}
