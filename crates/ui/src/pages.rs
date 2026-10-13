//! Page textures: one byte-capped cache serving every pane.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use mareader_session::{DocId, PageKey, Raster, Scale};

use crate::gpu::GpuPage;

/// Textures past this are evicted, least recently drawn first.
pub const CAP_BYTES: usize = 256 << 20;

/// A page on the GPU: ours for the page pass, or egui's without one.
#[derive(Clone)]
pub enum PageTexture {
    Gpu(Arc<GpuPage>),
    /// Headless runs have no device; egui keeps the pixels.
    Egui(TextureHandle),
}

impl PageTexture {
    pub fn egui(ctx: &Context, key: PageKey, raster: &Raster) -> Self {
        let size = [raster.width as usize, raster.height as usize];
        let image = ColorImage::from_rgba_premultiplied(size, &raster.pixels);
        let name = format!("page-{:?}-{}", key.doc, key.page);
        Self::Egui(ctx.load_texture(name, image, TextureOptions::LINEAR))
    }
}

impl std::fmt::Debug for PageTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gpu(_) => f.write_str("Gpu"),
            Self::Egui(h) => write!(f, "Egui({:?})", h.id()),
        }
    }
}

/// What a pane can draw for a page right now.
#[derive(Debug, Clone)]
pub enum Lookup {
    /// Drawn at the scale asked for.
    Sharp(PageTexture),
    /// Another scale, drawn stretched until the sharp one arrives.
    Stale(PageTexture),
    Failed,
    Missing,
}

struct Slot {
    scale: Scale,
    texture: PageTexture,
    bytes: usize,
    /// The frame it was last drawn in, and last drawn sharp in.
    used: u64,
    sharp: u64,
}

pub struct PageCache {
    pages: HashMap<(DocId, usize), Vec<Slot>>,
    failed: HashSet<(DocId, usize)>,
    bytes: usize,
    cap: usize,
    frame: u64,
}

impl Default for PageCache {
    fn default() -> Self {
        Self::new(CAP_BYTES)
    }
}

impl PageCache {
    pub fn new(cap: usize) -> Self {
        Self {
            pages: HashMap::new(),
            failed: HashSet::new(),
            bytes: 0,
            cap,
            frame: 1,
        }
    }

    /// Keeps an uploaded page; a slot at the same scale is replaced.
    pub fn insert(&mut self, key: PageKey, texture: PageTexture, bytes: usize) {
        let slots = self.pages.entry((key.doc, key.page)).or_default();
        if let Some(i) = slots.iter().position(|s| s.scale == key.scale) {
            self.bytes -= slots.swap_remove(i).bytes;
        }
        self.bytes += bytes;
        slots.push(Slot {
            scale: key.scale,
            texture,
            bytes,
            used: self.frame,
            sharp: 0,
        });
        self.failed.remove(&(key.doc, key.page));
    }

    pub fn fail(&mut self, key: PageKey) {
        self.failed.insert((key.doc, key.page));
    }

    /// The best texture for `key`, marked as drawn this frame.
    pub fn lookup(&mut self, key: PageKey) -> Lookup {
        let frame = self.frame;
        let Some(slots) = self.pages.get_mut(&(key.doc, key.page)) else {
            return self.missing(key);
        };
        if let Some(s) = slots.iter_mut().find(|s| s.scale == key.scale) {
            s.used = frame;
            s.sharp = frame;
            return Lookup::Sharp(s.texture.clone());
        }
        // The nearest scale; on a tie, the sharper one.
        let nearest = slots.iter_mut().min_by_key(|s| {
            let away = (i32::from(s.scale.steps()) - i32::from(key.scale.steps())).abs();
            (away, -i32::from(s.scale.steps()))
        });
        match nearest {
            Some(s) => {
                s.used = frame;
                Lookup::Stale(s.texture.clone())
            }
            None => self.missing(key),
        }
    }

    fn missing(&self, key: PageKey) -> Lookup {
        if self.failed.contains(&(key.doc, key.page)) {
            Lookup::Failed
        } else {
            Lookup::Missing
        }
    }

    /// Prefetched pages count as used, so eviction spares them.
    pub fn touch(&mut self, key: PageKey) -> bool {
        let frame = self.frame;
        let slot = self
            .pages
            .get_mut(&(key.doc, key.page))
            .and_then(|slots| slots.iter_mut().find(|s| s.scale == key.scale));
        slot.map(|s| s.used = frame).is_some() || self.failed.contains(&(key.doc, key.page))
    }

