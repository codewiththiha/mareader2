//! How a glyph moves, and what earns it a move.
//!
//! The pack says how each icon moves; where the icon sits says whether
//! it should. A menu closes the moment it is answered, so its icons move as
//! they are reached - a press's motion there would never be seen. A control
//! that stays put answers a press with the move. And a mark that only names
//! something - a pen beside its label - does not move at all, since nothing
//! about pointing at it is an event.
//!
//! Which of these applies is not the icon's business: the region around it
//! `announce`s one, and every glyph built inside behaves that way. So an icon
//! needs no new flag when it joins the pack, and a new kind of menu needs no
//! change to the icons in it. Only a glyph the pack drew can run at all, and
//! nothing runs while the Animations switch is off, and a run plays its pack's
//! timeline once: it ends on the glyph as the pack draws it, which is the
//! drawing the widget shows when nothing plays, so the handback is invisible.
//!
//! A frame is drawn on a canvas with `PAD` of grid around the glyph, and painted
//! into a box as much bigger than the icon: keyframes scale a glyph up and carry
//! parts outside the square it was drawn in, and clipping that to the icon's own
//! box reads as a glyph cut off mid-gesture. A widget that lays its glyph out - a
//! button - keeps showing a blank at the icon's size while its run plays, so the
//! headroom costs no layout at all, and the frame goes over it.

use std::time::Duration;

use egui::{
    Color32, Context, Id, Image, Pos2, Rect, Response, TextureHandle, TextureOptions, Ui, Vec2,
};

use crate::motion;

use super::{GRID, HOLE, Icon, PAD, Pack, Part, Prop, STROKE, Seg, WIDTH_HOLE, pack};

/// A frame's canvas, in grid units: the glyph's square and its headroom.
const SPAN: f32 = GRID + 2.0 * PAD;
/// How much bigger a frame's box is than the glyph's own, and so how much bigger
/// the canvas it is drawn on has to be to keep the glyph at its real size.
const BOX: f32 = SPAN / GRID;

/// What the icons of one region are asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Display, not a control: it never moves.
    Still,
    /// A menu that closes when answered: move as it is reached, while it shows.
    Hover,
    /// A control that stays put: answer a press with the move.
    Click,
}

impl Act {
    /// Whether the pointer newly reaching the widget, or a press landing on it,
    /// is what this act waits for.
    fn fires(self, entering: bool, pressed: bool) -> bool {
        match self {
            Self::Still => false,
            Self::Hover => entering,
            Self::Click => pressed,
        }
    }
}

/// What a frame asks of a glyph's run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    /// Leave it as it is.
    None,
    /// Begin once the current run lands. Interrupting a move mid-flight would
    /// snap the glyph about, which is why the lab's `play` finishes first too.
    Again,
    /// Begin now.
    Start,
}

/// The ask, from what the pointer did and whether something is already moving.
fn order(act: Act, entering: bool, pressed: bool, live: bool) -> Order {
    if !act.fires(entering, pressed) {
        return Order::None;
    }
    if live { Order::Again } else { Order::Start }
}

/// Resting on a widget is not reaching it. Being gone for a few frames counts as
/// having been left, which is how a menu that closed since the last look still
/// answers the next reach.
fn entering(hovered: bool, held: bool, gone: bool) -> bool {
    hovered && (!held || gone)
}

/// The announcements of the pass being built.
#[derive(Clone, Default)]
struct Scope {
    pass: u64,
    acts: Vec<Act>,
}

/// Where the pass's announcements live. A function, not a constant: egui's
/// `Id` hashes its contents, and hashing is not a constant expression.
fn scope() -> Id {
    Id::new("mareader-icon-anim")
}

/// Holds a region's announcement for as long as that region is being built.
/// It owns its handle on the context rather than borrowing the `Ui`, because a
/// region is built with the same `Ui` it announces through: a borrow here would
/// keep the widget from being added at all.
#[must_use = "an announcement holds only while it is held"]
pub(crate) struct Announced {
    ctx: Context,
    depth: usize,
}

impl Drop for Announced {
    fn drop(&mut self) {
        self.ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Scope>(scope())
                .acts
                .truncate(self.depth);
        });
    }
}

/// Speaks for the icons built inside it, so none of them needs saying-to.
///
/// The shout nearest a glyph is the one it hears, which is what lets a bar answer
/// a reach and the control that stays put inside it answer a press instead.
pub(crate) fn announce(ui: &Ui, act: Act) -> Announced {
    shout(ui, act, false)
}

/// [`announce`] for a control speaking only for itself: the answer a widget offers
/// its own glyph when nothing around it has said otherwise. A menu that shouts
/// over it is heard instead, which is why no call site has to know where it is
/// used.
pub(crate) fn assume(ui: &Ui, act: Act) -> Announced {
    shout(ui, act, true)
}

