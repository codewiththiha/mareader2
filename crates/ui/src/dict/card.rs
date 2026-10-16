//! The lookup card: a pack's entries for the words selected.

use egui::{
    Align, Align2, Context, Frame, Id, Layout, Margin, Order, Rect, RichText, ScrollArea, Stroke,
    Ui, pos2,
};
use mareader_dict::{Entry, Reply, WordMatch, fold, language};

use super::Action;
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::settings_window::Tab;
use crate::theme::Tokens;
use crate::widgets;

const WIDTH: f32 = 320.0;
const GAP: f32 = 8.0;
/// Below the words unless less than this is left there.
const ROOM_BELOW: f32 = 240.0;
/// Entries past this height scroll.
const LIST_HEIGHT: f32 = 280.0;

pub struct View<'a> {
    pub word: &'a str,
    /// None while the packs are asked.
    pub reply: Option<&'a Reply>,
    pub attached: bool,
    pub at: Rect,
    pub room: Rect,
}

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
    egui::Area::new(Id::new("dict-card"))
        .order(Order::Foreground)
        .pivot(pivot)
        .fixed_pos(pos)
        .constrain_to(room)
        .show(ctx, |ui| {
            Frame::popup(ui.style())
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius_lg)
                .inner_margin(Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(WIDTH);
                    ui.spacing_mut().item_spacing.y = 6.0;
                    body(ui, view, t, out);
                });
        })
        .response
        .rect
}

fn body(ui: &mut Ui, view: &View<'_>, t: &Tokens, out: &mut Commands) {
    ui.horizontal(|ui| {
        let word = RichText::new(view.word)
            .size(t.text * 1.35)
            .strong()
            .color(t.ink);
        ui.add(egui::Label::new(word).truncate());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::small_icon_button(ui, Icon::Close, "Close").clicked() {
                out.push(Command::Dict(Action::CloseCard));
            }
        });
    });
    let Some(reply) = view.reply else {
        muted(ui, t, "Looking up…");
        return;
    };
    if let (Some(from), Some(to)) = (&reply.from, &reply.to) {
        let way = format!("{} → {}", language(from), language(to));
        ui.label(RichText::new(way).size(t.text_small).color(t.muted));
    }
    if !view.attached {
        muted(ui, t, "Dictionaries need the desktop app.");
        return;
    }
    if reply.ready == 0 {
        muted(ui, t, "No dictionary is downloaded.");
        if ui.button("Get dictionaries…").clicked() {
            out.push(Command::OpenSettings(Tab::Dictionary));
        }
        return;
    }
    if reply.entries.is_empty() {
        let why = match &reply.failed {
            Some(e) => format!("The dictionary couldn't be read: {e}"),
            None => format!("No entry for “{}”.", view.word),
        };
        muted(ui, t, &why);
        return;
    }
    ScrollArea::vertical()
        .max_height(LIST_HEIGHT)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (i, entry) in reply.entries.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                entry_rows(ui, entry, view.word, t);
            }
        });
}

/// One entry: its answer first, then what qualifies it.
pub fn entry_rows(ui: &mut Ui, e: &Entry, asked: &str, t: &Tokens) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        // A bridge's asked side is the English between, not the ask.
        let other = e.via.is_none() && fold(e.asked()) != fold(asked);
        if e.fit != WordMatch::Exact || other {
            let near = RichText::new(format!("≈ {}", e.asked())).color(t.muted);
            ui.label(near).on_hover_text("The nearest entry");
        }
        let tags = e.tags();
        if !tags.is_empty() {
            ui.label(
                RichText::new(tags)
                    .italics()
                    .size(t.text_small)
                    .color(t.muted),
            );
        }
    });
    ui.label(RichText::new(e.answer()).color(t.ink));
    if let Some(r) = e.romanization.as_deref().filter(|r| !r.is_empty()) {
        ui.label(RichText::new(r).size(t.text_small).color(t.muted));
    }
    if let Some(s) = e.sense.as_deref().filter(|s| !s.is_empty()) {
        ui.label(RichText::new(s).italics().size(t.text_small).color(t.muted));
    }
    if let Some(via) = &e.via {
        ui.label(
            RichText::new(format!("via {via}"))
                .size(t.text_small)
                .color(t.muted),
        );
    }
}

fn muted(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).color(t.muted));
}
