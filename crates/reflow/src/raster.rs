//! Pages drawn in greys on white with vello_cpu; the page pass themes them.
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use parley::{Layout, PositionedLayoutItem};
use vello_cpu::color::palette::css::WHITE;
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, Rect};
use vello_cpu::peniko::{ImageQuality, ImageSampler};
use vello_cpu::{Glyph, Image, ImageSource, Pixmap, RenderContext, Resources};

use crate::flow::{Draw, Flow, Grid, MARKER_GAP, PAD, Piece, Set, span_height};
use crate::fonts::{Ink, Tone};
use crate::picture::{Decoded, Picture};

/// The largest side a raster may have, matching the PDF path.
pub const MAX_SIDE: u32 = 4096;

type Color = AlphaColor<Srgb>;

// mareader's text inks, as greys on white.
const BODY: Color = Color::from_rgba8(0, 0, 0, 255);
const SOFT: Color = Color::from_rgba8(85, 85, 85, 255);
const MUTED: Color = Color::from_rgba8(107, 107, 107, 255);
const CODE_GROUND: Color = Color::from_rgba8(0, 0, 0, 14);
const PRE_GROUND: Color = Color::from_rgba8(0, 0, 0, 12);
const BORDER: Color = Color::from_rgba8(0, 0, 0, 40);
const RULE: Color = Color::from_rgba8(0, 0, 0, 64);
const BAR: Color = Color::from_rgba8(0, 0, 0, 46);
const HEAD_GROUND: Color = Color::from_rgba8(0, 0, 0, 10);
/// Stands in for an image that won't decode.
const MISSING: Color = Color::from_rgba8(0, 0, 0, 24);
const BAR_WIDTH: f64 = 3.0;

fn tone(t: Tone) -> Color {
    match t {
        Tone::Body => BODY,
        Tone::Soft => SOFT,
        Tone::Muted => MUTED,
    }
}

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
        let resources = Resources::new();
        ctx.set_paint(WHITE);
        ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        let s = f64::from(scale);
        let mut p = Painter {
            ctx,
            resources,
            view: Affine::translate((-f64::from(x), -f64::from(y)))
                * Affine::scale(s)
                * Affine::translate((f64::from(PAD), f64::from(PAD))),
            // The page area in column units, to skip what's out of view.
            top: f64::from(y) / s - f64::from(PAD),
            bottom: f64::from(y + h) / s - f64::from(PAD),
            scale,
            coords: Vec::new(),
        };
        for piece in pieces {
            if let Some(set) = self.blocks.get(piece.block) {
                p.piece(set, piece, &self.decoded);
            }
        }
        let Painter {
            mut ctx,
            mut resources,
            ..
        } = p;
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

/// Draws in column units: the origin is the column's top left.
struct Painter {
    ctx: RenderContext,
    resources: Resources,
    view: Affine,
    top: f64,
    bottom: f64,
    scale: f32,
    coords: Vec<i16>,
}

impl Painter {
    fn fill(&mut self, color: Color, x: f32, y: f32, w: f32, h: f32) {
        self.ctx.set_transform(self.view);
        self.ctx.set_paint(color);
        let (x, y) = (f64::from(x), f64::from(y));
        let rect = Rect::new(x, y, x + f64::from(w), y + f64::from(h));
        self.ctx.fill_rect(&rect);
    }

    /// A one-unit outline inside `x, y, w, h`.
    fn frame(&mut self, color: Color, x: f32, y: f32, w: f32, h: f32) {
        self.fill(color, x, y, w, 1.0);
        self.fill(color, x, y + h - 1.0, w, 1.0);
        self.fill(color, x, y + 1.0, 1.0, h - 2.0);
        self.fill(color, x + w - 1.0, y + 1.0, 1.0, h - 2.0);
    }

    fn visible(&self, top: f32, bottom: f32) -> bool {
        f64::from(bottom) >= self.top && f64::from(top) <= self.bottom
    }

