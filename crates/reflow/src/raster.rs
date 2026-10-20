//! Pages drawn in greys on white with vello_cpu; the page pass themes them.
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use parley::{GlyphRun, Layout, PositionedInlineBox, PositionedLayoutItem};
use skrifa::{FontRef, MetadataProvider};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Point, Rect, RoundedRect, Shape, Stroke};
use vello_cpu::peniko::{ImageAlphaType, ImageQuality, ImageSampler};
use vello_cpu::{Glyph, Image, ImageSource, Pixmap, RenderContext, Resources};

use crate::block::{Backdrop, BackdropSize, Edge};
use crate::flow::{
    Deco, Draw, Flow, Grid, Laid, MARKER_GAP, PAD, Para, Piece, Set, resolve, span_height,
};
use crate::fonts::{Ink, Tone};
use crate::picture::{Decoded, Picture};

/// The largest side a raster may have, matching the PDF path.
pub const MAX_SIDE: u32 = 4096;

type Color = AlphaColor<Srgb>;

// mareader's text inks as greys: the page pass maps black to the ink.
const BODY: Color = Color::from_rgba8(0, 0, 0, 255);
const SOFT: Color = Color::from_rgba8(85, 85, 85, 255);
const MUTED: Color = Color::from_rgba8(107, 107, 107, 255);
const CODE_GROUND: Color = Color::from_rgba8(0, 0, 0, 14);
const PRE_GROUND: Color = Color::from_rgba8(0, 0, 0, 12);
const BORDER: Color = Color::from_rgba8(0, 0, 0, 40);
const RULE: Color = Color::from_rgba8(0, 0, 0, 64);
/// The most dashes one border side draws.
const MAX_DASHES: usize = 4096;
const BAR: Color = Color::from_rgba8(0, 0, 0, 46);
const HEAD_GROUND: Color = Color::from_rgba8(0, 0, 0, 10);
/// Stands in for an image that won't decode.
const MISSING: Color = Color::from_rgba8(0, 0, 0, 24);
const BAR_WIDTH: f64 = 3.0;
/// mareader draws link underlines 2 px below where fonts put them.
const UNDERLINE_GAP: f32 = 2.0;
const UNDERLINE_MIN: f32 = 0.75;
/// The most tiles a repeated background draws.
const MAX_TILES: usize = 256;
/// Book pixels to an em.
const BOOK_EM_PX: f32 = 16.0;

fn tone(t: Tone) -> Color {
    match t {
        Tone::Body => BODY,
        Tone::Soft => SOFT,
        Tone::Muted => MUTED,
    }
}

