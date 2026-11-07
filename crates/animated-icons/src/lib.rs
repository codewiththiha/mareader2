//! Generated resting glyphs and bounded part atlases for Lucide icons.

use tiny_skia::{
    FillRule as SkFillRule, LineCap as SkLineCap, LineJoin as SkLineJoin, Paint, Path, PathBuilder,
    Pixmap, PixmapPaint, Stroke as SkStroke, StrokeDash, Transform,
};

mod prop;
use prop::Prop;

pub const GRID: f32 = 24.0;
pub const PAD: f32 = 8.5;
pub const SPAN: f32 = GRID + 2.0 * PAD;
pub const FRAME_RATE: f32 = 60.0;
pub const MAX_SIDE: u32 = 192;
pub const MAX_ATLAS_BYTES: usize = 8 * 1024 * 1024;

struct Seg {
    part: u8,
    prop: Prop,
    t0: f32,
    dur: f32,
    keys: &'static [f32],
    times: &'static [f32],
    eases: &'static [u8],
}

struct Part {
    ox: f32,
    oy: f32,
    len: f32,
    width: f32,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum Command {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum FillRule {
    Winding,
    EvenOdd,
}

#[derive(Clone, Copy)]
struct Fill {
    opacity: f32,
    rule: FillRule,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum LineCap {
    Butt,
    Round,
    Square,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum LineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Clone, Copy)]
struct Stroke {
    opacity: f32,
    width: f32,
    cap: LineCap,
    join: LineJoin,
    miter: f32,
}

struct Shape {
    commands: &'static [Command],
    fill: Option<Fill>,
    stroke: Option<Stroke>,
}

enum Node {
    Group {
        part: Option<u8>,
        width_part: Option<u8>,
        opacity: f32,
        children: &'static [Node],
    },
    Shape(&'static Shape),
}

struct Layer {
    parts: &'static [u8],
    scene: &'static [Node],
}

pub struct Pack {
    id: &'static str,
    dur: f32,
    parts: &'static [Part],
    segs: &'static [Seg],
    layers: &'static [Layer],
}

impl Pack {
    pub fn id(&self) -> &'static str {
        self.id
    }

    pub fn duration(&self) -> f32 {
        self.dur
    }

    pub fn frame_count(&self) -> u16 {
        (self.dur * FRAME_RATE)
            .ceil()
            .clamp(1.0, f32::from(u16::MAX)) as u16
    }
}

#[allow(clippy::approx_constant, clippy::excessive_precision)]
mod generated {
    use super::*;

    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

use generated::{EASINGS, PACKS};

pub fn find(id: &str) -> Option<&'static Pack> {
    PACKS.iter().copied().find(|pack| pack.id == id)
}

pub fn all() -> impl ExactSizeIterator<Item = &'static Pack> {
    PACKS.iter().copied()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasKind {
    Parts,
}

#[derive(Clone, Copy, Debug)]
pub struct AtlasCell {
    pub atlas_x: u32,
    pub atlas_y: u32,
    pub target_x: u32,
    pub target_y: u32,
    pub width: u32,
    pub height: u32,
}

pub struct Atlas {
    pub kind: AtlasKind,
    pub width: u32,
    pub height: u32,
    pub side: u32,
    pub frames: u16,
    pub layers: u16,
    pub cells: Vec<AtlasCell>,
    pub pixels: Vec<u8>,
}

impl Atlas {
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }

    pub fn cell(&self, frame: u16, layer: u16) -> Option<AtlasCell> {
        let frame = frame.min(self.frames.saturating_sub(1));
        let layer = layer.min(self.layers.saturating_sub(1));
        let index = usize::from(frame)
            .checked_mul(usize::from(self.layers))?
            .checked_add(usize::from(layer))?;
        self.cells.get(index).copied()
    }

    pub fn uv(&self, cell: AtlasCell) -> [f32; 4] {
        [
            cell.atlas_x as f32 / self.width.max(1) as f32,
            cell.atlas_y as f32 / self.height.max(1) as f32,
            cell.atlas_x.saturating_add(cell.width) as f32 / self.width.max(1) as f32,
            cell.atlas_y.saturating_add(cell.height) as f32 / self.height.max(1) as f32,
        ]
    }
}

/// One composited resting glyph cropped from its 24-unit canvas.
pub struct RestingGlyph {
    pub side: u32,
    pub target_x: u32,
    pub target_y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RestingGlyph {
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }
}

/// Rasterises only the canonical first sample, never the animation atlas.
pub fn raster_resting(pack: &'static Pack, requested_side: u32) -> Option<RestingGlyph> {
    let side = requested_side.clamp(8, MAX_SIDE);
    let vals = values(pack, 0.0);
    let mut pixmap = Pixmap::new(side, side)?;
    let scale = side as f32 / GRID;
    let canvas = Transform::from_row(scale, 0.0, 0.0, scale, 0.0, 0.0);
    for layer in pack.layers {
        render_nodes(
            layer.scene,
            pack.parts,
            &vals,
            None,
            None,
            canvas,
            &mut pixmap,
        )?;
    }
    let (target_x, target_y, width, height, pixels) = crop(&pixmap)?;
    Some(RestingGlyph {
        side,
        target_x,
        target_y,
        width,
        height,
        pixels,
    })
}

pub fn raster_atlas(pack: &'static Pack, requested_side: u32) -> Option<Atlas> {
    raster_part_atlas(pack, requested_side.clamp(8, MAX_SIDE))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LayerKey {
    layer: u16,
    state: Vec<u32>,
}

struct Variant {
    key: LayerKey,
    target_x: u32,
    target_y: u32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    atlas_x: u32,
    atlas_y: u32,
}

fn raster_part_atlas(pack: &'static Pack, side: u32) -> Option<Atlas> {
    let frames = pack.frame_count();
    let layers = u16::try_from(pack.layers.len()).ok()?;
    if layers == 0 {
        return None;
    }
    let mut variants: Vec<Variant> = Vec::new();
    let mut mapping = Vec::with_capacity(usize::from(frames) * usize::from(layers));
    let mut variant_bytes = 0usize;
    for frame in 0..frames {
        let time = f32::from(frame) / FRAME_RATE;
        let vals = values(pack, time);
        for (layer_index, layer) in pack.layers.iter().enumerate() {
            let key = layer_key(layer, &vals, u16::try_from(layer_index).ok()?)?;
            let variant = variants.iter().position(|variant| variant.key == key);
            let index = if let Some(index) = variant {
                index
            } else {
                let variant = raster_layer(pack, layer, &vals, side, key)?;
                variant_bytes = variant_bytes.checked_add(variant.pixels.len())?;
                if variant_bytes > MAX_ATLAS_BYTES {
                    return None;
                }
                variants.push(variant);
                variants.len() - 1
            };
            mapping.push(index);
        }
    }
    let (width, height) = pack_variants(&mut variants)?;
    let byte_len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if byte_len > MAX_ATLAS_BYTES {
        return None;
    }
    let mut pixels = vec![0; byte_len];
    for variant in &variants {
        copy_rect(
            &variant.pixels,
            variant.width,
            variant.height,
            variant.atlas_x,
            variant.atlas_y,
            width,
            &mut pixels,
        )?;
    }
    let cells = mapping
        .into_iter()
        .map(|index| {
            let variant = variants.get(index)?;
            Some(AtlasCell {
                atlas_x: variant.atlas_x,
                atlas_y: variant.atlas_y,
                target_x: variant.target_x,
                target_y: variant.target_y,
                width: variant.width,
                height: variant.height,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Atlas {
        kind: AtlasKind::Parts,
        width,
        height,
        side,
        frames,
        layers,
        cells,
        pixels,
    })
}

#[cfg(test)]
fn raster_frame(pack: &'static Pack, frame: u16, requested_side: u32) -> Option<Vec<u8>> {
    let side = requested_side.clamp(8, MAX_SIDE);
    let time = f32::from(frame.min(pack.frame_count().saturating_sub(1))) / FRAME_RATE;
    let vals = values(pack, time);
    let mut pixmap = Pixmap::new(side, side)?;
    let scale = side as f32 / SPAN;
    let canvas = Transform::from_row(scale, 0.0, 0.0, scale, PAD * scale, PAD * scale);
    for layer in pack.layers {
        render_nodes(
            layer.scene,
            pack.parts,
            &vals,
            None,
            None,
            canvas,
            &mut pixmap,
        )?;
    }
    Some(pixmap.take())
}

#[derive(Clone, Copy)]
struct Vals {
    x: f32,
    y: f32,
    scale: f32,
    scale_x: f32,
    scale_y: f32,
    rot: f32,
    skew_x: f32,
    skew_y: f32,
    op: f32,
    len: f32,
    off: f32,
    width: f32,
}

impl Vals {
    fn rest(part: &Part) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rot: 0.0,
            skew_x: 0.0,
            skew_y: 0.0,
            op: 1.0,
            len: 1.0,
            off: 0.0,
            width: part.width,
        }
    }
}

fn values(pack: &Pack, time: f32) -> Vec<Vals> {
    let mut vals: Vec<Vals> = pack.parts.iter().map(Vals::rest).collect();
    for seg in pack.segs {
        let Some(value) = vals.get_mut(seg.part as usize) else {
            continue;
        };
        let x = at(seg, clock(seg, time));
        match seg.prop {
            Prop::X => value.x = x,
            Prop::Y => value.y = x,
            Prop::Scale => value.scale = x,
            Prop::ScaleX => value.scale_x = x,
            Prop::ScaleY => value.scale_y = x,
            Prop::Rotate => value.rot = x,
            Prop::SkewX => value.skew_x = x,
            Prop::SkewY => value.skew_y = x,
            Prop::Opacity => value.op = x,
            Prop::PathLen => value.len = x,
            Prop::PathOff => value.off = x,
            Prop::StrokeW => value.width = x,
        }
    }
    vals
}

fn layer_key(layer: &Layer, vals: &[Vals], index: u16) -> Option<LayerKey> {
    let mut state = Vec::with_capacity(layer.parts.len().saturating_mul(12));
    for part in layer.parts {
        let value = vals.get(usize::from(*part))?;
        state.extend([
            value.x.to_bits(),
            value.y.to_bits(),
            value.scale.to_bits(),
            value.scale_x.to_bits(),
            value.scale_y.to_bits(),
            value.rot.to_bits(),
            value.skew_x.to_bits(),
            value.skew_y.to_bits(),
            value.op.to_bits(),
            value.len.to_bits(),
            value.off.to_bits(),
            value.width.to_bits(),
        ]);
    }
    Some(LayerKey {
        layer: index,
        state,
    })
}

fn raster_layer(
    pack: &Pack,
    layer: &Layer,
    vals: &[Vals],
    side: u32,
    key: LayerKey,
) -> Option<Variant> {
    let mut pixmap = Pixmap::new(side, side)?;
    let scale = side as f32 / SPAN;
    let canvas = Transform::from_row(scale, 0.0, 0.0, scale, PAD * scale, PAD * scale);
    render_nodes(
        layer.scene,
        pack.parts,
        vals,
        None,
        None,
        canvas,
        &mut pixmap,
    )?;
    let (target_x, target_y, width, height, pixels) = crop(&pixmap)?;
    Some(Variant {
        key,
        target_x,
        target_y,
        width,
        height,
        pixels,
        atlas_x: 0,
        atlas_y: 0,
    })
}

fn crop(pixmap: &Pixmap) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    let (width, height) = (pixmap.width(), pixmap.height());
    let mut left = width;
    let mut top = height;
    let mut right = 0u32;
    let mut bottom = 0u32;
    let mut visible = false;
    for y in 0..height {
        for x in 0..width {
            let alpha = *pixmap.data().get(((y * width + x) * 4 + 3) as usize)?;
            if alpha != 0 {
                visible = true;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
    }
    if !visible {
        return Some((0, 0, 1, 1, vec![0; 4]));
    }
    left = left.saturating_sub(1);
    top = top.saturating_sub(1);
    right = right.saturating_add(2).min(width);
    bottom = bottom.saturating_add(2).min(height);
    let crop_width = right.checked_sub(left)?;
    let crop_height = bottom.checked_sub(top)?;
    let mut pixels = vec![
        0;
        (crop_width as usize)
            .checked_mul(crop_height as usize)?
            .checked_mul(4)?
    ];
    for row in 0..crop_height {
        let src = (((top + row) * width + left) * 4) as usize;
        let dst = (row * crop_width * 4) as usize;
        let bytes = (crop_width * 4) as usize;
        pixels
            .get_mut(dst..dst + bytes)?
            .copy_from_slice(pixmap.data().get(src..src + bytes)?);
    }
    Some((left, top, crop_width, crop_height, pixels))
}

type PackedLayout = (u64, u32, u32, Vec<(u32, u32)>);

fn pack_variants(variants: &mut [Variant]) -> Option<(u32, u32)> {
    const WIDTHS: [u32; 8] = [256, 512, 768, 1024, 1280, 1536, 1792, 2048];
    let mut order: Vec<usize> = (0..variants.len()).collect();
    order.sort_by_key(|index| std::cmp::Reverse(variants[*index].height));
    let mut best: Option<PackedLayout> = None;
    for width in WIDTHS {
        let mut x = 1u32;
        let mut y = 1u32;
        let mut row_height = 0u32;
        let mut positions = vec![(0, 0); variants.len()];
        let mut fits = true;
        for index in &order {
            let variant = variants.get(*index)?;
            let cell_width = variant.width.saturating_add(2);
            let cell_height = variant.height.saturating_add(2);
            if cell_width.saturating_add(2) > width {
                fits = false;
                break;
            }
            if x.saturating_add(cell_width) > width {
                x = 1;
                y = y.saturating_add(row_height);
                row_height = 0;
            }
            positions[*index] = (x + 1, y + 1);
            x = x.saturating_add(cell_width);
            row_height = row_height.max(cell_height);
        }
        if !fits {
            continue;
        }
        let height = y.saturating_add(row_height).saturating_add(1);
        let area = u64::from(width) * u64::from(height);
        if area.saturating_mul(4) > MAX_ATLAS_BYTES as u64 {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_area, _, _, _)| area < *best_area)
        {
            best = Some((area, width, height, positions));
        }
    }
    let (_, width, height, positions) = best?;
    for (variant, (x, y)) in variants.iter_mut().zip(positions) {
        variant.atlas_x = x;
        variant.atlas_y = y;
    }
    Some((width, height))
}

fn copy_rect(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    x: u32,
    y: u32,
    atlas_width: u32,
    atlas: &mut [u8],
) -> Option<()> {
    let row_bytes = (source_width as usize).checked_mul(4)?;
    for row in 0..source_height as usize {
        let src = row.checked_mul(row_bytes)?;
        let dst = ((y as usize).checked_add(row)?)
            .checked_mul(atlas_width as usize)?
            .checked_add(x as usize)?
            .checked_mul(4)?;
        atlas
            .get_mut(dst..dst.checked_add(row_bytes)?)?
            .copy_from_slice(source.get(src..src.checked_add(row_bytes)?)?);
    }
    Some(())
}

fn render_nodes(
    nodes: &[Node],
    parts: &[Part],
    vals: &[Vals],
    inherited_dash: Option<(f32, f32, f32)>,
    inherited_width: Option<u8>,
    transform: Transform,
    pixmap: &mut Pixmap,
) -> Option<()> {
    for node in nodes {
        match node {
            Node::Shape(shape) => {
                draw_shape(
                    shape,
                    vals,
                    inherited_dash,
                    inherited_width,
                    transform,
                    pixmap,
                );
            }
            Node::Group {
                part,
                width_part,
                opacity,
                children,
            } => {
                let mut child_transform = transform;
                let mut child_opacity = opacity.clamp(0.0, 1.0);
                let mut child_dash = inherited_dash;
                let child_width = (*width_part).or(inherited_width);
                if let Some(part) = part {
                    let index = *part as usize;
                    let value = vals.get(index)?;
                    let metrics = parts.get(index)?;
                    child_transform = multiply(transform, part_transform(metrics, value));
                    if (value.op - 1.0).abs() > 1.0e-4 {
                        child_opacity *= quantise(value.op.clamp(0.0, 1.0));
                    }
                    if ((value.len - 1.0).abs() > 1.0e-4 || value.off != 0.0) && metrics.len > 0.0 {
                        let visible = quantise(value.len.clamp(0.0, 1.0) * metrics.len);
                        let hidden = quantise((1.0 - value.len.clamp(0.0, 1.0)) * metrics.len);
                        let offset = quantise((1.0 - value.off.clamp(0.0, 1.0)) * metrics.len);
                        child_dash = Some((visible, hidden, offset));
                    }
                }
                if child_opacity <= 0.0 {
                    continue;
                }
                if child_opacity < 1.0 - 1.0e-6 {
                    let mut layer = Pixmap::new(pixmap.width(), pixmap.height())?;
                    render_nodes(
                        children,
                        parts,
                        vals,
                        child_dash,
                        child_width,
                        child_transform,
                        &mut layer,
                    )?;
                    let paint = PixmapPaint {
                        opacity: child_opacity,
                        ..PixmapPaint::default()
                    };
                    pixmap.draw_pixmap(0, 0, layer.as_ref(), &paint, Transform::identity(), None);
                } else {
                    render_nodes(
                        children,
                        parts,
                        vals,
                        child_dash,
                        child_width,
                        child_transform,
                        pixmap,
                    )?;
                }
            }
        }
    }
    Some(())
}

fn draw_shape(
    shape: &Shape,
    vals: &[Vals],
    inherited_dash: Option<(f32, f32, f32)>,
    inherited_width: Option<u8>,
    transform: Transform,
    pixmap: &mut Pixmap,
) {
    let width = inherited_width
        .and_then(|part| vals.get(usize::from(part)))
        .map(|value| quantise(value.width.max(0.0)));
    draw_shape_styled(shape, inherited_dash, width, transform, pixmap);
}

fn draw_shape_styled(
    shape: &Shape,
    dash: Option<(f32, f32, f32)>,
    width: Option<f32>,
    transform: Transform,
    pixmap: &mut Pixmap,
) {
    let Some(path) = path(shape.commands) else {
        return;
    };
    if let Some(fill) = shape.fill {
        let mut paint = white(fill.opacity);
        paint.anti_alias = true;
        let rule = match fill.rule {
            FillRule::Winding => SkFillRule::Winding,
            FillRule::EvenOdd => SkFillRule::EvenOdd,
        };
        pixmap.fill_path(&path, &paint, rule, transform, None);
    }
    if let Some(stroke) = shape.stroke {
        let mut sk_stroke = SkStroke {
            width: width.unwrap_or(stroke.width),
            miter_limit: stroke.miter,
            line_cap: match stroke.cap {
                LineCap::Butt => SkLineCap::Butt,
                LineCap::Round => SkLineCap::Round,
                LineCap::Square => SkLineCap::Square,
            },
            line_join: match stroke.join {
                LineJoin::Miter => SkLineJoin::Miter,
                LineJoin::Round => SkLineJoin::Round,
                LineJoin::Bevel => SkLineJoin::Bevel,
            },
            dash: None,
        };
        if sk_stroke.width <= 0.0 {
            return;
        }
        if let Some((visible, hidden, offset)) = dash {
            // SVG permits zero intervals; tiny-skia rejects them.
            if visible <= 0.0 {
                return;
            }
            if hidden > 0.0 {
                sk_stroke.dash = StrokeDash::new(vec![visible, hidden], offset);
            }
        }
        let mut paint = white(stroke.opacity);
        paint.anti_alias = true;
        pixmap.stroke_path(&path, &paint, &sk_stroke, transform, None);
    }
}

fn white(opacity: f32) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(
        255,
        255,
        255,
        (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
    );
    paint
}

fn path(commands: &[Command]) -> Option<Path> {
    let mut builder = PathBuilder::new();
    for command in commands {
        match *command {
            Command::Move(x, y) => builder.move_to(x, y),
            Command::Line(x, y) => builder.line_to(x, y),
            Command::Quad(x1, y1, x, y) => builder.quad_to(x1, y1, x, y),
            Command::Cubic(x1, y1, x2, y2, x, y) => {
                builder.cubic_to(x1, y1, x2, y2, x, y);
            }
            Command::Close => builder.close(),
        }
    }
    builder.finish()
}

fn part_transform(part: &Part, value: &Vals) -> Transform {
    let mut transform = Matrix64::translate(decimal(part.ox + value.x), decimal(part.oy + value.y));
    transform = transform.multiply(Matrix64::scale(
        decimal(value.scale * value.scale_x),
        decimal(value.scale * value.scale_y),
    ));
    if value.rot != 0.0 {
        transform = transform.multiply(Matrix64::rotate(decimal(value.rot)));
    }
    if value.skew_x != 0.0 {
        transform = transform.multiply(Matrix64::skew_x(decimal(value.skew_x)));
    }
    if value.skew_y != 0.0 {
        transform = transform.multiply(Matrix64::skew_y(decimal(value.skew_y)));
    }
    transform
        .multiply(Matrix64::translate(decimal(-part.ox), decimal(-part.oy)))
        .tiny()
}

#[derive(Clone, Copy)]
struct Matrix64 {
    sx: f64,
    kx: f64,
    ky: f64,
    sy: f64,
    tx: f64,
    ty: f64,
}

impl Matrix64 {
    fn translate(x: f64, y: f64) -> Self {
        Self {
            sx: 1.0,
            kx: 0.0,
            ky: 0.0,
            sy: 1.0,
            tx: x,
            ty: y,
        }
    }

    fn scale(x: f64, y: f64) -> Self {
        Self {
            sx: x,
            kx: 0.0,
            ky: 0.0,
            sy: y,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn rotate(degrees: f64) -> Self {
        let radians = degrees.to_radians();
        let (sin, cos) = radians.sin_cos();
        Self {
            sx: cos,
            kx: -sin,
            ky: sin,
            sy: cos,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn skew_x(degrees: f64) -> Self {
        Self {
            sx: 1.0,
            kx: degrees.to_radians().tan(),
            ky: 0.0,
            sy: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn skew_y(degrees: f64) -> Self {
        Self {
            sx: 1.0,
            kx: 0.0,
            ky: degrees.to_radians().tan(),
            sy: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn multiply(self, other: Self) -> Self {
        Self {
            sx: self.sx * other.sx + self.kx * other.ky,
            kx: self.sx * other.kx + self.kx * other.sy,
            ky: self.ky * other.sx + self.sy * other.ky,
            sy: self.ky * other.kx + self.sy * other.sy,
            tx: self.sx * other.tx + self.kx * other.ty + self.tx,
            ty: self.ky * other.tx + self.sy * other.ty + self.ty,
        }
    }

    fn tiny(self) -> Transform {
        Transform::from_row(
            self.sx as f32,
            self.ky as f32,
            self.kx as f32,
            self.sy as f32,
            self.tx as f32,
            self.ty as f32,
        )
    }
}

fn decimal(value: f32) -> f64 {
    f64::from((value * 1000.0).round() as i32) / 1000.0
}

fn quantise(value: f32) -> f32 {
    decimal(value) as f32
}

fn multiply(a: Transform, b: Transform) -> Transform {
    Transform::from_row(
        a.sx * b.sx + a.kx * b.ky,
        a.ky * b.sx + a.sy * b.ky,
        a.sx * b.kx + a.kx * b.sy,
        a.ky * b.kx + a.sy * b.sy,
        a.sx * b.tx + a.kx * b.ty + a.tx,
        a.ky * b.tx + a.sy * b.ty + a.ty,
    )
}

fn clock(seg: &Seg, time: f32) -> f32 {
    if seg.dur <= 0.0 {
        return if time >= seg.t0 { 1.0 } else { 0.0 };
    }
    ((time - seg.t0) / seg.dur).clamp(0.0, 1.0)
}

fn at(seg: &Seg, progress: f32) -> f32 {
    let count = seg.keys.len();
    match count {
        0 => 0.0,
        1 => seg.keys[0],
        _ => {
            let gaps = count - 1;
            let (index, fraction) = if seg.times.len() == count {
                let mut index = 0usize;
                while index + 2 < count && seg.times[index + 1] <= progress {
                    index += 1;
                }
                let span = (seg.times[index + 1] - seg.times[index]).max(1.0e-6);
                (
                    index,
                    ((progress - seg.times[index]) / span).clamp(0.0, 1.0),
                )
            } else {
                let position = (progress * gaps as f32).clamp(0.0, gaps as f32);
                let index = (position as usize).min(gaps - 1);
                (index, (position - index as f32).clamp(0.0, 1.0))
            };
            let easing = seg
                .eases
                .get(index.min(seg.eases.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0);
            let eased = match EASINGS.get(easing as usize).copied() {
                Some(Some(curve_points)) => curve(curve_points, fraction),
                _ => fraction,
            };
            seg.keys[index] + (seg.keys[index + 1] - seg.keys[index]) * eased
        }
    }
}

fn curve([x1, y1, x2, y2]: [f32; 4], x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let axis = |a: f32, b: f32, time: f32| {
        let rest = 1.0 - time;
        3.0 * rest * rest * time * a + 3.0 * rest * time * time * b + time * time * time
    };
    let (mut low, mut high) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let middle = (low + high) * 0.5;
        if axis(x1, x2, middle) < x {
            low = middle;
        } else {
            high = middle;
        }
    }
    axis(y1, y2, (low + high) * 0.5)
}

#[cfg(test)]
mod parity;

#[cfg(test)]
mod timeline_tests {
    use super::*;

    #[test]
    fn curves_keep_their_endpoints_and_shape() {
        let linear = [0.0, 0.0, 1.0, 1.0];
        assert_eq!(curve(linear, 0.0), 0.0);
        assert_eq!(curve(linear, 1.0), 1.0);
        assert!((curve(linear, 0.3) - 0.3).abs() < 1.0e-3);
        assert!(curve([0.2, 0.9, 0.2, 1.0], 0.3) > 0.5);
        assert!(curve([0.34, 1.56, 0.64, 1.0], 0.5) > 1.0);
    }

    #[test]
    fn segments_hold_before_and_after_themselves() {
        let segment = Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.1,
            dur: 0.2,
            keys: &[0.0, -6.0],
            times: &[],
            eases: &[0],
        };
        assert_eq!(at(&segment, clock(&segment, 0.0)), 0.0);
        assert_eq!(at(&segment, clock(&segment, 0.2)), -3.0);
        assert_eq!(at(&segment, clock(&segment, 0.9)), -6.0);
        let snap = Seg {
            dur: 0.0,
            keys: &[1.0, 0.0],
            ..segment
        };
        assert_eq!(at(&snap, clock(&snap, 0.09)), 1.0);
        assert_eq!(at(&snap, clock(&snap, 0.1)), 0.0);
    }

    #[test]
    fn explicit_key_times_place_each_gap() {
        let segment = Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.4,
            keys: &[1.0, 0.6, 1.0],
            times: &[0.0, 0.6, 1.0],
            eases: &[0, 0],
        };
        let down = at(&segment, clock(&segment, 0.2));
        assert!((down - (1.0 - 0.4 * (0.5 / 0.6))).abs() < 1.0e-4);
        assert!((at(&segment, clock(&segment, 0.24)) - 0.6).abs() < 1.0e-5);
        assert!((at(&segment, clock(&segment, 0.4)) - 1.0).abs() < 1.0e-5);
    }
}