    fn piece(&mut self, set: &Set, piece: &Piece, decoded: &Decoded) {
        let units = piece.units.clone();
        let top = set.units.get(units.start).map_or(0.0, |u| u.0) + piece.dy;
        let height = span_height(set, units.clone());
        if !self.visible(top - 8.0, top + height + 8.0) {
            return;
        }
        for &bar in &set.bars {
            self.fill(BAR, bar, top, BAR_WIDTH as f32, height);
        }
        match &set.draw {
            Draw::Text {
                layout,
                marker,
                pad,
                shaded,
            } => {
                if *shaded {
                    self.fill(PRE_GROUND, set.x, top, set.width, height);
                    self.frame(BORDER, set.x, top, set.width, height);
                }
                let x = set.x + pad;
                if units.start == 0
                    && let Some(marker) = marker
                {
                    self.marker(layout, marker, x, piece.dy);
                }
                self.lines(layout, units, x, piece.dy);
            }
            Draw::Rule => {
                let y = top + height / 2.0 - 0.5;
                self.fill(RULE, set.x, y, set.width, 1.0);
            }
            Draw::Image { picture, rect } => {
                let [x, y, w, h] = *rect;
                self.image(picture, decoded, set.x + x, piece.dy + y, w, h);
            }
            Draw::Table(grid) => self.table(set, grid, units, piece.dy),
        }
    }

