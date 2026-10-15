//! The pane workspace; each pane draws in its own child `Ui`.

use egui::{
    Color32, CornerRadius, CursorIcon, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2,
};
use mareader_appearance::Rgb;
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
    /// Where dragged files would land, outlined while they hover.
    pub drop_target: Option<PaneId>,
    pub styles: Vec<(PaneId, PaneStyle)>,
}

/// How one pane is themed; the frame around it stays the window's.
#[derive(Clone, Copy, Debug)]
pub struct PaneStyle {
    pub tokens: Tokens,
    /// Blend's paper behind the pages, raw.
    pub paper: Option<Rgb>,
    /// The pane's fill: blend's paper themed, else the look's.
    pub fill: Color32,
    /// A look of its own, so its widgets take its visuals.
    pub own: bool,
}

impl PaneView {
    pub fn style(&self, id: PaneId) -> Option<&PaneStyle> {
        self.styles.iter().find(|(k, _)| *k == id).map(|(_, s)| s)
    }

    /// A pane's corner radius: rounded only while the workspace is split.
    pub fn corner(&self, split: bool, t: &Tokens) -> f32 {
        if split && self.look.rounded() {
            t.radius_lg
        } else {
            0.0
        }
    }
}

const DIVIDER_HIT: f32 = 8.0;

fn to_bounds(r: Rect) -> Bounds {
    Bounds::new(r.left(), r.top(), r.width(), r.height())
}

fn to_rect(b: Bounds) -> Rect {
    Rect::from_min_size(pos2(b.x, b.y), vec2(b.w, b.h))
}

/// Draws every pane; `content` fills one and returns false if empty.
pub fn show(
    ui: &mut Ui,
    tree: &PaneTree,
    view: &PaneView,
    out: &mut Commands,
    mut content: impl FnMut(&mut Ui, PaneId) -> bool,
) -> Vec<(PaneId, Rect)> {
    let area = ui.max_rect();
    let split = tree.len() > 1;
    let gap = if split { view.look.gap } else { 0.0 };
    let mut rects = Vec::new();
    for (id, b) in tree.layout(to_bounds(area.shrink(gap / 2.0))) {
        let rect = to_rect(b).shrink(gap / 2.0);
        let builder = UiBuilder::new().max_rect(rect).id_salt(("pane", id.0));
        ui.scope_builder(builder, |ui| {
            ui.set_clip_rect(rect);
            pane(ui, id, split, view, out, &mut content);
        });
        rects.push((id, rect));
    }
    dividers(ui, tree, area, out);
    rects
}

fn pane(
    ui: &mut Ui,
    id: PaneId,
    split: bool,
    view: &PaneView,
    out: &mut Commands,
    content: &mut impl FnMut(&mut Ui, PaneId) -> bool,
) {
    let window = Tokens::get(ui.ctx());
    let style = view.style(id).copied();
    let t = style.map_or(window, |s| s.tokens);
    if style.is_some_and(|s| s.own) {
        ui.style_mut().visuals = theme::visuals(&t);
    }
    let rect = ui.max_rect();
    let radius = CornerRadius::same(view.corner(split, &window) as u8);
    if split && view.look.shadow {
        let shadow = egui::Shadow {
            offset: [0, 5],
            blur: 18,
            spread: 0,
            color: window.shadow,
        };
        ui.painter().add(shadow.as_shape(rect, radius));
    }
    let fill = style.map_or(window.paper, |s| s.fill);
    ui.painter().rect_filled(rect, radius, fill);
    let mut inner = ui.new_child(UiBuilder::new().max_rect(rect));
    inner.set_clip_rect(rect);
    if !content(&mut inner, id) {
        empty_state(ui, rect, &t, out);
    }
    // Any press focuses the pane, even one its page widgets take.
    let pressed = ui.input(|i| i.pointer.any_pressed());
    if pressed && id != view.active && ui.rect_contains_pointer(rect) {
        out.push(Command::FocusPane(id));
    }
    if view.drop_target == Some(id) {
        let stroke = Stroke::new(2.0, window.accent);
        ui.painter()
            .rect_stroke(rect.shrink(1.0), radius, stroke, StrokeKind::Inside);
    }
    if !split {
        return;
    }
    if id == view.active && view.look.outline_width > 0.0 {
        let colour = view.look.outline_rgb().map_or(window.accent, theme::color);
        let stroke = Stroke::new(view.look.outline_width, colour);
        ui.painter()
            .rect_stroke(rect, radius, stroke, StrokeKind::Inside);
    }
    if ui.rect_contains_pointer(rect) {
        let size = window.control;
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

fn empty_state(ui: &mut Ui, rect: Rect, t: &Tokens, out: &mut Commands) {
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
    let hint = "Drop a PDF here, or open one from your files";
    painter.text(
        c + vec2(0.0, 32.0),
        egui::Align2::CENTER_CENTER,
        hint,
        egui::FontId::proportional(12.0),
        t.muted,
    );
    let button = egui::Button::image_and_text(Icon::Open.image(16.0), "Open…")
        .image_tint_follows_text_color(true)
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius);
    let at = Rect::from_center_size(c + vec2(0.0, 68.0), vec2(96.0, 32.0));
    if ui.put(at, button).clicked() {
        out.push(Command::ShowOpenDialog { beside: false });
    }
}
