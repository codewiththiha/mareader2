//! Design tokens and the egui restyle. Colours come from
//! `mareader_appearance`; metrics are mareader's CSS (`docs/look.md`).

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke};
use mareader_appearance::{Appearance, Rgb};

/// mareader's `TITLE_BAR_H`.
pub const TITLE_BAR_H: f32 = 48.0;
/// Tailwind `rounded-lg`: buttons, menu rows.
pub const RADIUS_LG: u8 = 8;
/// Tailwind `rounded-xl`: popovers, the selection menu.
pub const RADIUS_XL: u8 = 12;

/// Named families; only usable from the frame after [`install_fonts`].
pub const MEDIUM: &str = "inter-medium";
pub const SEMIBOLD: &str = "inter-semibold";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub paper: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub surface: Color32,
    pub line: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    /// Stands in for backdrop blur: mareader's fallback, surface at 96 %.
    pub glass: Color32,
    pub dark: bool,
}

fn color(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

impl Tokens {
    pub fn from_appearance(a: &Appearance) -> Self {
        let t = a.ui_tokens();
        let surface = color(t.surface);
        Self {
            paper: color(t.paper),
            ink: color(t.ink),
            muted: color(t.muted),
            surface,
            line: color(t.line),
            accent: color(t.accent),
            accent_soft: color(t.accent_soft),
            glass: Color32::from_rgba_unmultiplied(surface.r(), surface.g(), surface.b(), 245),
            dark: a.base.is_dark(),
        }
    }

    /// The tokens [`apply`] last stored, or the default look's.
    pub fn get(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<Tokens>(egui::Id::new(TOKENS_ID)))
            .unwrap_or_else(|| Self::from_appearance(&Appearance::default()))
    }
}

const TOKENS_ID: &str = "mareader-tokens";

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 3] = [
        (
            "inter-regular",
            include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
        ),
        (
            MEDIUM,
            include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
        ),
        (
            SEMIBOLD,
            include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
        ),
    ];
    for (name, bytes) in faces {
        fonts.font_data.insert(
            name.into(),
            std::sync::Arc::new(FontData::from_static(bytes)),
        );
    }
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter-regular".into());
    for name in [MEDIUM, SEMIBOLD] {
        let mut chain = vec![name.to_owned()];
        chain.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), chain);
    }
    ctx.set_fonts(fonts);
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

/// Restyles egui for `a` and stores its [`Tokens`] for [`Tokens::get`].
pub fn apply(ctx: &egui::Context, a: &Appearance) {
    let t = Tokens::from_appearance(a);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(TOKENS_ID), t));
    let mut v = if t.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = t.surface;
    v.window_fill = t.paper;
    v.window_stroke = Stroke::new(1.0, t.line);
    v.extreme_bg_color = t.surface;
    v.faint_bg_color = t.surface;
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent;
    v.override_text_color = Some(t.ink);
    v.window_corner_radius = CornerRadius::same(RADIUS_XL);
    v.menu_corner_radius = CornerRadius::same(RADIUS_XL);
    // --shadow-float: a 1 px line ring plus 0 8px 24px black at 30 %.
    let float = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(77),
    };
    v.window_shadow = float;
    v.popup_shadow = float;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(RADIUS_LG);
        w.fg_stroke.color = t.ink;
        w.bg_stroke = Stroke::NONE;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.line);
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = t.surface;
    // mareader's buttons are `hover:bg-line` with no pressed shade.
    for w in [
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.weak_bg_fill = t.line;
        w.bg_fill = t.line;
    }
    v.text_options.subpixel_binning = false;
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 6.0);
        s.spacing.scroll.floating = true;
        s.text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        s.text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.0));
        s.interaction.tooltip_delay = 0.35;
    });
}
