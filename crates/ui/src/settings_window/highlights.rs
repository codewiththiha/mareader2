//! The Highlights tab: each pen's colour, the selection menu, the AI's look.

use egui::{Align2, FontId, Sense, Stroke, Ui, Vec2, vec2};
use mareader_appearance::Rgb;
use mareader_settings::{
    BarItem, GlossDensity, GlossSettings, HighlightSettings, Pen, PickColor, Reserved,
};

use super::{choice_row, divider, group, note, row, section};
use crate::icons::Icon;
use crate::study::marks::draw;
use crate::theme::{self, Tokens};
use crate::widgets;

const DOT: f32 = 22.0;

pub fn tab(ui: &mut Ui, h: &mut HighlightSettings, gloss: &mut GlossSettings) {
    section(ui, "Highlighters");
    group(ui, |ui| {
        for (i, pen) in Pen::ALL.into_iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            pen_row(ui, h, pen);
        }
        divider(ui);
        row(
            ui,
            "Fill strength",
            "How strongly fills tint the text",
            |ui| {
                ui.spacing_mut().slider_width = 140.0;
                let (lo, hi) = HighlightSettings::OPACITY;
                let mut pct = h.opacity * 100.0;
                let slider = egui::Slider::new(&mut pct, lo * 100.0..=hi * 100.0)
                    .step_by(5.0)
                    .fixed_decimals(0)
                    .suffix("%");
                if ui.add(slider).changed() {
                    h.opacity = pct / 100.0;
                }
            },
        );
    });
    section(ui, "When a highlighter is clicked");
    group(ui, |ui| {
        let picks = [
            (
                PickColor::Default,
                "Use its colour",
                "Highlights at once; new colours above recolour them.",
            ),
            (
                PickColor::Ask,
                "Choose a colour each time",
                "Shows the colours; the one picked stays with that highlight.",
            ),
        ];
        for (i, (pick, title, detail)) in picks.into_iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            if choice_row(ui, title, detail, h.pick == pick) {
                h.pick = pick;
            }
        }
    });
    section(ui, "Selection menu");
    group(ui, |ui| menu(ui, &mut h.bar));
    note(ui, "Hidden buttons stay off the bar under a selection.");
    section(ui, "Words the AI explained");
    group(ui, |ui| ai_rows(ui, h, gloss));
    let kept = "Highlights can't take the AI's look, so none passes for an AI mark.";
    note(ui, kept);
}

/// A pen's preview and the colours it may take.
fn pen_row(ui: &mut Ui, h: &mut HighlightSettings, pen: Pen) {
    let hint = "Its colour unless one is picked for a highlight";
    let opacity = h.opacity;
    let reserved = h.reserved();
    let mut color = h.color(pen);
    row(ui, pen.label(), hint, |ui| {
        let free = |c: Rgb| !reserved.contains(&Reserved { pen, color: c });
        palette(ui, (pen.label(), "Kept for AI marks"), &mut color, free);
        preview(ui, pen, color, opacity);
    });
    *h.color_mut(pen) = color;
}

/// "Aa" drawn over with `pen`, as a page shows it.
fn preview(ui: &mut Ui, pen: Pen, color: Rgb, opacity: f32) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(34.0, 22.0), Sense::hover());
    let line = rect.shrink2(vec2(3.0, 3.0));
    let painter = ui.painter();
    draw(painter, line, pen, color, opacity);
    let font = FontId::proportional(t.text);
    painter.text(rect.center(), Align2::CENTER_CENTER, "Aa", font, t.ink);
}

