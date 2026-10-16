//! The few controls egui has no stock widget for.

use egui::{
    Button, Color32, CornerRadius, Frame, Margin, Rect, Response, RichText, Sense, Shadow, Stroke,
    Ui, Vec2, pos2, vec2,
};

use crate::command::WindowCommand;
use crate::icons::Icon;
use crate::theme::Tokens;

/// A popup's own frame: clicks stay in it, but Tab skips it for its rows.
pub const POPUP_SENSE: Sense = Sense::CLICK;

/// Closes a menu when Enter or Space presses one of its rows.
pub fn close_on_key(ui: &Ui) {
    let ctx = ui.ctx();
    let pressed = ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space));
    let focused = ctx.memory(|m| m.focused());
    let layer =
        focused.and_then(|id| ctx.viewport(|v| v.this_pass.widgets.get(id).map(|w| w.layer_id)));
    if pressed && layer == Some(ui.layer_id()) {
        ui.close();
    }
}

/// A right-click menu that Shift+F10 also opens, under the focused widget.
pub fn context_menu(resp: &Response, add: impl FnOnce(&mut Ui)) {
    let ctx = &resp.ctx;
    let id = egui::Popup::default_response_id(resp);
    let keyed = id.with("keyed");
    let shift_f10 = egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::F10);
    if resp.has_focus() && ctx.input_mut(|i| i.consume_shortcut(&shift_f10)) {
        egui::Popup::open_id(ctx, id);
        ctx.data_mut(|d| d.insert_temp(keyed, true));
        // Its rows follow the widget in Tab order: the first takes focus.
        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::Next));
    }
    let open = egui::Popup::is_id_open(ctx, id);
    let by_key = open && ctx.data(|d| d.get_temp::<bool>(keyed)).unwrap_or(false);
    if (resp.secondary_clicked() || !open) && by_key {
        ctx.data_mut(|d| d.remove_temp::<bool>(keyed));
    }
    let popup = egui::Popup::context_menu(resp).sense(POPUP_SENSE);
    // No pointer to open at: it hangs from the widget instead.
    let popup = match by_key && !resp.secondary_clicked() {
        true => popup.anchor(resp.rect),
        false => popup,
    };
    popup.show(|ui| {
        add(ui);
        close_on_key(ui);
    });
}

/// A borderless icon button; `selected` marks a toggle that is on.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    icon_button_keyed(ui, icon, label, selected, None)
}

/// [`icon_button`] whose tooltip also names its shortcut.
pub fn icon_button_keyed(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    selected: bool,
    keys: Option<&str>,
) -> Response {
    let t = Tokens::get(ui.ctx());
    let button = Button::image(icon.image(t.icon))
        .image_tint_follows_text_color(true)
        .frame_when_inactive(false)
        .selected(selected)
        .min_size(Vec2::splat(t.control));
    let resp = ui.add(button);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    tip(&resp, label, keys);
    resp
}

/// A 24 pt icon button for rows and toasts.
pub fn small_icon_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let button = Button::image(icon.image(14.0))
        .image_tint_follows_text_color(true)
        .frame_when_inactive(false)
        .min_size(Vec2::splat(24.0));
    let resp = ui.add(button);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    tip(&resp, label, None);
    resp
}

/// Names a control with no visible label, for screen readers.
pub fn name(resp: &Response, label: &str) {
    resp.ctx
        .accesskit_node_builder(resp.id, |n| n.set_label(label));
}

/// Runs `add`, naming its nameless controls by `label` for screen readers.
pub fn labelled<R>(ui: &mut Ui, label: egui::Id, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layer = ui.layer_id();
    let count = |ui: &Ui| {
        ui.ctx()
            .viewport(|v| v.this_pass.widgets.get_layer(layer).count())
    };
    let before = count(ui);
    let out = add(ui);
    let added: Vec<egui::Id> = ui.ctx().viewport(|v| {
        let all = v.this_pass.widgets.get_layer(layer).skip(before);
        all.filter(|w| w.sense.interactive())
            .map(|w| w.id)
            .collect()
    });
    for id in added {
        ui.ctx().accesskit_node_builder(id, |n| {
            if n.label().is_none() && n.labelled_by().is_empty() {
                n.push_labelled_by(label.accesskit_id());
            }
        });
    }
    out
}

/// One menu row: icon, label, and its shortcut on the right.
pub fn menu_row(ui: &mut Ui, icon: Icon, label: &str, keys: Option<&str>, on: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let keys = RichText::new(keys.unwrap_or_default())
        .size(t.text_small)
        .color(t.muted);
    let button = Button::image_and_text(icon.image(16.0), label)
        .right_text(keys)
        .image_tint_follows_text_color(true)
        .frame_when_inactive(false)
        .selected(on)
        .min_size(vec2(ui.available_width(), 32.0));
    let resp = ui.add(button);
    // The label alone; the shortcut is not part of its name.
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
    resp
}

/// The app's tooltip: a small chip, the shortcut beside the words.
pub fn tip(resp: &Response, label: &str, keys: Option<&str>) {
    let t = Tokens::get(&resp.ctx);
    let mut tooltip = egui::Tooltip::for_enabled(resp);
    let frame = Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(6)
        .inner_margin(Margin::symmetric(8, 4))
        .shadow(Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: t.shadow.gamma_multiply(0.6),
        });
    tooltip.popup = tooltip.popup.frame(frame).gap(6.0);
    tooltip.show(|ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(RichText::new(label).size(t.text_small).color(t.ink));
            if let Some(keys) = keys {
                ui.label(RichText::new(keys).size(t.text_small).color(t.muted));
            }
        });
    });
}

