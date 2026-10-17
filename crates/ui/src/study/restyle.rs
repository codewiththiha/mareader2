//! A highlight's look, changed from its menu: its pen and colour.

use egui::{CornerRadius, Rect, Sense, Stroke, Ui, WidgetInfo, WidgetType, vec2};
use mareader_appearance::Rgb;
use mareader_library::{MarkId, Pen, Style};
use mareader_settings::{HighlightSettings, Reserved};

use super::Action;
use super::bar::pen_icon;
use crate::command::{Command, Commands};
use crate::theme::{Tokens, color};
use crate::widgets;

const SWATCH: f32 = 22.0;
const PEN_ROW: f32 = 46.0;

/// The colour a style shows: a reserved pick gives way to the pen's.
pub fn shown(style: Style, look: &HighlightSettings) -> Rgb {
    let picked = style.color.map(|[r, g, b]| Rgb(r, g, b));
    picked
        .filter(|c| look.allowed(style.pen, *c))
        .unwrap_or(look.color(style.pen))
}

/// The pen's own colour is kept as none, so it follows Settings.
fn style(pen: Pen, c: Rgb, look: &HighlightSettings) -> Style {
    let color = (c != look.color(pen)).then_some([c.0, c.1, c.2]);
    Style { pen, color }
}

/// Another pen keeps the colour shown, unless that look is reserved.
pub fn with_pen(now: Style, pen: Pen, look: &HighlightSettings) -> Style {
    let c = shown(now, look);
    match look.allowed(pen, c) {
        true => style(pen, c, look),
        false => Style { pen, color: None },
    }
}

/// Another colour, if the pen may take it.
pub fn with_color(now: Style, c: Rgb, look: &HighlightSettings) -> Option<Style> {
    look.allowed(now.pen, c).then(|| style(now.pen, c, look))
}

/// The pen's own colour, the palette, and the colour shown if neither.
pub fn offered(pen: Pen, now: Rgb, look: &HighlightSettings) -> Vec<Rgb> {
    let own = look.color(pen);
    let mut out = vec![own];
    out.extend(HighlightSettings::PALETTE.into_iter().filter(|c| *c != own));
    if !out.contains(&now) {
        out.push(now);
    }
    out
}

/// Why a look can't be used, if it can't.
pub fn kept_for(pen: Pen, c: Rgb, look: &HighlightSettings) -> Option<&'static str> {
    let wanted = Reserved { pen, color: c };
    if look.ai == wanted {
        Some("Kept for AI marks")
    } else if look.cefr == wanted {
        Some("Kept for CEFR shading")
    } else {
        None
    }
}

/// The pens as one segmented row, then the colours; each applies at once.
pub fn section(
    ui: &mut Ui,
    mark: MarkId,
    now: Style,
    look: &HighlightSettings,
    t: &Tokens,
    out: &mut Commands,
) {
    if let Some(pen) = pens(ui, now, look, t)
        && pen != now.pen
    {
        let next = with_pen(now, pen, look);
        out.push(Command::Study(Action::Restyle(mark, next)));
    }
    ui.add_space(8.0);
    if let Some(c) = swatches(ui, now, look, t)
        && let Some(next) = with_color(now, c, look)
        && next != now
    {
        out.push(Command::Study(Action::Restyle(mark, next)));
    }
    ui.add_space(2.0);
}

fn short(pen: Pen) -> &'static str {
    match pen {
        Pen::Full => "Highlight",
        Pen::Half => "Half",
        Pen::Underline => "Underline",
    }
}

/// Three cells, icon over name; each shows the colour it would take.
fn pens(ui: &mut Ui, now: Style, look: &HighlightSettings, t: &Tokens) -> Option<Pen> {
    let (track, _) = ui.allocate_exact_size(vec2(ui.available_width(), PEN_ROW), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect_filled(track, t.radius, t.line.gamma_multiply(0.35));
    let inner = track.shrink(2.0);
    let w = inner.width() / 3.0;
    let mut picked = None;
    for (i, pen) in Pen::ALL.into_iter().enumerate() {
        let min = inner.min + vec2(w * i as f32, 0.0);
        let cell = Rect::from_min_size(min, vec2(w, inner.height()));
        let resp = ui.interact(cell, ui.id().with(("restyle-pen", i)), Sense::click());
        let on = pen == now.pen;
        if on {
            painter.rect_filled(cell, t.radius - 2.0, t.surface);
            painter.rect_stroke(
                cell,
                t.radius - 2.0,
                Stroke::new(1.0, t.line),
                egui::StrokeKind::Inside,
            );
        } else if resp.hovered() {
            painter.rect_filled(cell, t.radius - 2.0, t.hover);
        }
        let ink = if on || resp.hovered() { t.ink } else { t.muted };
        let icon = Rect::from_center_size(cell.center_top() + vec2(0.0, 14.0), vec2(16.0, 16.0));
        pen_icon(pen).paint(ui, icon, ink);
        let strip = Rect::from_center_size(icon.center_bottom() + vec2(0.0, 3.0), vec2(14.0, 2.5));
        let would = shown(with_pen(now, pen, look), look);
        painter.rect_filled(strip, CornerRadius::same(1), color(would));
        let font = egui::FontId::proportional(t.text_small);
        let name = cell.center_bottom() - vec2(0.0, 10.0);
        painter.text(name, egui::Align2::CENTER_CENTER, short(pen), font, ink);
        resp.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, true, on, pen.label()));
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.clicked() {
            picked = Some(pen);
        }
    }
    picked
}

