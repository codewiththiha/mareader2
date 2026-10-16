//! The Appearance popover: presets, mode and colour, texture and grain.

use egui::{
    Align, Color32, Context, CornerRadius, CursorIcon, Id, Key, Layout, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use mareader_appearance::{
    Appearance, BaseMode, FilmGrain, NoiseMode, Preset, TextureMode, group_presets, section_names,
};
use mareader_settings::TextSettings;

use crate::command::{Command, Commands, Independence};
use crate::gpu::{Anchor, Gpu, GrainCallback, PagesCallback, Quad, crawl_offset};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

pub const WIDTH: f32 = 300.0;
/// A hue picked at zero strength would show nothing.
const STRENGTH_ON_HUE: u8 = 18;
/// Grain switched on at zero intensity would read as a dead control.
const INTENSITY_ON: u8 = 25;
/// The tallest the popover grows before it scrolls, as mareader's 32 rem.
pub const MAX_HEIGHT: f32 = 512.0;
/// A card's texture against a page's, as mareader's preview.
const CARD_ZOOM: f32 = 0.34;
const CARD_RADIUS: f32 = 6.0;
/// Named landmarks on the hue circle, as mareader's.
const HUES: [(u16, &str); 7] = [
    (34, "Sepia"),
    (14, "Rose"),
    (104, "Green"),
    (160, "Mint"),
    (200, "Sky"),
    (240, "Blue"),
    (290, "Violet"),
];
const DRAFT: &str = "appearance-preset-draft";

/// What the popover reads besides the look it edits.
pub struct MenuContext<'a> {
    pub current: &'a Appearance,
    /// The per-pane modes, while the workspace is split.
    pub independence: Option<Independence>,
    /// Every preset, built-ins first.
    pub presets: &'a [Preset],
    /// The user's own, which can be deleted and name sections.
    pub user: &'a [Preset],
    /// Draws cards as real pages; without it they are flat.
    pub gpu: Option<&'a Gpu>,
    /// The ink dial's value while a book is active; none hides it.
    pub ink: Option<f32>,
    /// Offers the page texture; the library has no page.
    pub texture: bool,
}

/// Edits a copy and pushes one command when anything changed.
pub fn show(ui: &mut Ui, cx: &MenuContext<'_>, out: &mut Commands) {
    let mut a = *cx.current;
    ui.set_width(WIDTH);
    let max = (ui.ctx().content_rect().height() * 0.7).min(MAX_HEIGHT);
    // egui caps a popup's first frame; claiming the height lifts it.
    egui::ScrollArea::vertical()
        .max_height(max)
        .min_scrolled_height(max)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            // A smaller interact height keeps slider knobs in proportion.
            ui.spacing_mut().interact_size.y = 20.0;
            section(ui, "Presets");
            presets(ui, cx, &mut a, out);
            save_form(ui, cx.user, &a, out);
            ui.separator();
            section(ui, "Mode & colour");
            mode(ui, &mut a);
            hue(ui, &mut a);
            slider(ui, "Strength", &mut a.tint_strength, 0..=100);
            if let Some(ink) = cx.ink {
                ink_dial(ui, ink, out);
            }
            let mut split = cx.independence;
            if cx.texture {
                ui.separator();
                texture(ui, &mut a, split.as_mut());
            }
            ui.separator();
            section(ui, "Film grain");
            let before = a.noise;
            let grain = [NoiseMode::Off, NoiseMode::Static, NoiseMode::Animated];
            widgets::choices(ui, &mut a.noise, &grain, noise_label, 3);
            if a.noise != before && a.noise != NoiseMode::Off && a.noise_intensity == 0 {
                a.noise_intensity = INTENSITY_ON;
            }
            ui.add_enabled_ui(a.noise != NoiseMode::Off, |ui| {
                slider(ui, "Intensity", &mut a.noise_intensity, 0..=100);
            });
            if let Some(i) = &mut split {
                ui.separator();
                let hint = "Each pane keeps its own colour; grain stays shared.";
                switch_row(ui, "Independent theme for each", hint, &mut i.themes);
            }
            if split != cx.independence
                && let Some(i) = split
            {
                out.push(Command::SetIndependence(i));
            }
        });
    if a != *cx.current {
        out.push(Command::SetAppearance(a));
    }
}

/// The page texture, its dials and, while split, one per pane.
fn texture(ui: &mut Ui, a: &mut Appearance, split: Option<&mut Independence>) {
    section(ui, "Page texture");
    widgets::choices(ui, &mut a.texture, &TextureMode::ALL, TextureMode::label, 3);
    // Kept in place but inert, so the popover never jumps.
    ui.add_enabled_ui(a.texture != TextureMode::None, |ui| {
        slider(ui, "Opacity", &mut a.texture_opacity, 0..=100);
        slider(ui, "Scale", &mut a.texture_scale, 25..=400);
    });
    if let Some(i) = split {
        let hint = "Each pane keeps its own texture and dials.";
        switch_row(ui, "Texture for each", hint, &mut i.textures);
    }
}