/// Premultiplied RGBA8 rows, `width × height`; clear where nothing is drawn.
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
        let left = self.column(page).0;
        catch_unwind(AssertUnwindSafe(|| {
            let decos = self.decos.get(page).map_or(&[][..], Vec::as_slice);
            self.draw((pieces, decos), (scale, left), region)
        }))
        .ok()?
    }

    fn draw(
        &self,
        (pieces, decos): (&[Piece], &[Deco]),
        (scale, left): (f32, f32),
        region: Region,
    ) -> Option<Pixels> {
        let [x, y, w, h] = region;
        let (w16, h16) = (u16::try_from(w).ok()?, u16::try_from(h).ok()?);
        let ctx = RenderContext::new(w16, h16);
        let resources = Resources::new();
        let s = f64::from(scale);
        let mut p = Painter {
            ctx,
            resources,
            view: Affine::translate((-f64::from(x), -f64::from(y)))
                * Affine::scale(s)
                * Affine::translate((f64::from(left), f64::from(PAD))),
            // The page area in column units, to skip what's out of view.
            top: f64::from(y) / s - f64::from(PAD),
            bottom: f64::from(y + h) / s - f64::from(PAD),
            scale,
            coords: Vec::new(),
        };
        for deco in decos {
            p.deco(deco, &self.decoded);
        }
        for piece in pieces {
            if let Some(set) = self.blocks.get(piece.block) {
                p.piece(set, piece, &self.decoded, || self.laid(piece.block));
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
        // Handed over, not copied: a page at Retina scale is 40 MB.
        let rgba = pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied);
        Some(Pixels {
            width: w,
            height: h,
            rgba,
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

    /// Draws `piece` if in view; `laid` gives its layouts only then.
    fn piece(&mut self, set: &Set, piece: &Piece, decoded: &Decoded, laid: impl Fn() -> Arc<Laid>) {
        if set.rotate == 0.0 || !set.rotate.is_finite() {
            return self.unturned(set, piece, decoded, laid);
        }
        // A turn is drawn about the piece's middle; it takes no room.
        let units = piece.units.clone();
        let top = set.units.get(units.start).map_or(0.0, |u| u.0) + piece.dy;
        let middle = Point::new(
            f64::from(set.x + set.width / 2.0),
            f64::from(top + span_height(set, units) / 2.0),
        );
        let view = self.view;
        let turn = f64::from(set.rotate).to_radians();
        self.view = view * Affine::rotate_about(turn, middle);
        self.unturned(set, piece, decoded, laid);
        self.view = view;
    }

    fn unturned(
        &mut self,
        set: &Set,
        piece: &Piece,
        decoded: &Decoded,
        laid: impl Fn() -> Arc<Laid>,
    ) {
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
            Draw::Text { pad, shaded } => {
                if *shaded {
                    self.fill(PRE_GROUND, set.x, top, set.width, height);
                    self.frame(BORDER, set.x, top, set.width, height);
                }
                let laid = laid();
                let Laid::Text { para, marker } = &*laid else {
                    return;
                };
                let x = set.x + pad;
                if units.start == 0
                    && let Some(marker) = marker
                {
                    self.marker(&para.layout, marker, x, piece.dy);
                }
                self.lines(para, units, x, piece.dy);
                self.inlines(para, piece.units.clone(), (x, piece.dy), decoded);
            }
            Draw::Rule => {
                let y = top + height / 2.0 - 0.5;
                self.fill(RULE, set.x, y, set.width, 1.0);
            }
            Draw::Image { picture, rect } => {
                let [x, y, w, h] = *rect;
                self.image(picture, decoded, set.x + x, piece.dy + y, w, h);
            }
            Draw::Table(grid) => {
                if let Laid::Table(rows) = &*laid() {
                    self.table(set, (grid, rows), units, piece.dy);
                }
            }
            Draw::Space => {}
        }
    }

    /// A book box's borders, in the ink the page pass themes.
    fn deco(&mut self, deco: &Deco, decoded: &Decoded) {
        let [x, y, w, h] = deco.rect;
        if !self.visible(y - 8.0, y + h + 8.0) {
            return;
        }
        let radius = deco.radius.min(w / 2.0).min(h / 2.0);
        if radius >= 0.5 {
            return self.rounded(deco, radius, decoded);
        }
        if let Some(alpha) = deco.fill {
            self.fill(BODY.with_alpha(alpha.clamp(0.0, 1.0)), x, y, w, h);
        }
        if let Some(backdrop) = &deco.backdrop {
            self.backdrop(backdrop, deco.em, deco.rect, decoded);
        }
        let [top, right, bottom, left] = deco.border;
        let [et, er, eb, el] = deco.edges;
        self.edge(et, [x, y, w, top], true);
        self.edge(eb, [x, y + h - bottom, w, bottom], true);
        self.edge(el, [x, y, left, h], false);
        self.edge(er, [x + w - right, y, right, h], false);
    }

    /// A box with round corners: its ground, picture and borders.
    fn rounded(&mut self, deco: &Deco, radius: f32, decoded: &Decoded) {
        let [x, y, w, h] = deco.rect;
        let shape = |inset: f32| {
            let r = f64::from((radius - inset).max(0.0));
            let (l, t) = (f64::from(x + inset), f64::from(y + inset));
            let (rr, b) = (f64::from(x + w - inset), f64::from(y + h - inset));
            RoundedRect::new(l, t, rr.max(l), b.max(t), r).to_path(0.1)
        };
        self.ctx.set_transform(self.view);
        if let Some(alpha) = deco.fill {
            self.ctx.set_paint(BODY.with_alpha(alpha.clamp(0.0, 1.0)));
            self.ctx.fill_path(&shape(0.0));
        }
        if let Some(backdrop) = &deco.backdrop {
            self.ctx.push_clip_layer(&shape(0.0));
            self.backdrop(backdrop, deco.em, deco.rect, decoded);
            self.ctx.set_transform(self.view);
            self.ctx.pop_layer();
        }
        let first = deco.edges[0];
        let same = deco.edges.iter().all(|e| *e == first);
        if !same {
            // Mixed sides keep square corners.
            let [top, right, bottom, left] = deco.border;
            let [et, er, eb, el] = deco.edges;
            self.edge(et, [x, y, w, top], true);
            self.edge(eb, [x, y + h - bottom, w, bottom], true);
            self.edge(el, [x, y, left, h], false);
            self.edge(er, [x + w - right, y, right, h], false);
            return;
        }
        let least = 1.0 / self.scale.max(0.01);
        let thick = deco.border.iter().fold(0.0f32, |a, b| a.max(*b)).max(least);
        let line = |inset: f32, width: f32, dashes: Option<[f64; 2]>| {
            let stroke = Stroke::new(f64::from(width));
            let stroke = match dashes {
                Some(d) => stroke.with_dashes(0.0, d),
                None => stroke,
            };
            (shape(inset), stroke)
        };
        let t = f64::from(thick);
        let strokes: Vec<(BezPath, Stroke)> = match first {
            Edge::None => Vec::new(),
            Edge::Double if thick * self.scale >= 3.0 => {
                let third = thick / 3.0;
                vec![
                    line(third / 2.0, third, None),
                    line(thick - third / 2.0, third, None),
                ]
            }
            Edge::Solid | Edge::Double => vec![line(thick / 2.0, thick, None)],
            Edge::Dashed => vec![line(thick / 2.0, thick, Some([t * 3.0, t * 2.0]))],
            Edge::Dotted => vec![line(thick / 2.0, thick, Some([t, t]))],
        };
        self.ctx.set_transform(self.view);
        self.ctx.set_paint(BODY);
        for (path, stroke) in strokes {
            self.ctx.set_stroke(stroke);
            self.ctx.stroke_path(&path);
        }
    }

    /// A background picture in `rect`, sized and placed as the book asks.
    fn backdrop(&mut self, b: &Backdrop, em: f32, rect: [f32; 4], decoded: &Decoded) {
        let [x, y, w, h] = rect;
        let px = em / BOOK_EM_PX;
        let (pw, ph) = (b.picture.width as f32 * px, b.picture.height as f32 * px);
        if pw < 0.5 || ph < 0.5 || w < 0.5 || h < 0.5 {
            return;
        }
        let (iw, ih) = match b.size {
            BackdropSize::Auto => (pw, ph),
            BackdropSize::Cover => {
                let s = (w / pw).max(h / ph);
                (pw * s, ph * s)
            }
            BackdropSize::Contain => {
                let s = (w / pw).min(h / ph);
                (pw * s, ph * s)
            }
            BackdropSize::Set([sw, sh]) => {
                let sw = sw.map(|l| resolve(l, w, em));
                let sh = sh.map(|l| resolve(l, h, em));
                match (sw, sh) {
                    (Some(a), Some(b)) => (a, b),
                    (Some(a), None) => (a, a * ph / pw),
                    (None, Some(b)) => (b * pw / ph, b),
                    (None, None) => (pw, ph),
                }
            }
        };
        if !(iw >= 0.5 && ih >= 0.5 && iw.is_finite() && ih.is_finite()) {
            return;
        }
        let [ax, ay] = b.at.map(|a| a.clamp(0.0, 1.0));
        let (ox, oy) = (x + (w - iw) * ax, y + (h - ih) * ay);
        let (mut xs, mut ys) = (vec![ox], vec![oy]);
        if b.repeat {
            // Tiles from the box's corner past its far edges.
            let first = |o: f32, start: f32, step: f32| o - ((o - start) / step).ceil() * step;
            let tiles = |from: f32, end: f32, step: f32| {
                std::iter::successors(Some(from), move |t| Some(t + step))
                    .take_while(move |t| *t < end)
                    .take(MAX_TILES)
                    .collect::<Vec<f32>>()
            };
            xs = tiles(first(ox, x, iw), x + w, iw);
            ys = tiles(first(oy, y, ih), y + h, ih);
        }
        let mut drawn = 0;
        for ty in &ys {
            for tx in &xs {
                if drawn >= MAX_TILES {
                    return;
                }
                self.clipped(&b.picture, decoded, [*tx, *ty, iw, ih], rect);
                drawn += 1;
            }
        }
    }

    /// `picture` drawn at `at`, only where it overlaps `clip`.
    fn clipped(&mut self, picture: &Arc<Picture>, decoded: &Decoded, at: [f32; 4], clip: [f32; 4]) {
        let [x, y, w, h] = at;
        let [cx, cy, cw, ch] = clip;
        let (l, t) = (x.max(cx), y.max(cy));
        let (r, b) = ((x + w).min(cx + cw), (y + h).min(cy + ch));
        if r <= l || b <= t || !self.visible(t, b) {
            return;
        }
        let Some(pixmap) = decoded.get(picture, w.max(h) * self.scale) else {
            return;
        };
        let (pw, ph) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
        self.ctx
            .set_transform(self.view * Affine::translate((f64::from(x), f64::from(y))));
        let scale = Affine::scale_non_uniform(f64::from(w) / pw, f64::from(h) / ph);
        self.ctx.set_paint_transform(scale);
        self.ctx.set_paint(Image {
            image: ImageSource::Pixmap(pixmap),
            sampler: ImageSampler::default().with_quality(ImageQuality::High),
        });
        let (l, t, r, b) = (l - x, t - y, r - x, b - y);
        let shown = Rect::new(f64::from(l), f64::from(t), f64::from(r), f64::from(b));
        self.ctx.fill_rect(&shown);
        self.ctx.reset_paint_transform();
    }

    /// One border side in `rect`, its lines running across if `across`.
    fn edge(&mut self, edge: Edge, rect: [f32; 4], across: bool) {
        let [x, y, w, h] = rect;
        // Hairlines stay visible at small scales.
        let least = 1.0 / self.scale.max(0.01);
        let (w, h) = match across {
            true => (w, h.max(least)),
            false => (w.max(least), h),
        };
        let thick = if across { h } else { w };
        match edge {
            Edge::None => {}
            Edge::Double if thick * self.scale >= 3.0 => {
                let line = thick / 3.0;
                let (far_x, far_y) = if across {
                    (x, y + h - line)
                } else {
                    (x + w - line, y)
                };
                let (lw, lh) = if across { (w, line) } else { (line, h) };
                self.fill(BODY, x, y, lw, lh);
                self.fill(BODY, far_x, far_y, lw, lh);
            }
            Edge::Solid | Edge::Double => self.fill(BODY, x, y, w, h),
            Edge::Dashed | Edge::Dotted => {
                let dash = if edge == Edge::Dashed {
                    thick * 3.0
                } else {
                    thick
                };
                let length = if across { w } else { h };
                let step = (dash * 2.0).max(least * 2.0);
                let count = ((length / step).ceil() as usize).min(MAX_DASHES);
                for i in 0..count {
                    let at = i as f32 * step;
                    let run = dash.min(length - at);
                    match across {
                        true => self.fill(BODY, x + at, y, run, h),
                        false => self.fill(BODY, x, y + at, w, run),
                    }
                }
            }
        }
    }

    /// The pictures in lines `lines` of `para`, its origin at `x, dy`.
    fn inlines(
        &mut self,
        para: &Para,
        lines: Range<usize>,
        (x, dy): (f32, f32),
        decoded: &Decoded,
    ) {
        if para.pictures.is_empty() {
            return;
        }
        let boxes: Vec<PositionedInlineBox> = lines
            .filter_map(|l| para.layout.get(l))
            .flat_map(|line| line.items().collect::<Vec<_>>())
            .filter_map(|item| match item {
                PositionedLayoutItem::InlineBox(b) => Some(b),
                PositionedLayoutItem::GlyphRun(_) => None,
            })
            .collect();
        for b in boxes {
            let picture = usize::try_from(b.id)
                .ok()
                .and_then(|i| para.pictures.get(i));
            if let Some(picture) = picture {
                self.image(picture, decoded, x + b.x, dy + b.y, b.width, b.height);
            }
        }
    }

    /// Lines `lines` of `para`, its origin at `x, dy`.
    fn lines(&mut self, para: &Para, lines: Range<usize>, x: f32, dy: f32) {
        let layout = &para.layout;
        self.ctx
            .set_transform(self.view * Affine::translate((f64::from(x), f64::from(dy))));
        for (n, line) in lines.filter_map(|l| Some((l, layout.get(l)?))) {
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
                if run.style().underline.is_some() {
                    self.underline(&run, x, dy);
                }
                if run.style().strikethrough.is_some() {
                    self.strike(&run, x, dy);
                }
            }
            let mut last = None;
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyphs) = item else {
                    continue;
                };
                let placed = glyphs.positioned_glyphs().map(|g| Glyph {
                    id: g.id,
                    x: g.x,
                    y: g.y,
                });
                self.glyphs(&glyphs, placed);
                last = Some(glyphs);
            }
            // parley breaks at a soft hyphen but leaves the hyphen to us.
            if let Some(run) = last.filter(|_| para.hyphens.binary_search(&n).is_ok()) {
                let font = run.run().font();
                let id = FontRef::from_index(font.data.as_ref(), font.index)
                    .ok()
                    .and_then(|f| f.charmap().map('-'));
                if let Some(id) = id {
                    let hyphen = Glyph {
                        id: id.to_u32(),
                        x: run.offset() + run.advance(),
                        y: run.baseline(),
                    };
                    self.glyphs(&run, std::iter::once(hyphen));
                }
            }
        }
    }

    /// A link's underline under `run`, a little clear of the glyphs.
    fn underline(&mut self, run: &GlyphRun<'_, Ink>, x: f32, dy: f32) {
        let m = run.run().font_metrics();
        let size = m.underline_size.max(UNDERLINE_MIN);
        let y = run.baseline() + m.underline_offset.abs() + UNDERLINE_GAP;
        let ink = tone(run.style().brush.tone);
        self.fill(ink, x + run.offset(), y + dy, run.advance(), size);
        self.ctx
            .set_transform(self.view * Affine::translate((f64::from(x), f64::from(dy))));
    }

    /// A line through `run`, at its font's strikeout height.
    fn strike(&mut self, run: &GlyphRun<'_, Ink>, x: f32, dy: f32) {
        let m = run.run().font_metrics();
        let size = m.strikethrough_size.max(UNDERLINE_MIN);
        let y = run.baseline() - m.strikethrough_offset.abs() - size / 2.0;
        let ink = tone(run.style().brush.tone);
        self.fill(ink, x + run.offset(), y + dy, run.advance(), size);
        self.ctx
            .set_transform(self.view * Affine::translate((f64::from(x), f64::from(dy))));
    }

    /// `placed` in `run`'s font, size, variation and tone.
    fn glyphs(&mut self, run: &GlyphRun<'_, Ink>, placed: impl Iterator<Item = Glyph> + Clone) {
        let font = run.run();
        self.coords.clear();
        self.coords
            .extend(font.normalized_coords().iter().map(|c| c.to_bits()));
        self.ctx.set_paint(tone(run.style().brush.tone));
        // A glyph the font can't draw is left blank, not fatal.
        let _ = self
            .ctx
            .glyph_run(&mut self.resources, font.font())
            .font_size(font.font_size())
            .normalized_coords(&self.coords)
            .fill_glyphs(placed);
    }

    /// The marker's right edge a gap left of `x`, on the first baseline.
    fn marker(&mut self, text: &Layout<Ink>, marker: &Para, x: f32, dy: f32) {
        let (Some(first), Some(own)) = (text.get(0), marker.layout.get(0)) else {
            return;
        };
        let size = own
            .items()
            .find_map(|i| match i {
                PositionedLayoutItem::GlyphRun(g) => Some(g.run().font_size()),
                PositionedLayoutItem::InlineBox(_) => None,
            })
            .unwrap_or(0.0);
        let left = x - MARKER_GAP * size - marker.layout.width();
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

    fn table(
        &mut self,
        set: &Set,
        (grid, laid): (&Grid, &[Vec<Para>]),
        rows: Range<usize>,
        dy: f32,
    ) {
        let [pad_x, pad_y] = grid.pad;
        for r in rows {
            let (Some(&(top, bottom)), Some(cells), Some(head)) =
                (set.units.get(r), laid.get(r), grid.heads.get(r))
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
                self.lines(cell, 0..cell.layout.len(), x + pad_x, top + pad_y);
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
        Flow::new(
            Arc::new(blocks.to_vec()),
            &Typography::default(),
            Fonts::bundled(),
        )
    }

    fn ink(px: &super::Pixels) -> usize {
        let (pixels, _) = px.rgba.as_chunks::<4>();
        pixels
            .iter()
            .filter(|&&[r, .., a]| a > 128 && r < 128)
            .count()
    }

    fn pixel(p: &super::Pixels, x: u32, y: u32) -> Option<[u8; 4]> {
        let at = ((y * p.width + x) * 4) as usize;
        p.rgba.get(at..at + 4)?.try_into().ok()
    }

    #[test]
    fn a_page_draws_dark_text_on_clear() {
        let f = sample();
        let (w, h) = (PAGE_WIDTH as u32 / 2, PAGE_HEIGHT as u32 / 2);
        let px = f.render(0, 0.5, [0, 0, w, h]);
        let px = px.as_ref();
        assert_eq!(px.map(|p| (p.width, p.height)), Some((w, h)));
        let dark = px.map_or(0, ink);
        assert!(dark > 200, "{dark} dark pixels");
        // The corner is margin: clear, so the paper shows through.
        assert_eq!(px.and_then(|p| pixel(p, 0, 0)), Some([0, 0, 0, 0]));
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
                    width: None,
                })
            })
            .collect();
        let f = Flow::new(Arc::new(blocks), &Typography::default(), Fonts::bundled());
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
        assert!(grey.is_some_and(|[.., a]| a > 10 && a < 50), "{grey:?}");
    }
}