/// The colours; the one shown is ringed, reserved ones are struck out.
fn swatches(ui: &mut Ui, now: Style, look: &HighlightSettings, t: &Tokens) -> Option<Rgb> {
    let shown_now = shown(now, look);
    let own = look.color(now.pen);
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for c in offered(now.pen, shown_now, look) {
            let kept = kept_for(now.pen, c, look);
            let sense = if kept.is_some() {
                Sense::hover()
            } else {
                Sense::click()
            };
            let (rect, resp) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), sense);
            let on = c == shown_now;
            paint_swatch(ui, rect, c, on, kept.is_some(), resp.hovered(), t);
            let hex = c.to_hex();
            let name = if c == own {
                format!("{hex}, the highlighter's own")
            } else {
                hex
            };
            let enabled = kept.is_none();
            resp.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, enabled, on, &name));
            widgets::tip(&resp, kept.unwrap_or(&name), None);
            if kept.is_none() {
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    picked = Some(c);
                }
            }
        }
    });
    picked
}

fn paint_swatch(ui: &Ui, rect: Rect, c: Rgb, on: bool, kept: bool, hovered: bool, t: &Tokens) {
    let painter = ui.painter();
    let centre = rect.center();
    let r = SWATCH * 0.5 - 3.0;
    let fill = if kept {
        color(c).gamma_multiply(0.3)
    } else {
        color(c)
    };
    painter.circle_filled(centre, r, fill);
    painter.circle_stroke(centre, r, Stroke::new(1.0, t.line));
    if on {
        painter.circle_stroke(centre, r + 2.5, Stroke::new(1.5, t.ink));
    } else if hovered && !kept {
        painter.circle_stroke(centre, r + 2.5, Stroke::new(1.0, t.muted));
    }
    if kept {
        let d = vec2(r, -r) * std::f32::consts::FRAC_1_SQRT_2;
        painter.line_segment([centre - d, centre + d], Stroke::new(1.5, t.muted));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIOLET: Rgb = Rgb(0xa5, 0x8a, 0xf0);
    const ORANGE: Rgb = Rgb(0xff, 0xbe, 0x00);

    /// The AI keeps a full violet highlight.
    fn look() -> HighlightSettings {
        let mut look = HighlightSettings {
            ai: Reserved {
                pen: Pen::Full,
                color: VIOLET,
            },
            ..HighlightSettings::default()
        };
        look.sanitize();
        look
    }

    #[test]
    fn a_new_pen_keeps_the_colour_unless_it_is_kept_for_the_ai() {
        let look = look();
        let orange = Style {
            pen: Pen::Half,
            color: Some([ORANGE.0, ORANGE.1, ORANGE.2]),
        };
        let full = with_pen(orange, Pen::Full, &look);
        assert_eq!(shown(full, &look), ORANGE);
        let violet = Style {
            pen: Pen::Half,
            color: Some([VIOLET.0, VIOLET.1, VIOLET.2]),
        };
        let full = with_pen(violet, Pen::Full, &look);
        assert_eq!(full.color, None, "the pen's own colour instead");
        assert!(look.allowed(Pen::Full, shown(full, &look)));
    }

    #[test]
    fn a_reserved_colour_is_offered_but_never_taken() {
        let look = look();
        let full = Style {
            pen: Pen::Full,
            color: None,
        };
        assert!(offered(Pen::Full, shown(full, &look), &look).contains(&VIOLET));
        assert_eq!(
            kept_for(Pen::Full, VIOLET, &look),
            Some("Kept for AI marks")
        );
        assert_eq!(with_color(full, VIOLET, &look), None);
        let cefr = (look.cefr.pen, look.cefr.color);
        assert_eq!(
            kept_for(cefr.0, cefr.1, &look),
            Some("Kept for CEFR shading")
        );
    }

    #[test]
    fn the_pen_s_own_colour_is_kept_as_none_to_follow_settings() {
        let look = look();
        let red = Style {
            pen: Pen::Full,
            color: Some([0xff, 0, 0]),
        };
        let own = with_color(red, look.full, &look);
        assert_eq!(own.map(|s| s.color), Some(None));
    }
}
