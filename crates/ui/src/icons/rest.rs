//! One-frame generated resting glyphs and their bounded texture cache.

use std::collections::HashMap;
use std::time::Instant;

use egui::{Context, Id, Image, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use mareader_animated_icons::{Pack, RestingGlyph};

use super::diagnostics::{self, CacheStats};

const CACHE_BYTES: usize = 4 * 1024 * 1024;
const CACHE_ENTRIES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GlyphKey {
    pack: usize,
    side: u16,
}

impl GlyphKey {
    fn new(pack: &'static Pack, side: u16) -> Self {
        Self {
            pack: pack as *const Pack as usize,
            side,
        }
    }
}

#[derive(Clone)]
struct CachedGlyph {
    texture: TextureHandle,
    side: u32,
    target_x: u32,
    target_y: u32,
    width: u32,
    height: u32,
}

impl CachedGlyph {
    fn image(&self, size: f32) -> Image<'static> {
        let width = self.width.max(1) as f32;
        let height = self.height.max(1) as f32;
        let uv = Rect::from_min_max(
            egui::pos2(
                -(self.target_x as f32) / width,
                -(self.target_y as f32) / height,
            ),
            egui::pos2(
                self.side.saturating_sub(self.target_x) as f32 / width,
                self.side.saturating_sub(self.target_y) as f32 / height,
            ),
        );
        Image::from_texture(egui::load::SizedTexture::new(
            self.texture.id(),
            Vec2::splat(size),
        ))
        .uv(uv)
        .fit_to_exact_size(Vec2::splat(size))
    }
}

#[derive(Clone, Default)]
struct RestingStore {
    entries: HashMap<GlyphKey, (CachedGlyph, usize, u64)>,
    bytes: usize,
    stamp: u64,
    evictions: u64,
    evicted_bytes: u64,
}

impl RestingStore {
    fn get(&mut self, key: &GlyphKey) -> Option<CachedGlyph> {
        self.stamp = self.stamp.wrapping_add(1);
        let entry = self.entries.get_mut(key)?;
        entry.2 = self.stamp;
        Some(entry.0.clone())
    }

    fn insert(&mut self, key: GlyphKey, glyph: CachedGlyph, cost: usize) {
        if cost > CACHE_BYTES {
            return;
        }
        if let Some((_, old_cost, _)) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(old_cost);
        }
        while self.bytes.saturating_add(cost) > CACHE_BYTES || self.entries.len() >= CACHE_ENTRIES {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, _, used))| *used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some((_, old_cost, _)) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(old_cost);
                self.evictions = self.evictions.saturating_add(1);
                self.evicted_bytes = self.evicted_bytes.saturating_add(old_cost as u64);
            }
        }
        self.stamp = self.stamp.wrapping_add(1);
        self.entries.insert(key, (glyph, cost, self.stamp));
        self.bytes = self.bytes.saturating_add(cost);
    }
}

pub(super) fn image(ui: &Ui, pack: &'static Pack, size: f32) -> Image<'static> {
    let ctx = ui.ctx();
    let side = side(size, ctx.pixels_per_point());
    let key = GlyphKey::new(pack, side as u16);
    let store_id = Id::new("resting-icon-glyphs");
    if let Some(glyph) = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<RestingStore>(store_id)
            .get(&key)
    }) {
        diagnostics::resting_lookup(ctx, true);
        return glyph.image(size);
    }
    diagnostics::resting_lookup(ctx, false);
    let raster_started = diagnostics::enabled().then(Instant::now);
    let Some(glyph) = mareader_animated_icons::raster_resting(pack, side) else {
        if let Some(started) = raster_started {
            diagnostics::raster(ctx, started.elapsed());
        }
        return blank(ctx, size);
    };
    if let Some(started) = raster_started {
        diagnostics::raster(ctx, started.elapsed());
    }
    let bytes = glyph.byte_len();
    let cached = upload(ctx, pack.id(), glyph);
    let image = cached.image(size);
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<RestingStore>(store_id)
            .insert(key, cached, bytes);
    });
    image
}

fn upload(ctx: &Context, pack_id: &str, glyph: RestingGlyph) -> CachedGlyph {
    let started = diagnostics::enabled().then(Instant::now);
    let bytes = glyph.byte_len();
    let image = egui::ColorImage::from_rgba_premultiplied(
        [glyph.width as usize, glyph.height as usize],
        &glyph.pixels,
    );
    let texture = ctx.load_texture(
        format!("resting-icon-{pack_id}-{}", glyph.side),
        image,
        TextureOptions::LINEAR,
    );
    if let Some(started) = started {
        diagnostics::upload(ctx, bytes, started.elapsed());
    }
    CachedGlyph {
        texture,
        side: glyph.side,
        target_x: glyph.target_x,
        target_y: glyph.target_y,
        width: glyph.width,
        height: glyph.height,
    }
}

pub(super) fn cache_stats(ctx: &Context) -> CacheStats {
    ctx.data_mut(|data| {
        let store = data.get_temp_mut_or_default::<RestingStore>(Id::new("resting-icon-glyphs"));
        CacheStats {
            bytes: store.bytes,
            evictions: store.evictions,
            evicted_bytes: store.evicted_bytes,
        }
    })
}

pub(super) fn blank(ctx: &Context, size: f32) -> Image<'static> {
    let key = Id::new("mareader-resting-icon-blank");
    let texture = if let Some(texture) = ctx.data(|data| data.get_temp::<TextureHandle>(key)) {
        texture
    } else {
        let image = egui::ColorImage::new([1, 1], vec![egui::Color32::TRANSPARENT]);
        let texture =
            ctx.load_texture("mareader-resting-icon-blank", image, TextureOptions::LINEAR);
        ctx.data_mut(|data| data.insert_temp(key, texture.clone()));
        texture
    };
    Image::from_texture(egui::load::SizedTexture::new(
        texture.id(),
        Vec2::splat(size),
    ))
    .fit_to_exact_size(Vec2::splat(size))
}

fn side(size: f32, ppp: f32) -> u32 {
    ((size * ppp).round() as i32).clamp(8, mareader_animated_icons::MAX_SIDE as i32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resting_sizes_are_device_pixel_buckets() {
        assert_eq!(side(16.0, 1.0), 16);
        assert_eq!(side(16.0, 1.25), 20);
        assert_eq!(side(16.0, 2.0), 32);
        assert_eq!(side(1.0, 1.0), 8);
        assert_eq!(side(200.0, 2.0), mareader_animated_icons::MAX_SIDE);
    }

    #[test]
    fn repeated_resting_glyphs_share_the_pack_and_size_entry() {
        let Some(pack) = mareader_animated_icons::find("sun") else {
            panic!("sun pack is missing");
        };
        let ctx = Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(100.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            let _first = image(ui, pack, 16.0);
            let _second = image(ui, pack, 16.0);
        });
        output.textures_delta.clear();
        let store_id = Id::new("resting-icon-glyphs");
        let store = ctx.data(|data| data.get_temp::<RestingStore>(store_id));
        assert_eq!(store.as_ref().map(|store| store.entries.len()), Some(1));
        assert!(store.is_some_and(|store| store.bytes > 0));
    }
}
