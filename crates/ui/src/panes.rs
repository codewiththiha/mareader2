//! The pane workspace view: surfaces, dividers, focus and close.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};
use mareader_workspace::{Bounds, PaneId, PaneTree, SplitAxis};

use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::{motion, widgets};

/// mareader's `WorkspaceSettings` look fields, egui-side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneLook {
    pub gap: f32,
    pub outline_width: f32,
    pub rounded: bool,
    pub shadow: bool,
}

impl Default for PaneLook {
    fn default() -> Self {
        Self {
            gap: 0.0,
            outline_width: 2.0,
            rounded: false,
            shadow: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneAction {
    Focus(PaneId),
    Close(PaneId),
}

fn to_bounds(r: Rect) -> Bounds {
    Bounds::new(r.left(), r.top(), r.width(), r.height())
}

fn to_rect(b: Bounds) -> Rect {
    Rect::from_min_size(pos2(b.x, b.y), vec2(b.w, b.h))
}

/// `under_bar`: a pinned title bar covers the top; controls drop below it.
pub fn show(
    ui: &mut Ui,
    tree: &mut PaneTree,
    active: PaneId,
    look: PaneLook,
    area: Rect,
    under_bar: bool,
) -> Vec<PaneAction> {
    let t = Tokens::get(ui.ctx());
    let mut out = Vec::new();
    let split = tree.len() > 1;
    let gap = if split { look.gap } else { 0.0 };
    let radius = if split && look.rounded {
        CornerRadius::same(10)
    } else {
        CornerRadius::ZERO
    };
    for (id, b) in tree.layout(to_bounds(area.shrink(gap / 2.0))) {
        let rect = to_rect(b).shrink(gap / 2.0);
        let resp = ui.interact(rect, ui.id().with(("pane", id.0)), Sense::click());
        if split && look.shadow {
            let shadow = egui::Shadow {
                offset: [0, 5],
                blur: 18,
                spread: 0,
                color: Color32::from_black_alpha(61),
            };
            ui.painter().add(shadow.as_shape(rect, radius));
        }
        ui.painter().rect_filled(rect, radius, t.paper);
        empty_pane(ui, rect, &t);
        if split && id == active {
            let stroke = Stroke::new(look.outline_width, t.accent);
            ui.painter()
                .rect_stroke(rect, radius, stroke, StrokeKind::Inside);
        }
        if resp.clicked() && id != active {
            out.push(PaneAction::Focus(id));
        }
        if split {
            let pad = if under_bar { 56.0 } else { 8.0 };
            let at = pos2(rect.right() - 8.0 - 32.0, rect.top() + pad);
            if let Some(close) = close_button(ui, id, at, resp.hovered()) {
                out.push(close);
            }
        }
    }
    for d in tree.dividers(to_bounds(area)) {
        let strip = match d.axis {
            SplitAxis::Horizontal => Rect::from_center_size(
                pos2(d.at, d.parent.y + d.parent.h / 2.0),
                vec2(8.0, d.parent.h),
            ),
            SplitAxis::Vertical => Rect::from_center_size(
                pos2(d.parent.x + d.parent.w / 2.0, d.at),
                vec2(d.parent.w, 8.0),
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
            tree.set_ratio(&d.path, ratio);
        }
    }
    out
}

/// Fades in while the pane is hovered, as mareader's `group-hover` does.
fn close_button(ui: &mut Ui, id: PaneId, at: egui::Pos2, hovered: bool) -> Option<PaneAction> {
    let key = ui.id().with(("pane-close", id.0));
    let shown = ui
        .ctx()
        .animate_bool_with_time(key, hovered, motion::secs(ui.ctx(), 0.15));
    if shown <= 0.0 {
        return None;
    }
    let builder = egui::UiBuilder::new().max_rect(Rect::from_min_size(at, vec2(32.0, 32.0)));
    let clicked = ui
        .scope_builder(builder, |ui| {
            ui.set_opacity(shown);
            widgets::compact_button(ui, Icon::Close, "Close this pane").clicked()
        })
        .inner;
    clicked.then_some(PaneAction::Close(id))
}

fn empty_pane(ui: &Ui, rect: Rect, t: &Tokens) {
    let c = rect.center();
    Icon::Book.paint(
        ui,
        Rect::from_center_size(c - vec2(0.0, 22.0), vec2(28.0, 28.0)),
        t.muted,
        1.75,
    );
    ui.painter().text(
        c + vec2(0.0, 12.0),
        Align2::CENTER_CENTER,
        "Open a book",
        theme::medium(14.0),
        t.ink,
    );
    let hint = "Drop a PDF or EPUB here, or pick one from the library";
    ui.painter().text(
        c + vec2(0.0, 32.0),
        Align2::CENTER_CENTER,
        hint,
        egui::FontId::proportional(12.0),
        t.muted,
    );
}
