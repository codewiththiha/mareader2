//! A result drawn over its page, and the bubbles that pick among them.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2,
};

use mareader_appearance::PageTone;

use crate::gpu::{Anchor, Quad};
use crate::pages::PageTexture;
use crate::pdf_view::{blank, keep_pictures};
use crate::theme::Tokens;

use super::cards::Card;

/// A bubble's side, in points.
const BUBBLE: f32 = 22.0;
const BUBBLE_GAP: f32 = 6.0;
/// The bubbles' inset from the page's corner.
const BUBBLE_INSET: f32 = 8.0;
/// How far a fade reaches over text that runs past the room.
const FADE: f32 = 28.0;

/// How a bubble's result stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Ready,
    /// Still being written.
    Busy,
    Failed,
    /// Made for another layout of the page.
    Stale,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bubble {
    pub label: String,
    /// What it holds, for the tooltip and screen readers.
    pub tip: String,
    pub active: bool,
    pub state: State,
}

/// A result shown on its page.
#[derive(Clone)]
pub struct Shown {
    /// None until first set.
    pub card: Option<Card>,
    /// The column's corner and size, in page units.
    pub at: Vec2,
    pub room: Vec2,
    /// The fade in, 0 to 1.
    pub alpha: f32,
    /// The page's width in page units, to scale to its rect.
    pub width: f32,
}

/// What a page shows of the AI: bubbles, maybe a result.
#[derive(Clone, Default)]
pub struct Overlay {
    pub page: usize,
    pub bubbles: Vec<Bubble>,
    pub card: Option<Shown>,
}

/// Where a card's text sits on screen, for the wheel and the fades.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardHit {
    pub page: usize,
    pub rect: Rect,
    /// How far it scrolls, and how far it has, in page units.
    pub max: f32,
    pub scroll: f32,
    /// Points per page unit.
    pub scale: f32,
}

/// A press on a bubble.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Press {
    Pick(usize, Pos2),
    Menu(usize, Pos2),
}

/// Queues `shown` over its page in `quads`; where its text sits.
pub fn card(
    ui: &Ui,
    ((rect, clip), anchor): ((Rect, Rect), Anchor),
    (page, shown, scroll): (usize, &Shown, f32),
    (t, pictures, emojis): (&Tokens, PageTone, PageTone),
    quads: &mut Vec<Quad>,
) -> Option<CardHit> {
    let a = (shown.alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    let tint = Color32::from_white_alpha(a);
    let k = rect.width() / shown.width.max(1.0);
    // Its own paper first, so nothing of the page shows through.
    let gpu = !matches!(
        shown.card.as_ref().map(|c| &c.texture),
        Some(PageTexture::Egui(_))
    );
    if !gpu {
        let paper = blank(t).gamma_multiply(shown.alpha.clamp(0.0, 1.0));
        ui.painter()
            .with_clip_rect(clip)
            .rect_filled(rect, 0.0, paper);
    }
    let mut ground = Quad::page((rect, clip), None, t.page, anchor);
    ground.tint = tint;
    quads.push(ground);
    let room = Rect::from_min_size(rect.min + shown.at * k, shown.room * k).intersect(clip);
    let card = shown.card.as_ref()?;
    let max = (card.size.y - room.height() / k).max(0.0);
    let scroll = scroll.clamp(0.0, max);
    let corner = rect.min + shown.at * k - vec2(0.0, scroll * k);
    let drawn = Rect::from_min_size(corner, card.size * k);
    match &card.texture {
        PageTexture::Gpu(texture) => {
            let first = quads.len();
            let mut q = Quad::page((drawn, room), Some(texture.clone()), t.page, anchor);
            q.tint = tint;
            quads.push(q);
            let place = |r: &Rect| Rect::from_min_size(corner + r.min.to_vec2() * k, r.size() * k);
            keep_pictures(quads, first, card.pictures.iter().map(place), pictures);
            keep_pictures(quads, first, card.emojis.iter().map(place), emojis);
        }
        PageTexture::Egui(handle) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            let painter = ui.painter().with_clip_rect(room);
            painter.image(handle.id(), drawn, uv, tint);
        }
    }
    Some(CardHit {
        page,
        rect: room,
        max,
        scroll,
        scale: k,
    })
}

