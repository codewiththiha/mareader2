//! Burmese, Thai or Arabic labels shaped by parley; egui misplaces marks.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use egui::{
    Color32, ColorImage, Id, Rect, Response, RichText, Sense, TextureHandle, TextureOptions, Ui,
    Vec2, WidgetInfo, WidgetType, pos2,
};

/// Past this many drawn labels the cache starts over.
const KEEP: usize = 96;
const CACHE: &str = "shaped-labels";

/// Whether egui's glyph-by-glyph text would garble `text`.
pub fn needs_shaping(text: &str) -> bool {
    text.chars().any(complex)
}

/// Scripts whose letters join, reorder or stack when written.
fn complex(c: char) -> bool {
    matches!(
        u32::from(c),
        // Hebrew, Arabic, Syriac, Thaana, N'Ko and their extensions.
        0x0590..=0x08FF
        // Devanagari to Sinhala, Thai, Lao, Tibetan, Myanmar.
        | 0x0900..=0x109F
        // Khmer, Mongolian, Tai scripts, Balinese and their neighbours.
        | 0x1780..=0x1CFF
        // Syloti Nagri to Myanmar Extended-B and Meetei Mayek.
        | 0xA800..=0xABFF
        // Arabic presentation forms.
        | 0xFB1D..=0xFDFF
        | 0xFE70..=0xFEFF
        // Brahmi and the historic Indic scripts.
        | 0x11000..=0x11FFF
    )
}

#[derive(Clone)]
struct Drawn {
    texture: TextureHandle,
    /// In points.
    size: Vec2,
}

#[derive(Clone, Default)]
struct Cache {
    labels: HashMap<u64, Option<Drawn>>,
}

/// `text` at `size` points in `color`, wrapping at the width left.
pub fn label(ui: &mut Ui, text: &str, size: f32, color: Color32) -> Response {
    if !needs_shaping(text) {
        return ui.label(RichText::new(text).size(size).color(color));
    }
    let width = ui.available_width().max(1.0);
    let Some(drawn) = drawn(ui, text, size, width) else {
        return ui.label(RichText::new(text).size(size).color(color));
    };
    let (rect, response) = ui.allocate_exact_size(drawn.size, Sense::hover());
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    ui.painter().image(drawn.texture.id(), rect, uv, color);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
}

/// The texture for this text, size, width and scale, drawn once.
fn drawn(ui: &Ui, text: &str, size: f32, width: f32) -> Option<Drawn> {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let mut h = std::hash::DefaultHasher::new();
    (text, size.to_bits(), width.round() as i32, ppp.to_bits()).hash(&mut h);
    let key = h.finish();
    let id = Id::new(CACHE);
    if let Some(hit) = ctx.data(|d| {
        d.get_temp::<Cache>(id)
            .and_then(|c| c.labels.get(&key).cloned())
    }) {
        return hit;
    }
    let made = mareader_reflow::label(text, size * ppp, width * ppp).map(|px| {
        let pixels = [px.width as usize, px.height as usize];
        let image = ColorImage::from_rgba_premultiplied(pixels, &px.rgba);
        Drawn {
            texture: ctx.load_texture("shaped-label", image, TextureOptions::LINEAR),
            size: Vec2::new(px.width as f32, px.height as f32) / ppp,
        }
    });
    ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<Cache>(id);
        if cache.labels.len() >= KEEP {
            cache.labels.clear();
        }
        cache.labels.insert(key, made.clone());
    });
    made
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_scripts_that_shape_take_the_long_way() {
        assert!(needs_shaping("ကျောင်း"), "Burmese");
        assert!(needs_shaping("สวัสดี"), "Thai");
        assert!(needs_shaping("مرحبا"), "Arabic");
        assert!(!needs_shaping("Nevermore"));
        assert!(
            !needs_shaping("日本語の本"),
            "CJK sets one glyph per letter"
        );
        assert!(!needs_shaping("Привет"));
    }
}
