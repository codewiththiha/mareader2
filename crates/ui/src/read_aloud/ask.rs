//! The question reading asks at a part it may pass over, as a table.

use std::sync::Arc;

use egui::{
    Align2, Context, CornerRadius, CursorIcon, Galley, Id, Order, Rect, Sense, Shadow, Stroke, Ui,
    WidgetInfo, WidgetType, pos2, vec2,
};

use crate::icons::Icon;
use crate::motion;
use crate::theme::{self, Tokens};

/// The pill's height, and its gap above the player.
const HEIGHT: f32 = 44.0;
const GAP: f32 = 10.0;
const PAD: f32 = 7.0;
/// The icon's disc, and the answers' height.
const DISC: f32 = 30.0;
const BUTTON_H: f32 = 30.0;
/// How far it rises as it comes in.
const RISE: f32 = 8.0;

/// Asks about `part` over `player`, `shown` of the way in; the answer.
pub(super) fn show(
    ctx: &Context,
    (player, room): (Rect, Rect),
    part: &str,
    shown: f32,
) -> Option<bool> {
    let foot = pos2(player.center().x, player.top() - GAP + (1.0 - shown) * RISE);
    let area = egui::Area::new(Id::new("read-aloud-ask"))
        .fade_in(false)
        .order(Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(foot)
        .constrain(false)
        .interactable(shown > 0.9);
    let asked = area.show(ctx, |ui| {
        ui.set_opacity(shown);
        ui.set_clip_rect(room);
        pill(ui, part)
    });
    crate::toast::keep_clear(ctx, asked.response.rect);
    asked.inner
}

/// The pill: the part's icon and name, then Read and Skip.
fn pill(ui: &mut Ui, part: &str) -> Option<bool> {
    let t = Tokens::get(ui.ctx());
    let painter = ui.painter().clone();
    let title = painter.layout_no_wrap(format!("{part} ahead"), theme::medium(t.text), t.ink);
    let small = theme::medium(t.text_small);
    let read = painter.layout_no_wrap("Read".to_owned(), small.clone(), t.ink);
    let skip = painter.layout_no_wrap("Skip".to_owned(), small, theme::ink_on(t.accent));
    let wide = |g: &Galley| g.size().x + 26.0;
    let width = PAD + DISC + 10.0 + title.size().x + 18.0 + wide(&read) + 6.0 + wide(&skip) + PAD;
    let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::hover());
    let round = CornerRadius::same((HEIGHT / 2.0) as u8);
    let base = ui.ctx().global_style().visuals.window_shadow;
    painter.add(base.as_shape(rect, round));
    painter.rect_filled(rect, round, t.surface);
    let edge = Stroke::new(1.0, t.line);
    painter.rect_stroke(rect, round, edge, egui::StrokeKind::Inside);
    let y = rect.center().y;
    let disc = pos2(rect.left() + PAD + DISC / 2.0, y);
    painter.circle_filled(disc, DISC / 2.0, t.accent_soft);
    let icon = Rect::from_center_size(disc, egui::Vec2::splat(16.0));
    Icon::Table.image(16.0).tint(t.accent).paint_at(ui, icon);
    let at = pos2(disc.x + DISC / 2.0 + 10.0, y - title.size().y / 2.0);
    painter.galley(at, title, t.ink);
    let lower = part.to_lowercase();
    let mut right = rect.right() - PAD;
    let mut slot = |g: &Galley| {
        let r = Rect::from_min_max(
            pos2(right - wide(g), y - BUTTON_H / 2.0),
            pos2(right, y + BUTTON_H / 2.0),
        );
        right = r.left() - 6.0;
        r
    };
    let (skip_at, read_at) = (slot(&skip), slot(&read));
    let mut answer = None;
    if button(ui, skip_at, skip, &format!("Skip the {lower}"), true) {
        answer = Some(false);
    }
    if button(ui, read_at, read, &format!("Read the {lower}"), false) {
        answer = Some(true);
    }
    answer
}

/// One answer: filled in the accent if `primary`, else quiet.
fn button(ui: &mut Ui, rect: Rect, text: Arc<Galley>, label: &str, primary: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let r = ui.interact(rect, ui.id().with(("ask", label)), Sense::click());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let ctx = ui.ctx().clone();
    let quick = motion::secs(&ctx, 0.1);
    let hot = ctx.animate_bool_with_time(r.id.with("hot"), r.hovered(), quick);
    let down = ctx.animate_bool_with_time(r.id.with("down"), r.is_pointer_button_down_on(), quick);
    if r.hovered() {
        ctx.set_cursor_icon(CursorIcon::PointingHand);
    }
    let shape = rect.shrink(down * 1.5);
    let round = CornerRadius::same((shape.height() / 2.0) as u8);
    let painter = ui.painter();
    if primary {
        let fill = t.accent.gamma_multiply(1.0 - 0.12 * hot);
        let lift = Shadow {
            offset: [0, 1],
            blur: (4.0 + 4.0 * hot) as u8,
            spread: 0,
            color: t.accent.gamma_multiply(0.35),
        };
        painter.add(lift.as_shape(shape, round));
        painter.rect_filled(shape, round, fill);
    } else {
        painter.rect_filled(shape, round, t.hover.gamma_multiply(0.6 + 0.4 * hot));
    }
    let at = shape.center() - text.size() / 2.0;
    let ink = if primary {
        theme::ink_on(t.accent)
    } else {
        t.ink
    };
    painter.galley(at, text, ink);
    r.clicked()
}
