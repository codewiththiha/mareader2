//! The small bar under a selection: Copy and Explain.

use egui::{Align2, Context, Frame, Id, Margin, Order, Rect, Stroke, pos2};

use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

/// Space between the selection and the bar.
const GAP: f32 = 10.0;
/// The bar's height, for choosing a side before it's laid out.
const HEIGHT: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPress {
    Copy,
    Explain,
}

/// The bar under `selected` (over it if there's no room), kept in `room`.
pub fn show(ctx: &Context, id: Id, selected: Rect, room: Rect) -> Option<BarPress> {
    let t = Tokens::get(ctx);
    let below = selected.bottom() + GAP + HEIGHT <= room.bottom();
    let x = selected
        .center()
        .x
        .clamp(room.left() + 48.0, room.right() - 48.0);
    let (at, pivot) = match below {
        true => (pos2(x, selected.bottom() + GAP), Align2::CENTER_TOP),
        false => (pos2(x, selected.top() - GAP), Align2::CENTER_BOTTOM),
    };
    let mut pressed = None;
    egui::Area::new(id)
        .order(Order::Foreground)
        .pivot(pivot)
        .fixed_pos(at)
        .constrain_to(room)
        .show(ctx, |ui| {
            Frame::popup(ui.style())
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius_lg)
                .inner_margin(Margin::same(3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let copy = widgets::icon_button(ui, Icon::Copy, "Copy", false);
                        if copy.clicked() {
                            pressed = Some(BarPress::Copy);
                        }
                        let explain = widgets::icon_button(ui, Icon::Explain, "Explain", false);
                        if explain.clicked() {
                            pressed = Some(BarPress::Explain);
                        }
                    });
                });
        });
    pressed
}
