//! The word card and the mark menu.

use egui::{
    Align, Align2, Context, Frame, Id, Layout, Margin, Order, Rect, RichText, Stroke, Ui, pos2,
    vec2,
};
use mareader_library::MarkId;
use mareader_settings::GlossDensity;
use mareader_study::{Failure, WordInfo};

use super::Action;
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::{motion, widgets};

const WIDTH: f32 = 320.0;
/// Space between the word and the card.
const GAP: f32 = 8.0;
/// Below the word unless less than this is left there.
const ROOM_BELOW: f32 = 240.0;

pub enum Answer<'a> {
    Waiting,
    /// Filling in as the model streams.
    Partial(&'a WordInfo),
    Ready(&'a WordInfo),
    Failed(&'a Failure),
}

pub struct View<'a> {
    pub word: &'a str,
    pub answer: Answer<'a>,
    /// The word on screen.
    pub at: Rect,
    /// The pane it's kept inside.
    pub room: Rect,
    pub density: GlossDensity,
}

/// Draws the card beside its word; returns where it went.
pub fn show(ctx: &Context, view: &View<'_>, t: &Tokens, out: &mut Commands) -> Rect {
    let (at, room) = (view.at, view.room);
    let below = room.bottom() - at.bottom() >= ROOM_BELOW || at.top() - room.top() < ROOM_BELOW;
    let half = WIDTH / 2.0 + GAP;
    let x = at.center().x.clamp(
        room.left() + half,
        (room.right() - half).max(room.left() + half),
    );
    let (pos, pivot) = match below {
        true => (pos2(x, at.bottom() + GAP), Align2::CENTER_TOP),
        false => (pos2(x, at.top() - GAP), Align2::CENTER_BOTTOM),
    };
    let comfy = view.density == GlossDensity::Comfortable;
    egui::Area::new(Id::new("study-card"))
        .order(Order::Foreground)
        .pivot(pivot)
        .fixed_pos(pos)
        .constrain_to(room)
        .show(ctx, |ui| {
            Frame::popup(ui.style())
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius_lg)
                .inner_margin(Margin::same(if comfy { 18 } else { 14 }))
                .show(ui, |ui| {
                    ui.set_width(WIDTH);
                    ui.spacing_mut().item_spacing.y = if comfy { 10.0 } else { 6.0 };
                    body(ui, view, t, out);
                });
        })
        .response
        .rect
}

fn body(ui: &mut Ui, view: &View<'_>, t: &Tokens, out: &mut Commands) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(view.word)
                .size(t.text * 1.35)
                .strong()
                .color(t.ink),
        );
        if let Answer::Ready(info) | Answer::Partial(info) = &view.answer
            && !info.pos.is_empty()
        {
            ui.label(RichText::new(&info.pos).italics().color(t.muted));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::small_icon_button(ui, Icon::Close, "Close").clicked() {
                out.push(Command::Study(Action::Close));
            }
        });
    });
    match &view.answer {
        Answer::Waiting => shimmer(ui, t),
        Answer::Partial(info) => {
            answer(ui, info, t);
            explaining(ui, t);
        }
        Answer::Ready(info) => answer(ui, info, t),
        Answer::Failed(e) => {
            ui.horizontal_wrapped(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                Icon::Alert.paint(ui, rect, t.danger);
                ui.label(RichText::new(e.why.message()).color(t.muted));
            });
            if let Some(detail) = &e.detail {
                ui.label(RichText::new(detail).size(t.text_small).color(t.muted));
            }
            if e.why.retry() && ui.button("Try again").clicked() {
                out.push(Command::Study(Action::Retry));
            }
        }
    }
}

fn heading(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(text)
            .size(t.text_small)
            .strong()
            .color(t.muted),
    );
}

fn answer(ui: &mut Ui, info: &WordInfo, t: &Tokens) {
    if !info.meaning.is_empty() {
        heading(ui, "Meaning", t);
        ui.label(RichText::new(&info.meaning).color(t.ink));
    }
    if !info.synonyms.is_empty() {
        heading(ui, "Synonyms", t);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for s in &info.synonyms {
                Frame::new()
                    .fill(t.accent_soft)
                    .corner_radius(t.radius)
                    .inner_margin(Margin::symmetric(8, 2))
                    .show(ui, |ui| {
                        ui.label(RichText::new(s).size(t.text_small).color(t.ink));
                    });
            }
        });
    }
    if !info.usages.is_empty() {
        heading(ui, "Usages", t);
        for u in &info.usages {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("•").color(t.muted));
                ui.label(RichText::new(u).italics().color(t.ink));
            });
        }
    }
}

/// Grey bars that breathe while the answer is on its way.
fn shimmer(ui: &mut Ui, t: &Tokens) {
    let breathe = motion::enabled(ui.ctx());
    let phase = if breathe {
        ui.ctx().request_repaint();
        (ui.input(|i| i.time) * 3.0).sin() as f32 * 0.5 + 0.5
    } else {
        0.5
    };
    let tint = t.line.gamma_multiply(0.6 + 0.4 * phase);
    for share in [0.35, 1.0, 0.8] {
        let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, 12.0), egui::Sense::hover());
        let bar = Rect::from_min_size(rect.min, vec2(WIDTH * share, rect.height()));
        ui.painter().rect_filled(bar, 4.0, tint);
    }
    explaining(ui, t);
}

fn explaining(ui: &mut Ui, t: &Tokens) {
    ui.label(
        RichText::new("Explaining…")
            .size(t.text_small)
            .color(t.muted),
    );
}

/// The right-click menu on a mark; returns where it went.
pub fn menu(ctx: &Context, mark: MarkId, at: egui::Pos2, t: &Tokens, out: &mut Commands) -> Rect {
    egui::Area::new(Id::new("study-mark-menu"))
        .order(Order::Foreground)
        .fixed_pos(at)
        .constrain(true)
        .show(ctx, |ui| {
            Frame::menu(ui.style())
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius_lg)
                .inner_margin(Margin::same(4))
                .show(ui, |ui| {
                    ui.set_width(190.0);
                    ui.visuals_mut().override_text_color = Some(t.danger);
                    let row = widgets::menu_row(ui, Icon::Remove, "Remove highlight", None, false);
                    if row.clicked() {
                        out.push(Command::Study(Action::Remove(vec![mark])));
                    }
                });
        })
        .response
        .rect
}
