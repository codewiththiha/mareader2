//! The few controls egui has no stock widget for.

use egui::{Button, Color32, CornerRadius, Rect, Response, Sense, Ui, Vec2, vec2};

use crate::command::WindowCommand;
use crate::icons::Icon;
use crate::theme::Tokens;

/// A borderless icon button; `selected` marks a toggle that is on.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
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
    resp.on_hover_text(label)
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
        WindowCommand::Close | WindowCommand::StartDrag => (Icon::Close, "Close"),
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