/// Fades over text that runs past a card's top or bottom edge.
pub fn fades(ui: &Ui, hits: &[CardHit], t: &Tokens) {
    let paper = blank(t);
    for hit in hits.iter().filter(|h| h.max > 0.0) {
        let r = hit.rect;
        let reach = FADE.min(r.height() / 3.0);
        if hit.scroll > 0.5 {
            let top = Rect::from_min_size(r.min, vec2(r.width(), reach));
            fade(ui, top, paper, true);
        }
        if hit.scroll < hit.max - 0.5 {
            let bottom = Rect::from_min_max(pos2(r.left(), r.bottom() - reach), r.max);
            fade(ui, bottom, paper, false);
        }
    }
}

/// Paper fading out from one edge of `r`: the top if `down`.
fn fade(ui: &Ui, r: Rect, paper: Color32, down: bool) {
    let clear = paper.gamma_multiply(0.0);
    let (top, bottom) = if down { (paper, clear) } else { (clear, paper) };
    let mut mesh = Mesh::default();
    let v = |pos: Pos2, color| Vertex {
        pos,
        uv: WHITE_UV,
        color,
    };
    mesh.vertices.extend([
        v(r.left_top(), top),
        v(r.right_top(), top),
        v(r.right_bottom(), bottom),
        v(r.left_bottom(), bottom),
    ]);
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    ui.painter().add(Shape::mesh(mesh));
}

/// The page's bubbles down its end side; a press on one.
pub fn bubbles(
    ui: &Ui,
    (rect, clip): (Rect, Rect),
    page: usize,
    all: &[Bubble],
    t: &Tokens,
) -> Option<Press> {
    let seen = rect.intersect(clip);
    let x = seen.right() - BUBBLE_INSET - BUBBLE / 2.0;
    let top = seen.top() + BUBBLE_INSET + BUBBLE / 2.0;
    let mut press = None;
    for (i, b) in all.iter().enumerate() {
        let centre = pos2(x, top + i as f32 * (BUBBLE + BUBBLE_GAP));
        let r = Rect::from_center_size(centre, Vec2::splat(BUBBLE));
        if !seen.contains_rect(r) {
            break;
        }
        let resp = ui.interact(r, ui.id().with(("page-ai-bubble", page, i)), Sense::click());
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, b.active, &b.tip)
        });
        paint_bubble(ui, r, b, resp.hovered() || resp.has_focus(), t);
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let at = resp.interact_pointer_pos().unwrap_or(r.left_bottom());
        if resp.clicked() {
            press = Some(Press::Pick(i, at));
        } else if resp.secondary_clicked() {
            press = Some(Press::Menu(i, at));
        }
        resp.on_hover_text(&b.tip);
    }
    press
}

fn paint_bubble(ui: &Ui, r: Rect, b: &Bubble, lit: bool, t: &Tokens) {
    let p = ui.painter();
    let c = r.center();
    let radius = r.width() / 2.0;
    let (fill, ink) = match (b.active, lit) {
        (true, _) => (t.accent, Color32::WHITE),
        (false, true) => (t.hover.blend(t.surface), t.ink),
        (false, false) => (t.surface, t.ink),
    };
    p.circle_filled(c + vec2(0.0, 1.0), radius, t.shadow);
    p.circle_filled(c, radius, fill);
    let edge = match b.state {
        State::Failed => Stroke::new(1.5, t.danger),
        _ => Stroke::new(1.0, t.line),
    };
    p.circle_stroke(c, radius, edge);
    let label = if b.state == State::Failed {
        "!"
    } else {
        &b.label
    };
    let font = FontId::proportional(12.0);
    p.text(c, Align2::CENTER_CENTER, label, font, ink);
    match b.state {
        State::Busy => {
            // A turning arc while it is written.
            let turn = ui.input(|i| i.time) as f32 * 5.0;
            let points: Vec<Pos2> = (0..=16)
                .map(|n| {
                    let a = turn + n as f32 / 16.0 * std::f32::consts::PI * 1.2;
                    c + vec2(a.cos(), a.sin()) * (radius + 2.5)
                })
                .collect();
            p.add(Shape::line(points, Stroke::new(2.0, t.accent)));
            ui.ctx().request_repaint();
        }
        State::Stale => {
            let dot = r.right_top() + vec2(-3.0, 3.0);
            p.circle_filled(dot, 3.5, t.surface);
            p.circle_filled(dot, 2.5, t.muted);
        }
        State::Ready | State::Failed => {}
    }
}
