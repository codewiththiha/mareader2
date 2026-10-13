//! The pane workspace; each pane draws in its own child `Ui`.

use egui::{CornerRadius, CursorIcon, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use mareader_settings::PaneLook;
use mareader_workspace::{Bounds, PaneId, PaneTree, SplitAxis};

use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets;

pub struct PaneView {
    pub active: PaneId,
    pub look: PaneLook,
    /// Bottom of the bar's hover band; pane controls stay below it.
    pub chrome_bottom: f32,
}

const DIVIDER_HIT: f32 = 8.0;

fn to_bounds(r: Rect) -> Bounds {
    Bounds::new(r.left(), r.top(), r.width(), r.height())
}

fn to_rect(b: Bounds) -> Rect {
    Rect::from_min_size(pos2(b.x, b.y), vec2(b.w, b.h))
}

pub fn show(ui: &mut Ui, tree: &PaneTree, view: &PaneView, out: &mut Commands) {
    let area = ui.max_rect();
    let split = tree.len() > 1;
    let gap = if split { view.look.gap } else { 0.0 };
    for (id, b) in tree.layout(to_bounds(area.shrink(gap / 2.0))) {
        let rect = to_rect(b).shrink(gap / 2.0);
        let builder = UiBuilder::new().max_rect(rect).id_salt(("pane", id.0));
        ui.scope_builder(builder, |ui| {
            ui.set_clip_rect(rect);
            pane(ui, id, split, view, out);
        });
    }
    dividers(ui, tree, area, out);
}

fn pane(ui: &mut Ui, id: PaneId, split: bool, view: &PaneView, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let rect = ui.max_rect();
    let radius = if split && view.look.rounded() {
        CornerRadius::same(t.radius_lg as u8)
    } else {
        CornerRadius::ZERO
    };
    if split && view.look.shadow {
        let shadow = egui::Shadow {
            offset: [0, 5],
            blur: 18,
            spread: 0,
            color: t.shadow,
        };
        ui.painter().add(shadow.as_shape(rect, radius));
    }
    ui.painter().rect_filled(rect, radius, t.paper);
    let focus = ui.interact(rect, ui.id().with("focus"), Sense::click());
    if focus.clicked() && id != view.active {
        out.push(Command::FocusPane(id));
    }
    empty_state(ui, rect, &t);
    if !split {
        return;
    }
    if id == view.active && view.look.outline_width > 0.0 {
        let colour = view.look.outline_rgb().map_or(t.accent, theme::color);
        let stroke = Stroke::new(view.look.outline_width, colour);
        ui.painter()
            .rect_stroke(rect, radius, stroke, StrokeKind::Inside);
    }
    if ui.rect_contains_pointer(rect) {
        let size = t.control;
        let top = rect.top().max(view.chrome_bottom) + 8.0;
        let at = pos2(rect.right() - 8.0 - size, top);
        let mut child =
            ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(at, vec2(size, size))));
        if widgets::icon_button(&mut child, Icon::Close, "Close pane", false).clicked() {
            out.push(Command::ClosePane(id));
        }
    }
}

fn dividers(ui: &mut Ui, tree: &PaneTree, area: Rect, out: &mut Commands) {
    for d in tree.dividers(to_bounds(area)) {
        let strip = match d.axis {
            SplitAxis::Horizontal => Rect::from_center_size(
                pos2(d.at, d.parent.y + d.parent.h / 2.0),
                vec2(DIVIDER_HIT, d.parent.h),
            ),
            SplitAxis::Vertical => Rect::from_center_size(
                pos2(d.parent.x + d.parent.w / 2.0, d.at),
                vec2(d.parent.w, DIVIDER_HIT),
            ),
        };
        let resp = ui.interact(
            strip,
            ui.id().with(("divider", d.path.clone())),
            Sense::drag(),
        );
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(match d.axis {
                SplitAxis::Horizontal => CursorIcon::ResizeColumn,
                SplitAxis::Vertical => CursorIcon::ResizeRow,
            });
        }
        if let (true, Some(p)) = (resp.dragged(), resp.interact_pointer_pos()) {
            let ratio = match d.axis {
                SplitAxis::Horizontal => (p.x - d.parent.x) / d.parent.w.max(1.0),
                SplitAxis::Vertical => (p.y - d.parent.y) / d.parent.h.max(1.0),
            };
            out.push(Command::SetRatio(d.path, ratio));
        }
    }
}

fn empty_state(ui: &mut Ui, rect: Rect, t: &Tokens) {
    let c = rect.center();
    let icon = Rect::from_center_size(c - vec2(0.0, 22.0), vec2(28.0, 28.0));
    Icon::Book.paint(ui, icon, t.muted);
    let painter = ui.painter();
    painter.text(
        c + vec2(0.0, 12.0),
        egui::Align2::CENTER_CENTER,
        "Open a book",
        theme::medium(14.0),
        t.ink,
    );
    let hint = "Drop a PDF or EPUB here, or pick one from the library";
    painter.text(
        c + vec2(0.0, 32.0),
        egui::Align2::CENTER_CENTER,
        hint,
        egui::FontId::proportional(12.0),
        t.muted,
    );
}
