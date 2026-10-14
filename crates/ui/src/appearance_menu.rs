//! The Appearance popover: presets, mode, tint, texture and grain.

use egui::{
    Color32, CornerRadius, CursorIcon, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};
use mareader_appearance::{Appearance, BaseMode, NoiseMode, TextureMode, builtin_presets};

use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

pub const WIDTH: f32 = 300.0;
/// A hue picked at zero strength would show nothing.
const STRENGTH_ON_HUE: u8 = 18;

/// Edits a copy and pushes one command when anything changed.
pub fn show(ui: &mut Ui, current: &Appearance, out: &mut Commands) {
    let mut a = *current;
    ui.set_width(WIDTH);
    ui.spacing_mut().item_spacing.y = 8.0;
    // A smaller interact height keeps slider knobs in proportion.
    ui.spacing_mut().interact_size.y = 20.0;
    ui.label(RichText::new("Appearance").strong());
    presets(ui, &mut a);
    ui.separator();
    section(ui, "Mode");
    mode(ui, &mut a);
    section(ui, "Tint");
    if hue_strip(ui, &mut a.tint_hue).changed() && a.tint_strength == 0 {
        a.tint_strength = STRENGTH_ON_HUE;
    }
    slider(ui, "Strength", &mut a.tint_strength, 0..=100);
    section(ui, "Texture");
    widgets::choices(ui, &mut a.texture, &TextureMode::ALL, TextureMode::label, 3);
    if a.texture != TextureMode::None {
        slider(ui, "Opacity", &mut a.texture_opacity, 0..=100);
        slider(ui, "Scale", &mut a.texture_scale, 25..=400);
    }
    section(ui, "Grain");
    let grain = [NoiseMode::Off, NoiseMode::Static, NoiseMode::Animated];
    widgets::choices(ui, &mut a.noise, &grain, noise_label, 3);
    if a.noise != NoiseMode::Off {
        slider(ui, "Intensity", &mut a.noise_intensity, 0..=100);
    }
    if a != *current {
        out.push(Command::SetAppearance(a));
    }
}

fn section(ui: &mut Ui, title: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(title).size(t.text_small).color(t.muted));
}

/// Label left, full-width slider, value in percent on the right.
fn slider<N: egui::emath::Numeric>(
    ui: &mut Ui,
    label: &str,
    value: &mut N,
    range: std::ops::RangeInclusive<N>,
) {
    ui.horizontal(|ui| {
        let t = Tokens::get(ui.ctx());
        let text = egui::Label::new(RichText::new(label).color(t.muted));
        ui.add_sized([64.0, 20.0], text);
        ui.spacing_mut().slider_width = ui.available_width() - 56.0;
        ui.add(egui::Slider::new(value, range).suffix(" %"));
    });
}

fn noise_label(n: NoiseMode) -> &'static str {
    match n {
        NoiseMode::Off => "Off",
        NoiseMode::Static => "Static",
        NoiseMode::Animated => "Animated",
    }
}

fn mode_icon(b: BaseMode) -> Icon {
    match b {
        BaseMode::Light => Icon::Light,
        BaseMode::Dark => Icon::Dark,
        BaseMode::Dim => Icon::Dim,
    }
}

fn mode(ui: &mut Ui, a: &mut Appearance) {
    let t = Tokens::get(ui.ctx());
    ui.columns(3, |cols| {
        for (ui, b) in cols.iter_mut().zip(BaseMode::ALL) {
            let face = (mode_icon(b).image(t.icon - 2.0), b.label());
            let button =
                egui::Button::selectable(a.base == b, face).image_tint_follows_text_color(true);
            if ui.add_sized([ui.available_width(), 32.0], button).clicked() {
                a.base = b;
            }
        }
    });
}