/// A title over its hint, the switch at the right.
fn switch_row(ui: &mut Ui, title: &str, hint: &str, on: &mut bool) {
    let t = Tokens::get(ui.ctx());
    // The switch claims its room first; the text wraps in what is left.
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        widgets::switch(ui, on, title);
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(title);
            let hint = RichText::new(hint).size(t.text_small).color(t.muted);
            ui.add(egui::Label::new(hint).wrap());
        });
    });
}

/// Drops a half-typed preset once the popover has closed.
pub fn forget_draft(ctx: &Context) {
    ctx.data_mut(|d| d.remove::<Draft>(Id::new(DRAFT)));
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
        let named = ui.add_sized([64.0, 20.0], text).id;
        ui.spacing_mut().slider_width = ui.available_width() - 56.0;
        widgets::labelled(ui, named, |ui| {
            ui.add(egui::Slider::new(value, range).suffix(" %"));
        });
    });
}

/// mareader's text ink intensity: only a book's text takes it.
fn ink_dial(ui: &mut Ui, ink: f32, out: &mut Commands) {
    let (lo, hi) = TextSettings::INK;
    let shown = ink.round().clamp(lo, hi) as u8;
    let mut value = shown;
    slider(ui, "Text ink", &mut value, lo as u8..=hi as u8);
    if value != shown {
        out.push(Command::SetInkContrast(f32::from(value)));
    }
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

/// The hue, read out in degrees: a strip and one-click landmarks.
fn hue(ui: &mut Ui, a: &mut Appearance) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Hue").size(t.text_small).color(t.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(format!("{}°", a.tint_hue)).size(t.text_small));
        });
    });
    let mut changed = hue_strip(ui, &mut a.tint_hue).changed();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (h, name) in HUES {
            if hue_swatch(ui, h, name, a.tint_hue == h).clicked() {
                a.tint_hue = h;
                changed = true;
            }
        }
    });
    if changed && a.tint_strength == 0 {
        a.tint_strength = STRENGTH_ON_HUE;
    }
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

/// A landmark hue; a ring marks it picked, so the row never shifts.
fn hue_swatch(ui: &mut Ui, h: u16, name: &str, on: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    let painter = ui.painter();
    let c = rect.center();
    painter.circle(c, 9.0, hue_colour(f32::from(h)), Stroke::new(1.0, t.line));
    if on {
        painter.circle_stroke(c, 11.0, Stroke::new(2.0, t.accent));
    } else if resp.hovered() {
        painter.circle_stroke(c, 11.0, Stroke::new(2.0, t.line));
    }
    let label = format!("{name} hue");
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &label));
    resp.on_hover_text(name)
}

/// Every preset as a small page, four to a row, under its section.
fn presets(ui: &mut Ui, cx: &MenuContext<'_>, a: &mut Appearance, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    for group in group_presets(cx.presets) {
        let title = RichText::new(group.name.to_uppercase())
            .size(t.text_small - 2.0)
            .strong()
            .color(t.muted);
        ui.label(title);
        for row in group.presets.chunks(4) {
            ui.columns(4, |cols| {
                for (ui, p) in cols.iter_mut().zip(row) {
                    card(ui, cx, p, a, out);
                }
            });
        }
    }
}

/// One preset; picks it on click, and the user's own show a delete.
fn card(ui: &mut Ui, cx: &MenuContext<'_>, p: &Preset, a: &mut Appearance, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let selected = p.appearance == *a;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, w * 0.75 + 16.0), Sense::click());
    let page = Rect::from_min_size(rect.min, vec2(w, w * 0.75));
    let hot = ui.rect_contains_pointer(rect);
    paint_page(ui, cx.gpu, page, &p.appearance);
    let border = match (selected, hot) {
        (true, _) => Stroke::new(2.0, t.accent),
        (false, true) => Stroke::new(2.0, t.muted),
        (false, false) => Stroke::new(1.0, t.line),
    };
    let r = CornerRadius::same(CARD_RADIUS as u8);
    ui.painter()
        .rect_stroke(page, r, border, StrokeKind::Inside);
    let name = egui::FontId::proportional(t.text_small - 1.0);
    let colour = if selected { t.accent } else { t.ink };
    let at = pos2(rect.center().x, rect.bottom());
    ui.painter()
        .text(at, egui::Align2::CENTER_BOTTOM, &p.name, name, colour);
    let label = p.name.clone();
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &label)
    });
    let own = cx.user.iter().any(|u| u.id == p.id);
    if own && hot && delete_button(ui, page, p).clicked() {
        out.push(Command::DeletePreset(p.id.clone()));
    } else if resp.on_hover_text(&p.name).clicked() {
        *a = p.appearance;
    }
}

/// A round × on a card's corner, above the card's own click.
fn delete_button(ui: &mut Ui, page: Rect, p: &Preset) -> Response {
    let t = Tokens::get(ui.ctx());
    let rect = Rect::from_center_size(page.right_top() + vec2(-3.0, 3.0), Vec2::splat(18.0));
    let id = ui.id().with(("delete-preset", &p.id));
    let resp = ui.interact(rect, id, Sense::click());
    let painter = ui.painter();
    painter.circle(rect.center(), 9.0, t.surface, Stroke::new(1.0, t.line));
    let tint = if resp.hovered() { t.ink } else { t.muted };
    Icon::Close
        .image(10.0)
        .tint(tint)
        .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(10.0)));
    let label = format!("Delete {}", p.name);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    resp
}

