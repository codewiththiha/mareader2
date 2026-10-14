//! Pages drawn black on white with vello_cpu; the page pass themes them.
use std::panic::{AssertUnwindSafe, catch_unwind};

use parley::PositionedLayoutItem;
use vello_cpu::color::palette::css::{BLACK, WHITE};
use vello_cpu::kurbo::{Affine, Rect};
use vello_cpu::{Glyph, Pixmap, RenderContext, Resources};

use crate::flow::{Flow, PAD, Piece};

/// The largest side a raster may have, matching the PDF path.
pub const MAX_SIDE: u32 = 4096;

/// Premultiplied, opaque RGBA8 rows, `width × height`.
#[derive(Clone, PartialEq, Eq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Pixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pixels({}×{})", self.width, self.height)
    }
}

/// A part of a page drawn at some scale: `[x, y, width, height]` pixels.
pub type Region = [u32; 4];

impl Flow {
    /// `region` of page `page` at `scale` pixels per unit; never panics.
    pub fn render(&self, page: usize, scale: f32, region: Region) -> Option<Pixels> {
        let pieces = self.pages.get(page)?;
        let [_, _, w, h] = region;
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE || scale.is_nan() || scale <= 0.0 {
            return None;
        }
        catch_unwind(AssertUnwindSafe(|| self.draw(pieces, scale, region))).ok()?
    }

    fn draw(&self, pieces: &[Piece], scale: f32, region: Region) -> Option<Pixels> {
        let [x, y, w, h] = region;
        let (w16, h16) = (u16::try_from(w).ok()?, u16::try_from(h).ok()?);
        let mut ctx = RenderContext::new(w16, h16);
        let mut resources = Resources::new();
        ctx.set_paint(WHITE);
        ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        ctx.set_paint(BLACK);
        let view = Affine::translate((-f64::from(x), -f64::from(y)))
            * Affine::scale(f64::from(scale))
            * Affine::translate((f64::from(PAD), f64::from(PAD)));
        // The page area in layout units, to skip lines out of view.
        let s = f64::from(scale);
        let (top, bottom) = (f64::from(y) / s, f64::from(y + h) / s);
        let mut coords = Vec::new();
        for piece in pieces {
            let Some(set) = self.blocks.get(piece.block) else {
                continue;
            };
            ctx.set_transform(view * Affine::translate((0.0, f64::from(piece.dy))));
            for line in piece.lines.clone().filter_map(|l| set.layout.get(l)) {
                let m = line.metrics();
                let (lt, lb) = (m.block_min_coord + piece.dy, m.block_max_coord + piece.dy);
                // Glyphs overflow their line box a little; keep a margin.
                let slack = f64::from(m.line_height);
                let (lt, lb) = (f64::from(lt + PAD), f64::from(lb + PAD));
                if lb + slack < top || lt - slack > bottom {
                    continue;
                }
                for item in line.items() {
                    let PositionedLayoutItem::GlyphRun(glyphs) = item else {
                        continue;
                    };
                    let run = glyphs.run();
                    coords.clear();
                    coords.extend(run.normalized_coords().iter().map(|c| c.to_bits()));
                    let placed = glyphs.positioned_glyphs().map(|g| Glyph {
                        id: g.id,
                        x: g.x,
                        y: g.y,
                    });
                    // A glyph the font can't draw is left blank, not fatal.
                    let _ = ctx
                        .glyph_run(&mut resources, run.font())
                        .font_size(run.font_size())
                        .normalized_coords(&coords)
                        .fill_glyphs(placed);
                }
            }
        }
        let mut pixmap = Pixmap::new(w16, h16);
        ctx.flush();
        ctx.render(&mut pixmap, &mut resources);
        Some(Pixels {
            width: w,
            height: h,
            rgba: pixmap.data_as_u8_slice().to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::block::{Block, Emphasis, Role};
    use crate::flow::{Flow, PAGE_HEIGHT, PAGE_WIDTH, Typography};
    use crate::fonts::Fonts;

    fn sample() -> Flow {
        let mut h = Block::new(Role::Heading(1));
        h.push("Chapter One", Emphasis::default());
        let mut p = Block::new(Role::Paragraph);
        p.push(
            "It was a bright cold day in April, and ",
            Emphasis::default(),
        );
        p.push(
            "the clocks were striking thirteen.",
            Emphasis {
                italic: true,
                bold: false,
            },
        );
        Flow::new(&[h, p], &Typography::default(), &mut Fonts::bundled())
    }

    fn ink(px: &super::Pixels) -> usize {
        let (pixels, _) = px.rgba.as_chunks::<4>();
        pixels.iter().filter(|&&[r, ..]| r < 128).count()
    }

    #[test]
    fn a_page_draws_dark_text_on_white() {
        let f = sample();
        let (w, h) = (PAGE_WIDTH as u32 / 2, PAGE_HEIGHT as u32 / 2);
        let px = f.render(0, 0.5, [0, 0, w, h]);
        let px = px.as_ref();
        assert_eq!(px.map(|p| (p.width, p.height)), Some((w, h)));
        let dark = px.map_or(0, ink);
        assert!(dark > 200, "{dark} dark pixels");
        // The corner is margin: paper white.
        assert_eq!(
            px.and_then(|p| p.rgba.get(..4)),
            Some(&[255, 255, 255, 255][..])
        );
    }

    #[test]
    fn a_tile_matches_the_same_area_of_the_whole_page() {
        let f = sample();
        let whole = f.render(0, 1.0, [0, 0, 794, 400]);
        let tile = f.render(0, 1.0, [64, 64, 256, 128]);
        assert!(whole.is_some() && tile.is_some(), "both draw");
        let (Some(whole), Some(tile)) = (whole, tile) else {
            return;
        };
        let row = |p: &super::Pixels, x: u32, y: u32, n: u32| {
            let at = ((y * p.width + x) * 4) as usize;
            p.rgba.get(at..at + (n * 4) as usize).map(<[u8]>::to_vec)
        };
        for y in (0..128).step_by(9) {
            assert_eq!(
                row(&whole, 64, 64 + y, 256),
                row(&tile, 0, y, 256),
                "row {y}"
            );
        }
    }

    #[test]
    fn out_of_range_requests_draw_nothing() {
        let f = sample();
        assert!(f.render(9, 1.0, [0, 0, 10, 10]).is_none());
        assert!(f.render(0, 1.0, [0, 0, 0, 10]).is_none());
        assert!(f.render(0, f32::NAN, [0, 0, 10, 10]).is_none());
    }
}