    /// Drops every texture of a closed document.
    pub fn forget(&mut self, doc: DocId) {
        self.pages.retain(|(d, _), slots| {
            if *d == doc {
                self.bytes -= slots.iter().map(|s| s.bytes).sum::<usize>();
            }
            *d != doc
        });
        self.failed.retain(|(d, _)| *d != doc);
    }

    /// Evicts superseded scales, then cold pages past the cap; never drawn.
    pub fn end_frame(&mut self) {
        let frame = self.frame;
        let mut freed = 0;
        for slots in self.pages.values_mut() {
            if slots.iter().any(|s| s.sharp == frame) {
                slots.retain(|s| {
                    let keep = s.used == frame;
                    freed += if keep { 0 } else { s.bytes };
                    keep
                });
            }
        }
        self.bytes -= freed;
        if self.bytes > self.cap {
            self.evict_cold();
        }
        self.pages.retain(|_, slots| !slots.is_empty());
        self.frame += 1;
    }

    fn evict_cold(&mut self) {
        let frame = self.frame;
        let mut cold: Vec<(u64, (DocId, usize), Scale)> = self
            .pages
            .iter()
            .flat_map(|(page, slots)| slots.iter().map(move |s| (s.used, *page, s.scale)))
            .filter(|(used, ..)| *used < frame)
            .collect();
        cold.sort_unstable_by_key(|(used, ..)| *used);
        for (_, page, scale) in cold {
            if self.bytes <= self.cap {
                break;
            }
            if let Some(slots) = self.pages.get_mut(&page)
                && let Some(i) = slots.iter().position(|s| s.scale == scale)
            {
                self.bytes -= slots.swap_remove(i).bytes;
            }
        }
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.pages.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Out = Result<(), &'static str>;

    fn raster(side: u32) -> Raster {
        Raster {
            width: side,
            height: side,
            pixels: vec![255; (side * side * 4) as usize],
        }
    }

    fn put(c: &mut PageCache, ctx: &Context, key: PageKey, side: u32) {
        let r = raster(side);
        c.insert(key, PageTexture::egui(ctx, key, &r), r.bytes());
    }

    use crate::testing::doc;

    fn key(doc: DocId, page: usize, scale: f32) -> PageKey {
        PageKey {
            doc,
            page,
            scale: Scale::covering(scale),
        }
    }

    #[test]
    fn a_stale_scale_stands_in_until_the_sharp_one_is_drawn() -> Out {
        let ctx = Context::default();
        let mut c = PageCache::default();
        let d = doc()?;
        put(&mut c, &ctx, key(d, 0, 1.0), 8);
        assert!(matches!(c.lookup(key(d, 0, 2.0)), Lookup::Stale(_)));
        c.end_frame();
        put(&mut c, &ctx, key(d, 0, 2.0), 16);
        assert!(matches!(c.lookup(key(d, 0, 2.0)), Lookup::Sharp(_)));
        c.end_frame();
        assert_eq!((c.len(), c.bytes()), (1, 16 * 16 * 4));
        Ok(())
    }

    #[test]
    fn the_cap_evicts_cold_pages_but_never_visible_ones() -> Out {
        let ctx = Context::default();
        let one = 8 * 8 * 4;
        let mut c = PageCache::new(2 * one);
        let d = doc()?;
        for page in 0..4 {
            put(&mut c, &ctx, key(d, page, 1.0), 8);
        }
        c.end_frame();
        // Pages 2 and 3 are on screen; 0 and 1 went cold.
        for page in [2, 3] {
            assert!(matches!(c.lookup(key(d, page, 1.0)), Lookup::Sharp(_)));
        }
        c.end_frame();
        assert_eq!(c.bytes(), 2 * one);
        assert!(!c.touch(key(d, 0, 1.0)), "evicted");
        // Over the cap, what is on screen still stays.
        for page in 4..7 {
            put(&mut c, &ctx, key(d, page, 1.0), 8);
            assert!(matches!(c.lookup(key(d, page, 1.0)), Lookup::Sharp(_)));
        }
        c.end_frame();
        assert_eq!(c.len(), 3);
        Ok(())
    }

    #[test]
    fn forgetting_a_document_frees_all_of_it() -> Out {
        let ctx = Context::default();
        let mut c = PageCache::default();
        let d = doc()?;
        put(&mut c, &ctx, key(d, 0, 1.0), 8);
        c.fail(key(d, 1, 1.0));
        assert!(matches!(c.lookup(key(d, 1, 1.0)), Lookup::Failed));
        c.forget(d);
        assert!(c.is_empty() && c.bytes() == 0);
        assert!(matches!(c.lookup(key(d, 1, 1.0)), Lookup::Missing));
        Ok(())
    }
}
