//! Custom-painted controls. Stock egui widgets are only restyled.

use egui::{Color32, CornerRadius, Rect, Response, Sense, Ui, Vec2, vec2};

use crate::icons::{Icon, STROKE};
use crate::motion;
use crate::theme::{RADIUS_LG, Tokens};

/// Eased hover and press amounts for one control.
#[derive(Clone, Copy, Debug)]
pub struct Press {
    pub hover: f32,
    pub down: f32,
}

impl Press {
    /// `secs` is the control's CSS colour transition; 0 snaps.
    pub fn track(ui: &Ui, resp: &Response, secs: f32) -> Self {
        let ctx = ui.ctx();
        let down = resp.is_pointer_button_down_on();
        let secs = motion::secs(ctx, secs);
        Self {
            hover: ctx.animate_bool_with_time(resp.id.with("hover"), resp.hovered() || down, secs),
            down: ctx.animate_bool_with_time(resp.id.with("down"), down, secs),
        }
    }

    pub fn fill(self, rest: Color32, hover: Color32, pressed: Color32) -> Color32 {
        rest.lerp_to_gamma(hover, self.hover)
            .lerp_to_gamma(pressed, self.down)
    }
}

/// mareader's `IconButton`. Pressed tints the icon accent; no fill.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, pressed: bool) -> Response {
    ghost(ui, vec2(36.0, 36.0), icon, 18.0, label, pressed)
}

/// mareader's `Button` (Ghost) around an 18 pt icon: `h-9 px-2.5`.
pub fn ghost_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    ghost(ui, vec2(40.0, 36.0), icon, 18.0, label, false)
}

/// The compact Ghost `Button` (`h-8 px-2`) around a 14 pt icon.
pub fn compact_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    ghost(ui, vec2(32.0, 32.0), icon, 14.0, label, false)
}

fn ghost(
    ui: &mut Ui,
    size: Vec2,
    icon: Icon,
    icon_size: f32,
    label: &str,
    pressed: bool,
) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let resp = resp.on_hover_text(label);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, pressed, label));
    if ui.is_rect_visible(rect) {
        let p = Press::track(ui, &resp, 0.15);
        let fill = Color32::TRANSPARENT.lerp_to_gamma(t.line, p.hover);
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS_LG), fill);
        let tint = if pressed { t.accent } else { t.ink };
        icon.paint(
            ui,
            Rect::from_center_size(rect.center(), Vec2::splat(icon_size)),
            tint,
            STROKE,
        );
    }
    resp
}

/// What a caption button asks the window to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caption {
    Minimize,
    ToggleMaximize,
    Close,
}

const CLOSE_RED: Color32 = Color32::from_rgb(0xe8, 0x11, 0x23);
const CLOSE_RED_DOWN: Color32 = Color32::from_rgb(0xc5, 0x0f, 0x1f);
const GNOME_RED: Color32 = Color32::from_rgb(0xe0, 0x52, 0x4f);
const GNOME_RED_DOWN: Color32 = Color32::from_rgb(0xc5, 0x3a, 0x37);

fn caption_icon(which: Caption, maximized: bool) -> Icon {
    match which {
        Caption::Minimize => Icon::WindowMinimize,
        Caption::ToggleMaximize if maximized => Icon::WindowRestore,
        Caption::ToggleMaximize => Icon::WindowMaximize,
        Caption::Close => Icon::Close,
    }
}

fn caption_label(which: Caption, maximized: bool) -> &'static str {
    match which {
        Caption::Minimize => "Minimize",
        Caption::ToggleMaximize if maximized => "Restore",
        Caption::ToggleMaximize => "Maximize",
        Caption::Close => "Close",
    }
}

/// Windows squares: 46 pt wide, full bar height, flush right.
pub fn windows_captions(ui: &mut Ui, height: f32, maximized: bool) -> Option<Caption> {
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    for which in [Caption::Minimize, Caption::ToggleMaximize, Caption::Close] {
        let (rect, resp) = ui.allocate_exact_size(vec2(46.0, height), Sense::click());
        let label = caption_label(which, maximized);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        // `.win-btn` has no transition, and `:active` is opacity .72.
        let p = Press::track(ui, &resp, 0.0);
        let close = which == Caption::Close;
        let (fill, ink) = match (close, p.hover > 0.5, p.down > 0.5) {
            (true, _, true) => (CLOSE_RED_DOWN, Color32::WHITE),
            (true, true, _) => (CLOSE_RED, Color32::WHITE),
            (false, true, down) => (t.line.gamma_multiply(if down { 0.72 } else { 1.0 }), t.ink),
            _ => (Color32::TRANSPARENT, t.ink),
        };
        ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
        let glyph = Rect::from_center_size(rect.center(), Vec2::splat(14.0));
        caption_icon(which, maximized).paint(ui, glyph, ink, STROKE);
        if resp.clicked() {
            out = Some(which);
        }
    }
    out
}

/// GNOME circles: 24 pt, 8 pt apart, translucent ink fills.
pub fn gnome_captions(ui: &mut Ui, maximized: bool) -> Option<Caption> {
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    for which in [Caption::Minimize, Caption::ToggleMaximize, Caption::Close] {
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
        let label = caption_label(which, maximized);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        let p = Press::track(ui, &resp, 0.12);
        let close = which == Caption::Close;
        let rest = t.ink.gamma_multiply(0.12);
        let fill = if close {
            p.fill(rest, GNOME_RED, GNOME_RED_DOWN)
        } else {
            p.fill(rest, t.ink.gamma_multiply(0.24), t.ink.gamma_multiply(0.34))
        };
        ui.painter().circle_filled(rect.center(), 12.0, fill);
        let ink = if close {
            t.ink.lerp_to_gamma(Color32::WHITE, p.hover)
        } else {
            t.ink
        };
        let glyph = Rect::from_center_size(rect.center(), Vec2::splat(12.0));
        caption_icon(which, maximized).paint(ui, glyph, ink, STROKE);
        if resp.clicked() {
            out = Some(which);
        }
    }
    out
}