fn shout(ui: &Ui, act: Act, only_if_silent: bool) -> Announced {
    let ctx = ui.ctx().clone();
    // Read the pass out here: egui's context lock is not reentrant, and
    // asking for it again inside `data_mut` is a deadlock waiting for a menu.
    let pass = pass(&ctx);
    let depth = ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<Scope>(scope());
        if s.pass != pass {
            s.pass = pass;
            s.acts.clear();
        }
        let depth = s.acts.len();
        if !only_if_silent || depth == 0 {
            s.acts.push(act);
        }
        depth
    });
    Announced { ctx, depth }
}

/// What the icons being built right now were told to do.
fn current(ctx: &Context) -> Act {
    let pass = pass(ctx);
    ctx.data(|d| {
        d.get_temp::<Scope>(scope())
            .filter(|s| s.pass == pass)
            .and_then(|s| s.acts.last().copied())
            .unwrap_or(Act::Still)
    })
}

/// Which pass we are in: the number a repaint advances, and the stamp every
/// announcement and run carries so a stale one is never mistaken for this pass's.
fn pass(ctx: &Context) -> u64 {
    ctx.cumulative_pass_nr()
}

/// One glyph's run, kept from frame to frame while it plays.
#[derive(Clone)]
struct Run {
    /// When the run began, in input seconds; negative while nothing plays.
    start: f64,
    /// Whether the pointer rested on its widget the last frame it was built.
    held: bool,
    /// That frame, so a widget that has gone is treated as having been left.
    seen: u64,
    /// A press that arrived mid-run asks to be played once the run lands.
    again: bool,
    /// The texture frames are drawn into, kept for the next run to reuse.
    tex: Option<TextureHandle>,
    /// Its edge, in device pixels: what frames are drawn at.
    side: u32,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            start: -1.0,
            held: false,
            seen: 0,
            again: false,
            tex: None,
            side: 0,
        }
    }
}

impl Run {
    /// Which second of the pack's timeline this frame shows, or `None` once the
    /// run is spent.
    ///
    /// A run is not played backwards and not held at the end: the frame it lands
    /// on is where the pack's last keyframe leaves the glyph, which is where the
    /// widget's own drawing has been all along, so the handback is invisible. A
    /// timeline that ended elsewhere - a part left spun up, a ghost left
    /// showing - would snap on that handback, and that is what the frame tests
    /// hold every glyph to. Playing each timeline forwards then back was the
    /// first answer to that worry and it was wrong: a run would then end on the
    /// *first* keyframe, which for a glyph that draws itself is nothing at all,
    /// so every icon would blink out as its flourish closed.
    fn through(&self, now: f64, dur: f32) -> Option<f32> {
        if self.start < 0.0 {
            return None;
        }
        let t = (now - self.start) as f32;
        (t < dur).then_some(t.max(0.0))
    }
}

/// A glyph's image: the frame it is on while a run plays, its stock drawing
/// otherwise. `tag` names the widget it sits in, so that two of one glyph in a
/// row each answer for themselves.
pub(crate) fn glyph(ui: &Ui, icon: Icon, size: f32, tag: &str) -> Image<'static> {
    match held(ui, icon, tag) {
        Some(_) => Image::from_texture(egui::load::SizedTexture::new(
            blank(ui.ctx()),
            Vec2::splat(size),
        )),
        None => icon.image(size),
    }
}

/// Drives the run for a glyph a `Button` paints from `glyph`, whose response
/// is where the press is read and whose laid-out size is what to draw at. The
/// frame goes over the widget rather than in its slot, at the box the headroom
/// asks for: a button keeps the size it was laid out at, and its glyph's ink is
/// free to reach past it.
pub(crate) fn play(ui: &Ui, resp: &Response, icon: Icon, tag: &str, size: f32) {
    let mark = Rect::from_center_size(resp.rect.center(), Vec2::splat(size));
    play_at(ui, resp, icon, tag, mark);
}

/// Plays a glyph in its laid-out box instead of the response centre.
pub(crate) fn play_at(ui: &Ui, resp: &Response, icon: Icon, tag: &str, mark: Rect) {
    if let Some(tex) = advance(ui, resp, icon, tag, mark.width()) {
        ui.painter().image(
            tex.id(),
            frame_box(mark),
            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
            ink_of(ui, resp),
        );
    }
}

fn frame_box(mark: Rect) -> Rect {
    mark.expand(mark.width() * (BOX - 1.0) * 0.5)
}

/// The colour a button's own glyph is tinted by, so a frame laid over it matches
/// the glyph it replaces: the same foreground stroke egui tints
/// `image_tint_follows_text_color` buttons with, read off the style.
fn ink_of(ui: &Ui, resp: &Response) -> Color32 {
    let w = &ui.visuals().widgets;
    if resp.is_pointer_button_down_on() {
        w.active.fg_stroke.color
    } else if resp.hovered() {
        w.hovered.fg_stroke.color
    } else {
        w.inactive.fg_stroke.color
    }
}