/// The palette as dots, laid right to left; kept ones can't be picked.
fn palette(ui: &mut Ui, (name, kept): (&str, &str), value: &mut Rgb, free: impl Fn(Rgb) -> bool) {
    let t = Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.x = 4.0;
    let mut srgb = [value.0, value.1, value.2];
    ui.spacing_mut().interact_size = Vec2::splat(20.0);
    let picker = egui::color_picker::color_edit_button_srgb(ui, &mut srgb);
    let custom = Rgb(srgb[0], srgb[1], srgb[2]);
    if custom != *value && free(custom) {
        *value = custom;
    }
    widgets::tip(&picker, "Custom", None);
    for c in HighlightSettings::PALETTE.into_iter().rev() {
        let (rect, r) = ui.allocate_exact_size(Vec2::splat(DOT), Sense::click());
        let (open, on) = (free(c), *value == c);
        let painter = ui.painter();
        let fill = theme::color(c);
        let fill = if open { fill } else { fill.gamma_multiply(0.3) };
        painter.circle_filled(rect.center(), 9.0, fill);
        if on {
            painter.circle_stroke(rect.center(), 10.5, Stroke::new(1.5, t.ink));
        }
        if !open {
            let d = vec2(5.0, -5.0);
            let slash = [rect.center() - d, rect.center() + d];
            painter.line_segment(slash, Stroke::new(1.5, t.muted));
        }
        let label = format!("{name} {}", c.to_hex());
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, open, on, &label));
        let tip = if open { c.to_hex() } else { kept.to_owned() };
        widgets::tip(&r, &tip, None);
        if open && r.clicked() {
            *value = c;
        }
    }
}

/// What a menu row asks for.
enum Move {
    Show(BarItem, bool),
    Up(usize),
    Down(usize),
}

/// The bar's buttons in order, then the hidden; each shown or not.
fn menu(ui: &mut Ui, bar: &mut Vec<BarItem>) {
    let hidden = BarItem::ALL.into_iter().filter(|i| !bar.contains(i));
    let listed: Vec<BarItem> = bar.iter().copied().chain(hidden).collect();
    let mut asked = None;
    for (i, item) in listed.iter().copied().enumerate() {
        if i > 0 {
            divider(ui);
        }
        let shown = i < bar.len();
        let label = item.label();
        let hint = if shown { "On the menu" } else { "Hidden" };
        row(ui, label, hint, |ui| {
            let mut on = shown;
            if widgets::switch(ui, &mut on, label).changed() || on != shown {
                asked = Some(Move::Show(item, on));
            }
            if !shown {
                return;
            }
            let down =
                widgets::small_icon_button(ui, Icon::MoveDown, &format!("Move {label} down"));
            if down.clicked() && i + 1 < bar.len() {
                asked = Some(Move::Down(i));
            }
            let up = widgets::small_icon_button(ui, Icon::MoveUp, &format!("Move {label} up"));
            if up.clicked() && i > 0 {
                asked = Some(Move::Up(i));
            }
        });
    }
    match asked {
        Some(Move::Show(item, true)) => bar.push(item),
        Some(Move::Show(item, false)) => bar.retain(|i| *i != item),
        Some(Move::Up(i)) => bar.swap(i, i - 1),
        Some(Move::Down(i)) => bar.swap(i, i + 1),
        None => {}
    }
}

/// The AI's pen and colour, which highlights then can't take.
fn ai_rows(ui: &mut Ui, h: &mut HighlightSettings, gloss: &mut GlossSettings) {
    let mut ai = h.ai;
    row(ui, "Mark", "How explained words are marked", |ui| {
        for pen in Pen::ALL.into_iter().rev() {
            let on = ai.pen == pen;
            if ui.add(egui::Button::selectable(on, pen.label())).clicked() {
                ai.pen = pen;
            }
        }
        preview(ui, ai.pen, ai.color, h.opacity);
    });
    divider(ui);
    let taken = h.color(ai.pen);
    row(ui, "Colour", "A pen's own colour can't be used", |ui| {
        let kept = "This pen's own colour for highlights";
        palette(ui, ("AI", kept), &mut ai.color, |c| c != taken);
    });
    // A new pen may meet that pen's own colour; settings move the pen.
    h.ai = ai;
    divider(ui);
    row(ui, "Card spacing", "How roomy the word card is", |ui| {
        let all = [GlossDensity::Comfortable, GlossDensity::Compact];
        for d in all {
            let on = gloss.density == d;
            if ui.add(egui::Button::selectable(on, d.label())).clicked() {
                gloss.density = d;
            }
        }
    });
}