    /// Lines `lines` of `layout`, its origin at `x, dy`.
    fn lines(&mut self, layout: &Layout<Ink>, lines: Range<usize>, x: f32, dy: f32) {
        self.ctx
            .set_transform(self.view * Affine::translate((f64::from(x), f64::from(dy))));
        for line in lines.filter_map(|l| layout.get(l)) {
            let m = line.metrics();
            // Glyphs overflow their line box a little; keep a margin.
            let slack = m.line_height;
            if !self.visible(
                m.block_min_coord + dy - slack,
                m.block_max_coord + dy + slack,
            ) {
                continue;
            }
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else {
                    continue;
                };
                if run.style().brush.code {
                    // mareader's code ground: a pad around the run.
                    let size = run.run().font_size();
                    let (top, bottom) = (run.baseline() - size, run.baseline() + size * 0.3);
                    let left = x + run.offset() - size * 0.2;
                    let w = run.advance() + size * 0.4;
                    self.fill(CODE_GROUND, left, top + dy, w, bottom - top);
                    self.ctx.set_transform(
                        self.view * Affine::translate((f64::from(x), f64::from(dy))),
                    );
                }
            }
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyphs) = item else {
                    continue;
                };
                let run = glyphs.run();
                self.coords.clear();
                self.coords
                    .extend(run.normalized_coords().iter().map(|c| c.to_bits()));
                self.ctx.set_paint(tone(glyphs.style().brush.tone));
                let placed = glyphs.positioned_glyphs().map(|g| Glyph {
                    id: g.id,
                    x: g.x,
                    y: g.y,
                });
                // A glyph the font can't draw is left blank, not fatal.
                let _ = self
                    .ctx
                    .glyph_run(&mut self.resources, run.font())
                    .font_size(run.font_size())
                    .normalized_coords(&self.coords)
                    .fill_glyphs(placed);
            }
        }
    }

    /// The marker's right edge a gap left of `x`, on the first baseline.
    fn marker(&mut self, text: &Layout<Ink>, marker: &Layout<Ink>, x: f32, dy: f32) {
        let (Some(first), Some(own)) = (text.get(0), marker.get(0)) else {
            return;
        };
        let size = own
            .items()
            .find_map(|i| match i {
                PositionedLayoutItem::GlyphRun(g) => Some(g.run().font_size()),
                PositionedLayoutItem::InlineBox(_) => None,
            })
            .unwrap_or(0.0);
        let left = x - MARKER_GAP * size - marker.width();
        let shift = first.metrics().baseline - own.metrics().baseline;
        self.lines(marker, 0..1, left, dy + shift);
    }

    fn image(&mut self, picture: &Arc<Picture>, decoded: &Decoded, x: f32, y: f32, w: f32, h: f32) {
        let need = w.max(h) * self.scale;
        let Some(pixmap) = decoded.get(picture, need) else {
            return self.fill(MISSING, x, y, w, h);
        };
        let (pw, ph) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
        self.ctx
            .set_transform(self.view * Affine::translate((f64::from(x), f64::from(y))));
        self.ctx.set_paint_transform(Affine::scale_non_uniform(
            f64::from(w) / pw,
            f64::from(h) / ph,
        ));
        self.ctx.set_paint(Image {
            image: ImageSource::Pixmap(pixmap),
            sampler: ImageSampler::default().with_quality(ImageQuality::High),
        });
        self.ctx
            .fill_rect(&Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        self.ctx.reset_paint_transform();
    }

    fn table(&mut self, set: &Set, grid: &Grid, rows: Range<usize>, dy: f32) {
        let [pad_x, pad_y] = grid.pad;
        for r in rows {
            let (Some(&(top, bottom)), Some((cells, head))) = (set.units.get(r), grid.rows.get(r))
            else {
                continue;
            };
            let (top, h) = (top + dy, bottom - top);
            if *head {
                self.fill(HEAD_GROUND, set.x, top, set.width, h);
            }
            for (c, cell) in cells.iter().enumerate() {
                let x = set.x + c as f32 * grid.column;
                self.frame(BORDER, x, top, grid.column, h);
                self.lines(cell, 0..cell.len(), x + pad_x, top + pad_y);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::block::{Block, Content, Emphasis, Role, Text};
    use crate::flow::{Flow, PAGE_HEIGHT, PAGE_WIDTH, Typography};
    use crate::fonts::Fonts;
    use crate::picture::Picture;
    use crate::picture::tests::png;

    fn sample() -> Flow {
        let h = Block::text(Role::Heading(1), "Chapter One");
        let mut p = Text::new(Role::Paragraph);
        p.push(
            "It was a bright cold day in April, and ",
            Emphasis::default(),
        );
        p.push(
            "the clocks were striking thirteen.",
            Emphasis {
                italic: true,
                ..Emphasis::default()
            },
        );
        let blocks = [h, Block::new(Content::Text(p))];
        Flow::new(&blocks, &Typography::default(), &mut Fonts::bundled())
    }

    fn ink(px: &super::Pixels) -> usize {
        let (pixels, _) = px.rgba.as_chunks::<4>();
        pixels.iter().filter(|&&[r, ..]| r < 128).count()
    }

    fn pixel(p: &super::Pixels, x: u32, y: u32) -> Option<[u8; 4]> {
        let at = ((y * p.width + x) * 4) as usize;
        p.rgba.get(at..at + 4)?.try_into().ok()
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
        assert_eq!(px.and_then(|p| pixel(p, 0, 0)), Some([255, 255, 255, 255]));
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

    #[test]
    fn images_draw_their_pixels_and_junk_draws_a_box() {
        let good = Picture::probe(png(200, 100)).map(Arc::new);
        let blocks: Vec<Block> = good
            .into_iter()
            .chain([Arc::new(Picture::empty())])
            .map(|picture| {
                Block::new(Content::Image {
                    picture,
                    cover: false,
                })
            })
            .collect();
        let f = Flow::new(&blocks, &Typography::default(), &mut Fonts::bundled());
        let px = f.render(0, 1.0, [0, 0, 794, 400]);
        // Left of the picture is red, right is blue; below, a grey box.
        let red = px.as_ref().and_then(|p| pixel(p, 72 + 5, 72 + 50));
        assert!(red.is_some_and(|[r, _, b, _]| r > 200 && b < 60), "{red:?}");
        let blue = px.as_ref().and_then(|p| pixel(p, 72 + 195, 72 + 50));
        assert!(
            blue.is_some_and(|[r, _, b, _]| r < 60 && b > 200),
            "{blue:?}"
        );
        let gap = 17;
        let grey = px.as_ref().and_then(|p| pixel(p, 72, 72 + 100 + gap));
        assert!(grey.is_some_and(|[r, ..]| r < 250 && r > 200), "{grey:?}");
    }
}