/// One window-caption button family per desktop convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionStyle {
    /// Full-height squares, flush right; close turns red.
    Windows,
    /// Small translucent circles (GNOME/libadwaita).
    Gnome,
}

const CAPTIONS: [WindowCommand; 3] = [
    WindowCommand::Minimize,
    WindowCommand::ToggleMaximize,
    WindowCommand::Close,
];

/// Lays the three captions out left to right and returns the one clicked.
pub fn captions(ui: &mut Ui, style: CaptionStyle, maximized: bool) -> Option<WindowCommand> {
    let t = Tokens::get(ui.ctx());
    let mut clicked = None;
    for which in CAPTIONS {
        let size = match style {
            CaptionStyle::Windows => vec2(46.0, ui.available_height()),
            CaptionStyle::Gnome => Vec2::splat(24.0),
        };
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let (icon, label) = caption_face(which, maximized);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        let close = which == WindowCommand::Close;
        let (fill, ink) = caption_colours(style, close, &resp, &t);
        if style == CaptionStyle::Windows {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
        } else {
            ui.painter().circle_filled(rect.center(), 12.0, fill);
        }
        let glyph = match style {
            CaptionStyle::Windows => 14.0,
            CaptionStyle::Gnome => 12.0,
        };
        icon.paint(
            ui,
            Rect::from_center_size(rect.center(), Vec2::splat(glyph)),
            ink,
        );
        if resp.clicked() {
            clicked = Some(which);
        }
    }
    clicked
}

fn caption_face(which: WindowCommand, maximized: bool) -> (Icon, &'static str) {
    match which {
        WindowCommand::Minimize => (Icon::WindowMinimize, "Minimize"),
        WindowCommand::ToggleMaximize if maximized => (Icon::WindowRestore, "Restore"),
        WindowCommand::ToggleMaximize => (Icon::WindowMaximize, "Maximize"),
        WindowCommand::Close | WindowCommand::StartDrag | WindowCommand::ToggleFullscreen => {
            (Icon::Close, "Close")
        }
    }
}

fn caption_colours(
    style: CaptionStyle,
    close: bool,
    resp: &Response,
    t: &Tokens,
) -> (Color32, Color32) {
    let down = resp.is_pointer_button_down_on();
    let hot = resp.hovered() || down;
    match (style, close) {
        (CaptionStyle::Windows, true) if hot => {
            let red = if down { 0xc5_0f_1f } else { 0xe8_11_23 };
            (rgb(red), Color32::WHITE)
        }
        (CaptionStyle::Windows, _) if hot => (t.line, t.ink),
        (CaptionStyle::Windows, _) => (Color32::TRANSPARENT, t.ink),
        (CaptionStyle::Gnome, true) if hot => {
            let red = if down { 0xc5_3a_37 } else { 0xe0_52_4f };
            (rgb(red), Color32::WHITE)
        }
        (CaptionStyle::Gnome, _) => {
            let alpha = if down {
                0.34
            } else if hot {
                0.24
            } else {
                0.12
            };
            (t.ink.gamma_multiply(alpha), t.ink)
        }
    }
}

fn rgb(hex: u32) -> Color32 {
    let [_, r, g, b] = hex.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

/// Stock selectable buttons, `cols` to a row, sharing the width.
pub fn choices<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    all: &[T],
    label: fn(T) -> &'static str,
    cols: usize,
) {
    for row in all.chunks(cols) {
        ui.columns(cols, |cols| {
            for (ui, &v) in cols.iter_mut().zip(row) {
                let button = egui::Button::selectable(*value == v, label(v));
                if ui.add_sized([ui.available_width(), 28.0], button).clicked() {
                    *value = v;
                }
            }
        });
    }
}

/// Minus, the value, plus; laid right to left, as rows place controls.
pub fn stepper(
    ui: &mut Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    show: impl Fn(f32) -> String,
    label: &str,
) {
    let (lo, hi) = (*range.start(), *range.end());
    let up = ui.add_enabled_ui(*value < hi, |ui| {
        small_icon_button(ui, Icon::Plus, &format!("More {label}"))
    });
    let shown = egui::Label::new(show(*value));
    ui.add_sized([44.0, 24.0], shown);
    let down = ui.add_enabled_ui(*value > lo, |ui| {
        small_icon_button(ui, Icon::Minus, &format!("Less {label}"))
    });
    // Off-grid values, as imported ones may be, snap to the grid.
    if up.inner.clicked() {
        *value = ((*value / step).floor() * step + step).min(hi);
    }
    if down.inner.clicked() {
        *value = ((*value / step).ceil() * step - step).max(lo);
    }
}

/// An on/off switch; `label` names it for screen readers and tests.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, mut resp) = ui.allocate_exact_size(vec2(36.0, 20.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let (enabled, value) = (ui.is_enabled(), *on);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, value, label)
    });
    let how = ui.ctx().animate_bool_responsive(resp.id, value);
    let mut track = t.line.lerp_to_gamma(t.accent, how);
    if resp.hovered() {
        track = track.lerp_to_gamma(t.ink, 0.08);
    }
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), track);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, how);
    ui.painter()
        .circle_filled(pos2(x, rect.center().y), 8.0, Color32::WHITE);
    resp
}
