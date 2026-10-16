//! The small bar under a selection: Copy, Explain and the highlighters.

use egui::{
    Align2, Context, CornerRadius, Frame, Id, Margin, Order, Rect, Sense, Stroke, Ui, pos2, vec2,
};
use mareader_appearance::Rgb;
use mareader_library::Pen;
use mareader_settings::{BarItem, HighlightSettings, PickColor};

use crate::icons::Icon;
use crate::theme::{Tokens, color};
use crate::widgets;

/// Space between the selection and the bar.
const GAP: f32 = 10.0;
/// The bar's height, for choosing a side before it's laid out.
const HEIGHT: f32 = 40.0;
/// A colour swatch's size in the picker row.
const SWATCH: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPress {
    Copy,
    Explain,
    /// A highlighter; a colour if the reader picked one.
    Highlight(Pen, Option<Rgb>),
}

/// The icon a highlighter's button shows.
pub fn pen_icon(pen: Pen) -> Icon {
    match pen {
        Pen::Full => Icon::Highlight,
        Pen::Half => Icon::HalfHighlight,
        Pen::Underline => Icon::Underline,
    }
}

/// The bar under `selected` (over it if there's no room), kept in `room`.
pub fn show(
    ctx: &Context,
    id: Id,
    selected: Rect,
    room: Rect,
    look: &HighlightSettings,
) -> Option<BarPress> {
    if look.bar.is_empty() {
        return None;
    }
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
    // Which pen's colours are out, when the reader picks each time.
    let open_id = id.with("pen");
    let mut open: Option<Pen> = ctx.data(|d| d.get_temp(open_id)).flatten();
    if look.pick == PickColor::Default {
        open = None;
    }
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
                        for item in &look.bar {
                            if let Some(press) = button(ui, *item, look, &mut open) {
                                pressed = Some(press);
                            }
                        }
                    });
                    if let Some(pen) = open {
                        ui.add_space(3.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for c in look.colors(pen) {
                                if swatch(ui, c, &t).clicked() {
                                    pressed = Some(BarPress::Highlight(pen, Some(c)));
                                }
                            }
                        });
                    }
                });
        });
    ctx.data_mut(|d| d.insert_temp(open_id, open.filter(|_| pressed.is_none())));
    pressed
}

/// One of the bar's buttons; a highlighter shows its colour underneath.
fn button(
    ui: &mut Ui,
    item: BarItem,
    look: &HighlightSettings,
    open: &mut Option<Pen>,
) -> Option<BarPress> {
    match item {
        BarItem::Copy => widgets::icon_button(ui, Icon::Copy, "Copy", false)
            .clicked()
            .then_some(BarPress::Copy),
        BarItem::Explain => widgets::icon_button(ui, Icon::Explain, "Explain", false)
            .clicked()
            .then_some(BarPress::Explain),
        BarItem::Highlight(pen) => {
            let resp = widgets::icon_button(ui, pen_icon(pen), pen.label(), *open == Some(pen));
            let strip =
                Rect::from_center_size(resp.rect.center_bottom() - vec2(0.0, 5.0), vec2(14.0, 2.5));
            let fill = color(look.color(pen));
            ui.painter().rect_filled(strip, CornerRadius::same(1), fill);
            if !resp.clicked() {
                return None;
            }
            if look.pick == PickColor::Default {
                return Some(BarPress::Highlight(pen, None));
            }
            *open = if *open == Some(pen) { None } else { Some(pen) };
            None
        }
    }
}

/// A round colour to pick, named by its hex for screen readers.
fn swatch(ui: &mut Ui, c: Rgb, t: &Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
    let ring = if resp.hovered() { t.ink } else { t.line };
    let painter = ui.painter();
    painter.circle_filled(rect.center(), SWATCH * 0.5 - 1.0, color(c));
    painter.circle_stroke(rect.center(), SWATCH * 0.5 - 1.0, Stroke::new(1.0, ring));
    let name = c.to_hex();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