/// A tiny page in the look's own paper, texture and grain.
fn paint_page(ui: &Ui, gpu: Option<&Gpu>, page: Rect, look: &Appearance) {
    let c = look.ui_tokens();
    let rgb = |v: mareader_appearance::Rgb| Color32::from_rgb(v.0, v.1, v.2);
    let painter = ui.painter();
    let clip = page.intersect(ui.clip_rect());
    match gpu {
        Some(gpu) => {
            let anchor = Anchor {
                origin: page.min,
                zoom: CARD_ZOOM,
            };
            let mut quad = Quad::backdrop(page, CARD_RADIUS, look.page_tone(), anchor);
            quad.clip = clip;
            painter.add(PagesCallback::shape(gpu.pass.clone(), page, vec![quad]));
        }
        None => {
            let r = CornerRadius::same(CARD_RADIUS as u8);
            painter.rect_filled(page, r, rgb(c.paper));
        }
    }
    let inner = page.shrink2(Vec2::new(page.width() * 0.18, page.height() * 0.24));
    for (i, frac) in [1.0, 0.8, 0.6].into_iter().enumerate() {
        let y = inner.top() + i as f32 * inner.height() / 2.0;
        let line =
            Rect::from_min_size(pos2(inner.left(), y - 1.0), vec2(inner.width() * frac, 2.0));
        painter.rect_filled(line, CornerRadius::same(1), rgb(c.ink).gamma_multiply(0.7));
    }
    painter.circle_filled(page.right_top() + vec2(-7.0, 7.0), 3.0, rgb(c.accent));
    if let (Some(gpu), Some(grain)) = (gpu, FilmGrain::of(look)) {
        let offset = crawl_offset(ui.ctx(), grain);
        let card = (page, CARD_RADIUS);
        painter.add(GrainCallback::card(gpu.grain.clone(), card, grain, offset));
    }
}

/// A preset being named; it lives in egui's memory while the form shows.
#[derive(Clone, Default)]
struct Draft {
    open: bool,
    name: String,
    group: String,
    focus: bool,
}

/// "Save current look": a name, an optional section, Enter or Save.
fn save_form(ui: &mut Ui, user: &[Preset], look: &Appearance, out: &mut Commands) {
    let id = Id::new(DRAFT);
    let mut d = ui.data(|m| m.get_temp::<Draft>(id)).unwrap_or_default();
    if d.open {
        draft_form(ui, user, look, &mut d, out);
    } else {
        let t = Tokens::get(ui.ctx());
        let face = (Icon::Plus.image(12.0), "Save current look");
        let button = egui::Button::new(face)
            .image_tint_follows_text_color(true)
            .stroke(Stroke::new(1.0, t.line))
            .fill(Color32::TRANSPARENT);
        if ui.add_sized([ui.available_width(), 26.0], button).clicked() {
            d.open = true;
            d.focus = true;
        }
    }
    ui.data_mut(|m| m.insert_temp(id, d));
}

fn field<'a>(text: &'a mut String, hint: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text)
        .hint_text(hint.to_owned())
        .desired_width(f32::INFINITY)
}

fn draft_form(ui: &mut Ui, user: &[Preset], look: &Appearance, d: &mut Draft, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let frame = egui::Frame::new()
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(8.0);
    frame.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let name = ui.add(field(&mut d.name, "Preset name"));
        widgets::name(&name, "Preset name");
        if std::mem::take(&mut d.focus) {
            name.request_focus();
        }
        let group = ui.add(field(&mut d.group, "Section (optional)"));
        widgets::name(&group, "Section");
        let sections = section_names(user);
        if !sections.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for s in sections {
                    if ui.selectable_label(d.group.trim() == s, s).clicked() {
                        d.group = s.to_owned();
                    }
                }
            });
        }
        let named = !d.name.trim().is_empty();
        let left = name.lost_focus() || group.lost_focus();
        let enter = left && ui.input(|i| i.key_pressed(Key::Enter));
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let (mut save, mut cancel) = (false, false);
        ui.columns(2, |cols| {
            if let [l, r] = cols {
                let ink = if t.dark {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                };
                let primary = egui::Button::new(RichText::new("Save").color(ink)).fill(t.accent);
                save = l
                    .add_enabled_ui(named, |ui| {
                        ui.add_sized([ui.available_width(), 24.0], primary)
                    })
                    .inner
                    .clicked();
                let size = [r.available_width(), 24.0];
                cancel = r.add_sized(size, egui::Button::new("Cancel")).clicked();
            }
        });
        if named && (save || enter) {
            out.push(Command::SavePreset {
                name: d.name.trim().to_owned(),
                group: d.group.trim().to_owned(),
                look: *look,
            });
            *d = Draft::default();
        } else if cancel || escape {
            *d = Draft::default();
        }
    });
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
