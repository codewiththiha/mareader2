//! One toast's card: measured first, so a stack can place it, then drawn.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{
    Button, Color32, Context, CornerRadius, FontId, Galley, Rect, RichText, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};

use super::{Live, Tone};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

const PAD_X: f32 = 12.0;
const PAD_Y: f32 = 10.0;
/// The tone's round mark at the left.
const MARK: f32 = 26.0;
const GAP: f32 = 10.0;
const CLOSE: f32 = 24.0;
const ACTION_H: f32 = 26.0;

/// A card laid out for one width.
pub(super) struct Card {
    galley: Arc<Galley>,
    pub height: f32,
    /// The action button's width, nothing without one.
    action: f32,
}

pub(super) fn measure(ctx: &Context, live: &Live, width: f32, t: &Tokens) -> Card {
    let font = FontId::proportional(t.text);
    let action = match live.toast.undo {
        Some(_) => {
            let label = live.toast.label.to_owned();
            let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label, font.clone(), t.ink));
            galley.size().x + 20.0
        }
        None => 0.0,
    };
    let wrap = width - 2.0 * PAD_X - MARK - GAP - action - CLOSE - 8.0;
    let mut job = LayoutJob::default();
    job.append(&live.toast.message, 0.0, TextFormat::simple(font, t.ink));
    if live.times > 1 {
        let count = format!("  ×{}", live.times);
        let small = FontId::proportional(t.text_small);
        job.append(&count, 0.0, TextFormat::simple(small, t.muted));
    }
    job.wrap.max_width = wrap.max(80.0);
    let galley = ctx.fonts_mut(|f| f.layout_job(job));
    let height = (galley.size().y.max(MARK) + 2.0 * PAD_Y).ceil();
    Card {
        galley,
        height,
        action,
    }
}

/// The tone's icon, its colour and the card's edge.
fn look(tone: Tone, t: &Tokens) -> (Icon, Color32, Color32) {
    match tone {
        Tone::Info => (Icon::Done, t.accent, t.line),
        Tone::Error => (Icon::Alert, t.danger, t.danger.gamma_multiply(0.5)),
        Tone::Warning => (Icon::Alert, t.accent, t.accent.gamma_multiply(0.5)),
    }
}

/// How a card's body is drawn where it sits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Body {
    /// In front, floating over the page.
    Front,
    /// Peeking out behind: a shade toward the ink, so it reads on any page.
    Behind,
    /// In the opened list, flat on the panel.
    Listed,
}

/// The card's body alone, as a toast behind the front one shows.
pub(super) fn shell(ui: &Ui, rect: Rect, tone: Tone, body: Body, t: &Tokens) {
    let (_, _, edge) = look(tone, t);
    let radius = CornerRadius::from(t.radius_lg);
    let painter = ui.painter();
    let fill = match body {
        Body::Front => t.surface,
        Body::Behind => t.surface.lerp_to_gamma(t.ink, 0.07),
        Body::Listed => t.card,
    };
    if body == Body::Front {
        painter.add(ui.style().visuals.popup_shadow.as_shape(rect, radius));
    }
    let stroke = Stroke::new(1.0, edge);
    painter.rect(rect, radius, fill, stroke, StrokeKind::Inside);
}

/// The mark, the words, the buttons and, outside the list, the time line.
pub(super) fn content(ui: &mut Ui, rect: Rect, live: &Live, card: &Card, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let (icon, tint, _) = look(live.toast.tone, &t);
    let painter = ui.painter().clone();
    let mark = Rect::from_min_size(rect.min + vec2(PAD_X, PAD_Y), Vec2::splat(MARK));
    painter.circle_filled(mark.center(), MARK / 2.0, tint.gamma_multiply(0.16));
    icon.paint(
        ui,
        Rect::from_center_size(mark.center(), Vec2::splat(15.0)),
        tint,
    );
    let words = card.galley.size().y;
    let text = pos2(
        mark.right() + GAP,
        mark.top() + ((MARK - words) / 2.0).max(0.0),
    );
    painter.galley(text, card.galley.clone(), t.ink);
    let left = live.remaining();
    if left > 0.0 && !live.listed {
        // Inside the rounding, so the line never pokes past the corners.
        let from = rect.left() + t.radius_lg;
        let y = rect.bottom() - 2.5;
        let to = from + (rect.width() - 2.0 * t.radius_lg) * left;
        let stroke = Stroke::new(2.0, tint.gamma_multiply(0.45));
        painter.line_segment([pos2(from, y), pos2(to, y)], stroke);
    }
    if live.going() {
        return;
    }
    let row = mark.center().y;
    let close = Rect::from_center_size(
        pos2(rect.right() - 8.0 - CLOSE / 2.0, row),
        Vec2::splat(CLOSE),
    );
    let dismiss = |ui: &mut Ui| widgets::small_icon_button(ui, Icon::Close, "Dismiss");
    if ui.put(close, dismiss).clicked() {
        out.push(Command::CloseToast(live.id));
    }
    let Some(command) = &live.toast.undo else {
        return;
    };
    let at = Rect::from_min_max(
        pos2(close.left() - 4.0 - card.action, row - ACTION_H / 2.0),
        pos2(close.left() - 4.0, row + ACTION_H / 2.0),
    );
    let label = RichText::new(live.toast.label).size(t.text).strong();
    let button = Button::new(label.color(t.accent))
        .fill(t.accent_soft)
        .stroke(Stroke::NONE)
        .corner_radius(t.radius);
    if ui.put(at, button).clicked() {
        out.push((**command).clone());
        out.push(Command::CloseToast(live.id));
    }
}