/// A pixel of nothing: what a button shows while its glyph is painted over it, so
/// the run changes neither the widget's size nor the id it answers to.
fn blank(ctx: &Context) -> egui::TextureId {
    let key = Id::new("mareader-icon-blank");
    if let Some(tex) = ctx.data(|d| d.get_temp::<TextureHandle>(key)) {
        return tex.id();
    }
    let image = egui::ColorImage::new([1, 1], vec![Color32::TRANSPARENT]);
    let tex = ctx.load_texture("mareader-icon-blank", image, TextureOptions::LINEAR);
    let id = tex.id();
    ctx.data_mut(|d| d.insert_temp(key, tex));
    id
}

/// A glyph a widget paints itself: driven, drawn and shown in the one frame,
/// since the widget's response is already to hand.
pub(crate) fn paint(ui: &Ui, rect: Rect, icon: Icon, color: Color32, tag: &str, resp: &Response) {
    match advance(ui, resp, icon, tag, rect.width()) {
        Some(tex) => {
            // The box the icon was asked for, grown by the headroom the frame was
            // drawn with, so the glyph keeps its size and the ink keeps its room.
            ui.painter().image(
                tex.id(),
                frame_box(rect),
                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                color,
            );
        }
        None => icon.paint(ui, rect, color),
    }
}

/// The texture holding a glyph's current frame, after this frame has been read:
/// what a run needs to start, and what starting one costs.
fn advance(ui: &Ui, resp: &Response, icon: Icon, tag: &str, size: f32) -> Option<TextureHandle> {
    let pack = pack::of(icon)?;
    let ctx = ui.ctx();
    let act = current(ctx);
    let dur = if act == Act::Still {
        0.0
    } else {
        motion::secs(ctx, pack.dur)
    };
    let id = key(ui, icon, tag);
    if dur <= 0.0 {
        // Nothing to play and nothing to keep: a switch thrown mid-run, or a
        // glyph that has been told to stand still, leaves no half-drawn mark.
        if ctx.data(|d| d.get_temp::<Run>(id)).is_some() {
            ctx.data_mut(|d| d.remove::<Run>(id));
        }
        return None;
    }
    let now = ctx.input(|i| i.time);
    let (hovered, pressed) = (resp.hovered(), resp.clicked());
    let seen = pass(ctx);
    let mut run = ctx.data(|d| d.get_temp::<Run>(id)).unwrap_or_default();
    let side = side(size * BOX, ctx.pixels_per_point());
    if run.side != side {
        run.side = side;
        run.tex = None;
    }
    let live = run.through(now, dur).is_some();
    let ask = order(
        act,
        entering(hovered, run.held, seen > run.seen + 2),
        pressed,
        live,
    );
    match ask {
        Order::Start => {
            run.start = now;
            run.again = false;
        }
        Order::Again => run.again = true,
        Order::None => {}
    }
    run.held = hovered;
    run.seen = seen;
    let t = match run.through(now, dur) {
        Some(t) => t,
        None => {
            // Landed. Either the press that came in during the run is next, or
            // the glyph rests and its texture waits for a later run.
            if run.again {
                run.again = false;
                run.start = now;
                let out = upload(ctx, &mut run, &frame_svg(pack, 0.0, PAD), pack.id);
                ctx.data_mut(|d| d.insert_temp(id, run));
                ctx.request_repaint_after(Duration::from_secs_f32(1.0 / 60.0));
                return out;
            }
            run.start = -1.0;
            ctx.data_mut(|d| d.insert_temp(id, run));
            return None;
        }
    };
    let out = upload(ctx, &mut run, &frame_svg(pack, t, PAD), pack.id);
    ctx.data_mut(|d| d.insert_temp(id, run));
    // Only as long as the run itself asks for frames; the last one lands and
    // the repaints stop with it.
    ctx.request_repaint_after(Duration::from_secs_f32(1.0 / 60.0));
    out
}

/// A frame's canvas in device pixels: the box it is painted into at the screen's
/// scale, and no more than an icon should ever cost to draw.
fn side(size: f32, ppp: f32) -> u32 {
    ((size * ppp).round() as i32).clamp(8, 192) as u32
}

/// The frame a widget shows when it did not drive the run: the texture the driver
/// last drew, while the run is still playing. A `Button` paints its image before
/// its own response exists, so the two halves of a button's frame are read here
/// and driven by [`play`].
fn held(ui: &Ui, icon: Icon, tag: &str) -> Option<TextureHandle> {
    let pack = pack::of(icon)?;
    let ctx = ui.ctx();
    if current(ctx) == Act::Still {
        return None;
    }
    let dur = motion::secs(ctx, pack.dur);
    if dur <= 0.0 {
        return None;
    }
    let now = ctx.input(|i| i.time);
    let run = ctx.data(|d| d.get_temp::<Run>(key(ui, icon, tag)))?;
    run.through(now, dur)?;
    run.tex
}

