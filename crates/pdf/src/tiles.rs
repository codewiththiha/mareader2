//! A page too big for one raster, split into a grid of tiles.

use crate::PageSize;

/// A tile's longest side; any GPU takes 2048.
pub const TILE_SIDE: u32 = 2048;
/// Tiles grow to a quarter of the page before there are more of them.
const MIN_TILE_SIDE: u32 = 1024;
/// A tiled page's longest side, so a deep zoom stays bounded.
pub const MAX_TILED_SIDE: u32 = 16_384;

/// How a page splits at a scale; workers and views derive the same one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tiling {
    /// Pixels per point, lowered to the tiled side cap.
    pub scale: f32,
    /// The whole page in pixels.
    pub width: u32,
    pub height: u32,
    pub side: u32,
}

impl Tiling {
    pub fn new(size: PageSize, scale: f32) -> Self {
        let long = size.width.max(size.height).max(1.0);
        let scale = scale.clamp(1.0 / 64.0, MAX_TILED_SIDE as f32 / long);
        // Truncated as hayro truncates a whole page's pixmap.
        let width = ((size.width * scale) as u32).max(1);
        let height = ((size.height * scale) as u32).max(1);
        let side = width.max(height).div_ceil(4);
        Self {
            scale,
            width,
            height,
            side: side.clamp(MIN_TILE_SIDE, TILE_SIDE),
        }
    }

    pub fn cols(&self) -> u32 {
        self.width.div_ceil(self.side)
    }

    pub fn rows(&self) -> u32 {
        self.height.div_ceil(self.side)
    }

    /// Tile `col`, `row` in pixels: left, top, width, height.
    pub fn rect(&self, col: u32, row: u32) -> Option<[u32; 4]> {
        if col >= self.cols() || row >= self.rows() {
            return None;
        }
        let (x, y) = (col * self.side, row * self.side);
        let w = self.side.min(self.width - x);
        let h = self.side.min(self.height - y);
        Some([x, y, w, h])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: PageSize = PageSize {
        width: 612.0,
        height: 792.0,
    };

    #[test]
    fn tiles_cover_the_page_once_and_stay_within_the_side() {
        for scale in [2.0_f32, 6.0, 9.5, 20.0, 32.0] {
            let t = Tiling::new(LETTER, scale);
            let mut area = 0u64;
            for row in 0..t.rows() {
                for col in 0..t.cols() {
                    let [_, _, w, h] = t.rect(col, row).unwrap_or_default();
                    assert!(w <= TILE_SIDE && h <= TILE_SIDE && w > 0 && h > 0);
                    area += u64::from(w) * u64::from(h);
                }
            }
            assert_eq!(area, u64::from(t.width) * u64::from(t.height));
            assert!(t.width.max(t.height) <= MAX_TILED_SIDE);
            assert_eq!(t.rect(t.cols(), 0), None);
        }
    }

    #[test]
    fn tiles_grow_with_zoom_before_they_multiply() {
        // At 6×, 4752 px high: four rows of 1188.
        let t = Tiling::new(LETTER, 6.0);
        assert_eq!((t.side, t.rows()), (1188, 4));
        // At 32×, capped at 16,384 high: eight rows of 2048.
        let t = Tiling::new(LETTER, 32.0);
        assert_eq!((t.height, t.side, t.rows()), (MAX_TILED_SIDE, TILE_SIDE, 8));
    }
}