/// The swatch hue matches what the tint filter produces (HSL 60 %, 55 %).
fn hue_colour(h: f32) -> Color32 {
    let (s, l) = (0.6, 0.55);
    let c = (1.0 - (2.0 * l - 1.0_f32).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let byte = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(byte(r), byte(g), byte(b))
}

/// Click or drag along the hue circle.
fn hue_strip(ui: &mut Ui, hue: &mut u16) -> Response {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), 18.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let strip = rect.shrink2(vec2(9.0, 3.0));
    if let Some(p) = resp.interact_pointer_pos() {
        let f = ((p.x - strip.left()) / strip.width()).clamp(0.0, 1.0);
        let h = (f * 359.0).round() as u16;
        if h != *hue {
            *hue = h;
            resp.mark_changed();
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    const STEPS: usize = 36;
    let w = strip.width() / STEPS as f32;
    let painter = ui.painter();
    for i in 0..STEPS {
        let seg = Rect::from_min_size(
            pos2(strip.left() + i as f32 * w, strip.top()),
            vec2(w + 0.5, strip.height()),
        );
        painter.rect_filled(seg, CornerRadius::ZERO, hue_colour(i as f32 * 10.0 + 5.0));
    }
    painter.rect_stroke(
        strip,
        CornerRadius::same(3),
        Stroke::new(1.0, t.line),
        StrokeKind::Outside,
    );
    let x = strip.left() + f32::from(*hue) / 359.0 * strip.width();
    let knob = pos2(x, rect.center().y);
    painter.circle(
        knob,
        8.0,
        hue_colour(f32::from(*hue)),
        Stroke::new(2.5, Color32::WHITE),
    );
    painter.circle_stroke(knob, 9.5, Stroke::new(1.0, t.shadow));
    let value = f64::from(*hue);
    resp.widget_info(|| egui::WidgetInfo::slider(true, value, "Hue"));
    resp
}

/// Thumbnails of the built-in looks, four to a row.
fn presets(ui: &mut Ui, a: &mut Appearance) {
    let t = Tokens::get(ui.ctx());
    let all = builtin_presets();
    for row in all.chunks(4) {
        ui.columns(4, |cols| {
            for (ui, p) in cols.iter_mut().zip(row) {
                let selected = p.appearance == *a;
                let w = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(vec2(w, w * 0.75 + 16.0), Sense::click());
                let page = Rect::from_min_size(rect.min, vec2(w, w * 0.75));
                swatch(ui, page, &p.appearance, selected || resp.hovered(), &t);
                let name = egui::Align2::CENTER_BOTTOM;
                ui.painter().text(
                    pos2(rect.center().x, rect.bottom()),
                    name,
                    &p.name,
                    egui::FontId::proportional(t.text_small - 1.0),
                    if selected { t.accent } else { t.ink },
                );
                let label = p.name.clone();
                resp.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &label)
                });
                if resp.clicked() {
                    *a = p.appearance;
                }
            }
        });
    }
}

/// A tiny page in the look's own colours.
fn swatch(ui: &Ui, page: Rect, look: &Appearance, hot: bool, t: &Tokens) {
    let c = look.ui_tokens();
    let rgb = |v: mareader_appearance::Rgb| Color32::from_rgb(v.0, v.1, v.2);
    let painter = ui.painter();
    let r = CornerRadius::same(6);
    painter.rect_filled(page, r, rgb(c.paper));
    let border = if hot {
        Stroke::new(2.0, t.accent)
    } else {
        Stroke::new(1.0, t.line)
    };
    painter.rect_stroke(page, r, border, StrokeKind::Inside);
    let inner = page.shrink2(Vec2::new(page.width() * 0.18, page.height() * 0.24));
    for (i, frac) in [1.0, 0.8, 0.6].into_iter().enumerate() {
        let y = inner.top() + i as f32 * inner.height() / 2.0;
        let line =
            Rect::from_min_size(pos2(inner.left(), y - 1.0), vec2(inner.width() * frac, 2.0));
        painter.rect_filled(line, CornerRadius::same(1), rgb(c.ink).gamma_multiply(0.7));
    }
    painter.circle_filled(page.right_top() + vec2(-7.0, 7.0), 3.0, rgb(c.accent));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_runs_round_the_hue_circle() {
        assert_eq!(hue_colour(0.0), hue_colour(360.0));
        let red = hue_colour(0.0);
        assert!(red.r() > red.g() && red.r() > red.b());
        let blue = hue_colour(240.0);
        assert!(blue.b() > blue.r() && blue.b() > blue.g());
    }
}