/// One glyph's, in one widget's, run.
fn key(ui: &Ui, icon: Icon, tag: &str) -> Id {
    ui.id().with(("icon-anim", icon.slot(), tag))
}

/// Draw one frame into the run's texture, making the texture on the first.
fn upload(ctx: &Context, run: &mut Run, svg: &str, name: &str) -> Option<TextureHandle> {
    let img = mareader_reflow::raster_svg(svg.as_bytes(), run.side)?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], img.as_raw());
    match &mut run.tex {
        Some(tex) => tex.set(image, TextureOptions::LINEAR),
        None => {
            run.tex = Some(ctx.load_texture(
                format!("mareader-icon-{name}"),
                image,
                TextureOptions::LINEAR,
            ));
        }
    }
    run.tex.clone()
}

/// One frame of a run as an SVG: the resting markup, with every part moved,
/// faded or half-drawn as the timeline has it at `t` seconds, drawn white for
/// egui to tint as it tints the stock glyph.
fn frame_svg(pack: &Pack, t: f32, pad: f32) -> String {
    let mut vals: Vec<Vals> = pack.parts.iter().map(Vals::rest).collect();
    for seg in pack.segs {
        let Some(v) = vals.get_mut(seg.part as usize) else {
            continue;
        };
        let x = at(seg, clock(seg, t));
        match seg.prop {
            Prop::X => v.x = x,
            Prop::Y => v.y = x,
            Prop::Scale => v.scale = x,
            Prop::ScaleX => v.scale_x = x,
            Prop::ScaleY => v.scale_y = x,
            Prop::Rotate => v.rot = x,
            Prop::SkewX => v.skew_x = x,
            Prop::SkewY => v.skew_y = x,
            Prop::Opacity => v.op = x,
            Prop::PathLen => v.len = x,
            Prop::PathOff => v.off = x,
            Prop::StrokeW => v.w = x,
        }
    }
    let mut body = String::with_capacity(pack.svg.len() + 32);
    let mut next = 0usize;
    for c in pack.svg.chars() {
        if c != HOLE && c != WIDTH_HOLE {
            body.push(c);
            continue;
        }
        let Some(hole) = pack.holes.get(next) else {
            continue;
        };
        next += 1;
        let (Some(part), Some(v)) = (
            pack.parts.get(hole.part as usize),
            vals.get(hole.part as usize),
        ) else {
            continue;
        };
        let bit = if hole.width { width(v) } else { moves(part, v) };
        body.push_str(&bit);
    }
    // The grid, hung in the middle of a canvas `pad` larger on every side: resvg
    // clips to the viewport, so the slack is what a part that leaves the box is
    // drawn on instead of cut off by.
    let left = num(-pad);
    let span = num(GRID + 2.0 * pad);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{span}" height="{span}" viewBox="{left} {left} {span} {span}" fill="none" stroke="white" stroke-width="{STROKE}" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
    )
}

/// What one part of a glyph is doing at this instant.
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
    w: f32,
}

impl Vals {
    /// The glyph as drawn: nothing moved, nothing hidden, its own stroke.
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
            w: part.w,
        }
    }

    fn moved(&self) -> bool {
        self.x != 0.0
            || self.y != 0.0
            || self.scale != 1.0
            || self.scale_x != 1.0
            || self.scale_y != 1.0
            || self.rot != 0.0
            || self.skew_x != 0.0
            || self.skew_y != 0.0
    }
}

/// Where a part stands, and where it hangs from: a turn is around the part's own
/// middle, as a browser's `transform-box: fill-box` has it.
fn moves(part: &Part, v: &Vals) -> String {
    let mut s = String::new();
    if v.moved() {
        let mut bits = vec![
            format!("translate({} {})", num(part.ox + v.x), num(part.oy + v.y)),
            format!(
                "scale({} {})",
                num(v.scale * v.scale_x),
                num(v.scale * v.scale_y)
            ),
        ];
        if v.rot != 0.0 {
            bits.push(format!("rotate({})", num(v.rot)));
        }
        if v.skew_x != 0.0 {
            bits.push(format!("skewX({})", num(v.skew_x)));
        }
        if v.skew_y != 0.0 {
            bits.push(format!("skewY({})", num(v.skew_y)));
        }
        bits.push(format!("translate({} {})", num(-part.ox), num(-part.oy)));
        s.push_str(&format!(r#" transform="{}""#, bits.join(" ")));
    }
    // A stroke drawn on is a dash whose visible share grows. The timeline
    // carries shares of the part's own length, which the pack measured.
    if ((v.len - 1.0).abs() > 1e-4 || v.off != 0.0) && part.len > 0.0 {
        let p = v.len.clamp(0.0, 1.0);
        let o = v.off.clamp(0.0, 1.0);
        s.push_str(&format!(
            r#" stroke-dasharray="{} {}" stroke-dashoffset="{}""#,
            num(p * part.len),
            num((1.0 - p) * part.len),
            num((1.0 - o) * part.len)
        ));
    }
    if (v.op - 1.0).abs() > 1e-4 {
        s.push_str(&format!(r#" opacity="{}""#, num(v.op.clamp(0.0, 1.0))));
    }
    s
}

/// The stroke's width, which the pack took off the element so that a run thinning
/// a line can also give it back: at rest the frame writes what the markup said.
fn width(v: &Vals) -> String {
    format!(r#" stroke-width="{}""#, num(v.w.max(0.0)))
}

/// How far through a segment `t` is: held back before it starts, spent after.
fn clock(seg: &Seg, t: f32) -> f32 {
    if seg.dur <= 0.0 {
        return if t >= seg.t0 { 1.0 } else { 0.0 };
    }
    ((t - seg.t0) / seg.dur).clamp(0.0, 1.0)
}

/// The value a segment holds at share `u` of itself, keyframe by keyframe.
fn at(seg: &Seg, u: f32) -> f32 {
    let n = seg.keys.len();
    match n {
        0 => 0.0,
        1 => seg.keys[0],
        _ => {
            let gaps = n - 1;
            let (i, f) = if seg.times.len() == n {
                let mut i = 0usize;
                while i + 2 < n && seg.times[i + 1] <= u {
                    i += 1;
                }
                let span = (seg.times[i + 1] - seg.times[i]).max(1e-6);
                (i, ((u - seg.times[i]) / span).clamp(0.0, 1.0))
            } else {
                let q = (u * gaps as f32).clamp(0.0, gaps as f32);
                let i = (q as usize).min(gaps - 1);
                (i, (q - i as f32).clamp(0.0, 1.0))
            };
            let e = seg
                .eases
                .get(i.min(seg.eases.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0);
            let eased = match pack::EASINGS.get(e as usize).copied() {
                Some(Some(c)) => curve(c, f),
                _ => f,
            };
            let (a, b) = (seg.keys[i], seg.keys[i + 1]);
            a + (b - a) * eased
        }
    }
}

/// The height of a cubic-bézier easing at progress `x`. Halving is no quicker
/// than Newton's method and every bit as good at icon scale.
fn curve([x1, y1, x2, y2]: [f32; 4], x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let axis = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        if axis(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    axis(y1, y2, (lo + hi) * 0.5)
}

/// A number as markup wants it: three decimals, no trailing zeros, no exponent.
fn num(v: f32) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r == 0.0 {
        "0".to_owned()
    } else if r.fract() == 0.0 {
        format!("{}", r as i32)
    } else {
        r.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The glyph's square as the tests ask for it, and the canvas a frame needs
    /// around it: the same scale, `PAD` of grid on every side.
    const SIDE: u32 = (48.0 * BOX) as u32;
    /// How much paper a frame gets when its ink is measured: far more than the
    /// paint asks for, so the measuring never becomes the thing that clips.
    const MEASURE: f32 = 20.0;

    /// A frame's canvas, with the headroom cut back off it: what the widget shows.
    fn square(img: &image::RgbaImage) -> image::RgbaImage {
        let at = (SIDE - 48) / 2;
        image::imageops::crop_imm(img, at, at, 48, 48).to_image()
    }

    fn draw(t: f32, pack: &Pack) -> Option<image::RgbaImage> {
        mareader_reflow::raster_svg(frame_svg(pack, t, PAD).as_bytes(), SIDE)
    }

    /// A frame on the measuring canvas, whose width fixes the scale: two device
    /// pixels to a grid unit, whatever the pad.
    fn measured(t: f32, pack: &Pack) -> Option<image::RgbaImage> {
        let side = (48.0 * (GRID + 2.0 * MEASURE) / GRID).round() as u32;
        mareader_reflow::raster_svg(frame_svg(pack, t, MEASURE).as_bytes(), side)
    }

    /// The box a picture's ink occupies, if it has any.
    fn ink_box(img: &image::RgbaImage) -> Option<(u32, u32, u32, u32)> {
        let (mut lo, mut top, mut hi, mut bottom) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for (x, y, p) in img.enumerate_pixels() {
            if p.0[3] > 8 {
                lo = lo.min(x);
                top = top.min(y);
                hi = hi.max(x);
                bottom = bottom.max(y);
            }
        }
        (lo <= hi).then_some((lo, top, hi, bottom))
    }

    /// How far a frame's ink reaches past the grid it was drawn on, in units.
    fn overreach(img: &image::RgbaImage) -> f32 {
        let Some((lo, top, hi, bottom)) = ink_box(img) else {
            return 0.0;
        };
        let per_unit = img.width() as f32 / (GRID + 2.0 * MEASURE);
        // The canvas starts `-MEASURE` of a grid unit before the glyph's box, so
        // a pixel is this far along the grid; past 0 or 24 is past the box.
        let unit = |v: u32| v as f32 / per_unit - MEASURE;
        (unit(hi) - GRID)
            .max(unit(bottom) - GRID)
            .max(-unit(lo))
            .max(-unit(top))
            .max(0.0)
    }

    /// Ink past the box the frame is painted into is ink cut off, which is the one
    /// thing the headroom around the grid exists to prevent: a glyph mid-gesture
    /// may reach past the square it was drawn in, but only as far as its box.
    #[test]
    fn a_frame_holds_its_ink_inside_its_headroom() {
        let mut worst = (String::from("none"), 0.0f32);
        for pack in pack::ALL {
            for step in 0..=16 {
                let t = pack.dur * step as f32 / 16.0;
                let Some(img) = measured(t, pack) else {
                    continue;
                };
                let over = overreach(&img);
                if over > worst.1 {
                    worst = (format!("{} at {t:.2} s", pack.id), over);
                }
            }
        }
        assert!(
            worst.1 <= PAD,
            "{} sends its ink {:.1} grid units past the box, and `PAD` only gives {}",
            worst.0,
            worst.1,
            PAD
        );
    }

    /// A run is not the event's own: it plays on over the frames after it, while
    /// the pointer is elsewhere, and lands when its pack says rather than when the
    /// widget stops being hovered. A run cut short by the answer it gave reads as
    /// a glyph frozen mid-gesture, which is what the title bar used to do.
    #[test]
    fn a_run_outlives_the_press_that_started_it() {
        use egui::{Event, Modifiers, PointerButton, Sense, pos2};
        let pack = pack::of(Icon::Light).unwrap_or(pack::ALL[0]);
        let ctx = egui::Context::default();
        let cell = Rect::from_min_size(pos2(4.0, 4.0), Vec2::splat(18.0));
        let mut run_of = Id::new("never");
        let mut live = Vec::new();
        for (n, time) in [0.0f64, 0.02, pack.dur as f64 * 0.5, pack.dur as f64 * 2.0]
            .into_iter()
            .enumerate()
        {
            // The press and its release in one pass, the pointer gone the next:
            // `clicked` is the frame that starts the run, and nothing after it.
            let at = cell.center();
            let events = match n {
                1 => vec![
                    Event::PointerMoved(at),
                    Event::PointerButton {
                        pos: at,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::default(),
                    },
                    Event::PointerButton {
                        pos: at,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::default(),
                    },
                ],
                _ => vec![Event::PointerMoved(pos2(390.0, 290.0))],
            };
            let raw = egui::RawInput {
                time: Some(time),
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), Vec2::splat(400.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let resp = ui.interact(cell, ui.id().with("cell"), Sense::click());
                let _act = announce(ui, Act::Click);
                if n == 1 {
                    run_of = key(ui, Icon::Light, "cell");
                }
                paint(ui, cell, Icon::Light, Color32::WHITE, "cell", &resp);
            });
            // Nothing renders here, so the frame's texture work is spent once it
            // is let go of: what this test reads is the run, not its pixels.
            out.textures_delta.clear();
            live.push(
                ctx.data(|d| d.get_temp::<Run>(run_of))
                    .is_some_and(|r| r.start >= 0.0),
            );
        }
        assert_eq!(
            live,
            [false, true, true, false],
            "the press should start a run, hold it through the frames after, and land"
        );
    }

    /// Whether a pack leaves one of its parts hidden as it lands, which is the
    /// one way a run may legitimately end on a different picture from the one it
    /// was drawn as: a part the lab added and fades out again.
    fn hides_a_part_when_it_lands(pack: &Pack) -> bool {
        pack.parts.iter().enumerate().any(|(i, _)| {
            pack.segs
                .iter()
                .rfind(|seg| seg.part as usize == i && seg.prop == Prop::Opacity)
                .and_then(|seg| seg.keys.last())
                .is_some_and(|v| *v <= 0.0)
        })
    }

    /// Pixels two pictures disagree about, in either channel.
    fn differ(a: &image::RgbaImage, b: &image::RgbaImage) -> usize {
        if a.dimensions() != b.dimensions() {
            return a.width() as usize * a.height() as usize;
        }
        a.pixels()
            .zip(b.pixels())
            .filter(|(x, y)| x.0 != y.0)
            .count()
    }

    /// The pack's glyph as its markup draws it, with nothing moved: what the
    /// widget paints when no run is playing, in our own shell. This is the
    /// picture every run has to land back on.
    fn drawn(pack: &Pack) -> Option<image::RgbaImage> {
        let rest = pack.svg.replace([HOLE, WIDTH_HOLE], "");
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="{STROKE}" stroke-linecap="round" stroke-linejoin="round">{rest}</svg>"#
        );
        mareader_reflow::raster_svg(svg.as_bytes(), 48)
    }

    /// Every glyph's run, checked against our own evaluator: the last frame it
    /// paints is the glyph as its markup draws it. A pack that left a part spun
    /// up or an echo showing would snap the icon on the frame after, and the
    /// lab's timelines are written so that they do not - a stroke ends drawn, a
    /// wash ends gone - while the turns that end off-axis (an X through ninety
    /// degrees, a gear sixty, a crown of rays forty-five) land on a drawing of
    /// the same glyph, which is why the two are compared as pictures rather than
    /// as numbers. The last frame is asked for a breath before the run is over,
    /// since the frame after it is not ours to paint.
    #[test]
    fn a_run_ends_on_the_glyph_as_drawn() {
        let mut bent: Vec<(&str, usize)> = Vec::new();
        let mut blind = Vec::new();
        for pack in pack::ALL {
            // Three of the lab's glyphs carry a part of their own that Lucide's
            // drawing has never seen - an open book's loose leaf, a bookmark's
            // fill, a reader's next page - and hide it again when the flourish
            // closes. For those the run is *meant* to end on a drawing with
            // something gone, so the comparison below would only measure that.
            // What the app paints at rest is the stock glyph, which has none of
            // them, and the end of the run has none either, so nothing snaps.
            if hides_a_part_when_it_lands(pack) {
                continue;
            }
            let (Some(rest), Some(end)) = (drawn(pack), draw(pack.dur - 1e-6, pack)) else {
                blind.push(pack.id);
                continue;
            };
            // Not `==` on the two pictures: a part that lands on a drawing of
            // itself through a turn no path traces exactly can differ at the
            // anti-aliasing - the settings gear's sixty degrees, whose pivot is
            // a hair off the glyph's true centre - and a glyph left bent differs
            // by hundreds. Two per cent of the tile tells them apart, which is
            // more than a hair of anti-aliasing and less than any feature a
            // glyph could be missing.
            let n = differ(&rest, &square(&end));
            if n > (rest.width() * rest.height() / 50) as usize {
                bent.push((pack.id, n));
            }
        }
        assert!(
            blind.is_empty(),
            "these glyphs could not be drawn at all: {}",
            blind.join(", ")
        );
        assert!(
            bent.is_empty(),
            "these runs did not land on their glyph: {}",
            bent.iter()
                .map(|(id, n)| format!("{id} ({n} pixels)"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    /// And a glyph is actually there. An empty picture equals an empty picture,
    /// so the test above would pass on a pack that drew nothing at all - a stray
    /// attribute, a stroke nobody asked for, a rasteriser that was never told to
    /// stroke anything - and so would this one, were the two not compared.
    #[test]
    fn a_drawn_glyph_has_ink() {
        let mut blank = Vec::new();
        for pack in pack::ALL {
            match drawn(pack) {
                Some(img) if img.pixels().any(|p| p[3] > 0) => {}
                _ => blank.push(pack.id),
            }
        }
        assert!(
            blank.is_empty(),
            "these glyphs drew nothing at all: {}",
            blank.join(", ")
        );
    }

    /// A frame is markup, not the template with holes left in it.
    #[test]
    fn a_frame_fills_every_hole() {
        for pack in pack::ALL {
            for t in [-1.0, pack.dur * 0.5, pack.dur] {
                let svg = frame_svg(pack, t, PAD);
                assert!(
                    !svg.contains(HOLE) && !svg.contains(WIDTH_HOLE),
                    "{}: a hole at {t} went unfilled",
                    pack.id
                );
            }
        }
    }

    /// Every hole has a mark to sit in, and every part a hole of its own; a part
    /// with no mark could be moved but never seen moving.
    #[test]
    fn holes_match_the_markup() {
        for pack in pack::ALL {
            let marks = pack.svg.matches(HOLE).count() + pack.svg.matches(WIDTH_HOLE).count();
            assert_eq!(
                marks,
                pack.holes.len(),
                "{}: holes and marks disagree",
                pack.id
            );
            assert_eq!(
                pack.svg.matches(HOLE).count(),
                pack.parts.len(),
                "{}: a part with nowhere to go",
                pack.id
            );
            for hole in pack.holes {
                assert!(
                    (hole.part as usize) < pack.parts.len(),
                    "{}: a hole names a part that is not there",
                    pack.id
                );
            }
            for seg in pack.segs {
                assert!(
                    (seg.part as usize) < pack.parts.len(),
                    "{}: a segment names a part that is not there",
                    pack.id
                );
                assert!(
                    !seg.keys.is_empty(),
                    "{}: a segment with nothing to do",
                    pack.id
                );
                assert!(
                    pack.dur >= seg.t0 + seg.dur - 1e-4,
                    "{}: a segment outlives the run",
                    pack.id
                );
            }
        }
    }

    #[test]
    fn a_curve_is_its_endpoints_and_its_own_shape() {
        let linear = [0.0, 0.0, 1.0, 1.0];
        assert_eq!(curve(linear, 0.0), 0.0);
        assert_eq!(curve(linear, 1.0), 1.0);
        assert!((curve(linear, 0.3) - 0.3).abs() < 1e-3, "a line is itself");
        // Snap: most of the way there well before the midpoint of the time.
        assert!(curve([0.2, 0.9, 0.2, 1.0], 0.3) > 0.5, "snap attacks");
        let settle = [0.34, 1.56, 0.64, 1.0];
        assert!(curve(settle, 0.5) > 1.0, "settle overshoots mid-flight");
        assert!(
            (curve(settle, 1.0) - 1.0).abs() < 1e-6,
            "and settles by the end"
        );
    }

    #[test]
    fn segments_hold_before_and_after_themselves() {
        let seg = Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.1,
            dur: 0.2,
            keys: &[0.0, -6.0],
            times: &[],
            eases: &[0],
        };
        assert_eq!(at(&seg, clock(&seg, 0.0)), 0.0, "before it starts");
        assert_eq!(at(&seg, clock(&seg, 0.2)), -3.0, "halfway, linearly");
        assert_eq!(at(&seg, clock(&seg, 0.9)), -6.0, "spent, and held");
        let snap = Seg {
            dur: 0.0,
            keys: &[1.0, 0.0],
            ..seg
        };
        assert_eq!(
            at(&snap, clock(&snap, 0.09)),
            1.0,
            "a snap has not happened yet"
        );
        assert_eq!(at(&snap, clock(&snap, 0.1)), 0.0, "then it has");
    }

    #[test]
    fn keyframes_are_spread_by_times_when_it_gives_them() {
        let seg = Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.4,
            keys: &[1.0, 0.6, 1.0],
            times: &[0.0, 0.6, 1.0],
            eases: &[0, 0],
        };
        // Halfway through the segment is five sixths of the way through the
        // first gap, and that gap runs from 1.0 down to its 0.6.
        let down = at(&seg, clock(&seg, 0.2));
        assert!(
            (down - (1.0 - 0.4 * (0.5 / 0.6))).abs() < 1e-4,
            "two thirds of the way down the first gap, not {down}"
        );
        let deep = at(&seg, clock(&seg, 0.24));
        assert!((deep - 0.6).abs() < 1e-5, "the deep key sits at 60%");
        let landed = at(&seg, clock(&seg, 0.4));
        assert!(
            (landed - 1.0).abs() < 1e-5,
            "and the landing is the landing"
        );
    }

    #[test]
    fn each_act_waits_for_its_own_event() {
        assert!(!Act::Still.fires(true, true), "a label does not twitch");
        assert!(Act::Hover.fires(true, false));
        assert!(
            !Act::Hover.fires(false, true),
            "a menu closes before a press shows"
        );
        assert!(Act::Click.fires(false, true));
        assert!(!Act::Click.fires(true, false), "a glyph is not a menu");
    }

    #[test]
    fn the_nearest_announcement_overrides_its_menu() {
        let ctx = Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            assert_eq!(current(ui.ctx()), Act::Still);
            let menu = announce(ui, Act::Hover);
            assert_eq!(current(ui.ctx()), Act::Hover);
            {
                let control = announce(ui, Act::Click);
                assert_eq!(current(ui.ctx()), Act::Click);
                drop(control);
            }
            assert_eq!(current(ui.ctx()), Act::Hover);
            drop(menu);
            assert_eq!(current(ui.ctx()), Act::Still);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn a_move_starts_on_a_reaching_not_a_resting() {
        assert!(entering(true, false, false), "reached");
        assert!(!entering(true, true, false), "already resting on it");
        assert!(entering(true, true, true), "back after its menu closed");
        assert!(!entering(false, false, false));
        assert_eq!(order(Act::Hover, true, false, false), Order::Start);
        assert_eq!(
            order(Act::Click, false, true, true),
            Order::Again,
            "a press mid-run is played once the run lands"
        );
        assert_eq!(order(Act::Hover, false, false, true), Order::None);
    }

    /// Numbers the markup can carry: no exponent, no minus zero, no long tail.
    #[test]
    fn numbers_write_as_markup_wants() {
        assert_eq!(num(0.0), "0");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(6.0), "6");
        assert_eq!(num(-1.25), "-1.25");
        assert_eq!(num(0.0004), "0");
        assert_eq!(num(1e-7), "0");
        assert_eq!(num(1234.5678), "1234.568");
    }

    /// A frame is a whole SVG, and one that usvg will read: it is drawn through
    /// the same rasteriser a document's pictures use.
    #[test]
    fn a_frame_is_a_whole_svg() {
        for pack in pack::ALL {
            let svg = frame_svg(pack, pack.dur * 0.5, PAD);
            assert!(
                svg.starts_with("<svg ") && svg.ends_with("</svg>"),
                "{}: not a whole svg",
                pack.id
            );
            assert!(
                draw(pack.dur * 0.5, pack).is_some(),
                "{}: its middle frame will not draw",
                pack.id
            );
        }
    }
}
