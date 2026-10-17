//! The lookup card: one entry at a time, its caret on the words.

use egui::{
    Align, Context, Direction, Frame, Id, Layout, Margin, Rect, RichText, ScrollArea, Sense,
    Stroke, Ui, vec2,
};
use mareader_dict::{Entry, Reply, language};

use super::{Action, entry};
use crate::bubble::Bubble;
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::settings_window::Tab;
use crate::shaped;
use crate::theme::Tokens;
use crate::widgets;

const WIDTH: f32 = 320.0;
/// Room each section keeps from the card's sides, and above and below.
const PAD: i8 = 16;
const PAD_Y: i8 = 12;
/// An entry taller than the card's room scrolls, never below this.
const ENTRY_MIN: f32 = 96.0;
/// The pager row under a word with several entries.
const FOOT: f32 = 40.0;
/// Wheel travel that turns to the next entry.
const STEP: f32 = 24.0;
/// A pause this long, in seconds, ends one flick of the wheel.
const SETTLE: f64 = 0.15;
/// Points in one line of a mouse wheel, as egui's default.
const LINE: f32 = 40.0;
const CARD: &str = "dict-card";

/// Which entry the card shows, and the wheel flick turning it.
#[derive(Clone, Copy, Debug, Default)]
struct Pager {
    at: usize,
    pull: f32,
    last: f64,
    /// This flick has turned an entry, or scrolled one: it turns no more.
    spent: bool,
    /// Last frame's entry was scrolled to its top, and to its bottom.
    edges: (bool, bool),
}

pub struct View<'a> {
    pub word: &'a str,
    /// None while the packs are asked.
    pub reply: Option<&'a Reply>,
    pub attached: bool,
    pub at: Rect,
    pub room: Rect,
}

pub fn show(ctx: &Context, view: &View<'_>, t: &Tokens, out: &mut Commands) -> Rect {
    let id = Id::new((CARD, view.word));
    let bubble = Bubble {
        id,
        at: view.at,
        room: view.room,
        width: WIDTH,
        margin: Margin::ZERO,
    };
    bubble
        .show(ctx, t, |ui, cap| card(ui, id, view, cap, t, out))
        .0
}

fn card(ui: &mut Ui, id: Id, view: &View<'_>, cap: f32, t: &Tokens, out: &mut Commands) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let top = ui.cursor().top();
    let way = view.reply.and_then(|r| match (&r.from, &r.to) {
        (Some(from), Some(to)) => Some(format!("{} → {}", language(from), language(to))),
        _ => None,
    });
    header(ui, view.word, way.as_deref(), t, out);
    rule(ui, t);
    let Some(reply) = view.reply else {
        section(ui, |ui| muted(ui, t, "Looking up…"));
        return;
    };
    let n = reply.entries.len();
    let pager_id = id.with(n);
    let mut p: Pager = ui.data(|d| d.get_temp(pager_id)).unwrap_or_default();
    p.at = p.at.min(n.saturating_sub(1));
    let Some(e) = reply.entries.get(p.at).filter(|_| view.attached) else {
        section(ui, |ui| empty(ui, view, reply, t, out));
        return;
    };
    let paged = n > 1;
    let used = ui.cursor().top() - top + f32::from(PAD_Y) * 2.0;
    let list = (cap - used - if paged { FOOT } else { 0.0 }).max(ENTRY_MIN);
    section(ui, |ui| body(ui, view.word, e, (&mut p, list), t));
    if paged {
        rule(ui, t);
        footer(ui, &mut p, n, t);
    }
    let wheel = wheel_over(ui.ctx(), id);
    turn(&mut p, n, wheel, ui.input(|i| i.time));
    ui.data_mut(|d| d.insert_temp(pager_id, p));
}

/// The word large, the close button, and the languages under them.
fn header(ui: &mut Ui, word: &str, way: Option<&str>, t: &Tokens, out: &mut Commands) {
    let margin = Margin {
        left: PAD,
        right: 8,
        top: 10,
        bottom: 10,
    };
    Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.horizontal(|ui| {
            // The close button claims its room first; the word takes the rest.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::small_icon_button(ui, Icon::Close, "Close").clicked() {
                    out.push(Command::Dict(Action::CloseCard));
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if shaped::needs_shaping(word) {
                        shaped::label(ui, word, t.text * 1.3, t.ink);
                    } else {
                        let big = RichText::new(word).size(t.text * 1.3).strong();
                        ui.add(egui::Label::new(big.color(t.ink)).truncate());
                    }
                });
            });
        });
        if let Some(way) = way {
            ui.label(RichText::new(way).size(t.text_small).color(t.muted));
        }
    });
}

/// The entry turned to, scrolling only when taller than the room.
fn body(ui: &mut Ui, asked: &str, e: &Entry, (p, list): (&mut Pager, f32), t: &Tokens) {
    let shown = ScrollArea::vertical()
        .id_salt((CARD, asked, p.at))
        .max_height(list)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if let Some(word) = entry::near(e, asked) {
                entry::heading(ui, word, t);
            }
            entry::rows(ui, e, t);
        });
    let room = (shown.content_size.y - shown.inner_rect.height()).max(0.0);
    let off = shown.state.offset.y;
    p.edges = (off <= 0.5, off >= room - 0.5);
}

/// ‹ 2 of 5 ›: the entries one at a time.
fn footer(ui: &mut Ui, p: &mut Pager, n: usize, t: &Tokens) {
    Frame::new()
        .inner_margin(Margin::symmetric(8, 7))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let back = ui.add_enabled_ui(p.at > 0, |ui| {
                    widgets::small_icon_button(ui, Icon::ChevronLeft, "Previous entry").clicked()
                });
                let middle = vec2(ui.available_width() - 24.0, 24.0);
                let centred = Layout::centered_and_justified(Direction::LeftToRight);
                ui.allocate_ui_with_layout(middle, centred, |ui| {
                    let count = format!("{} of {n}", p.at + 1);
                    ui.label(RichText::new(count).size(t.text_small).color(t.muted));
                });
                let next = ui.add_enabled_ui(p.at + 1 < n, |ui| {
                    widgets::small_icon_button(ui, Icon::ChevronRight, "Next entry").clicked()
                });
                if back.inner {
                    p.at = p.at.saturating_sub(1);
                }
                if next.inner {
                    p.at += 1;
                }
            });
        });
}

/// Why there is no entry, and the way to one when there is.
fn empty(ui: &mut Ui, view: &View<'_>, reply: &Reply, t: &Tokens, out: &mut Commands) {
    ui.spacing_mut().item_spacing.y = 8.0;
    if !view.attached {
        muted(ui, t, "Dictionaries need the desktop app.");
    } else if reply.ready == 0 {
        muted(ui, t, "No dictionary is downloaded.");
        if ui.button("Get dictionaries…").clicked() {
            out.push(Command::OpenSettings(Tab::Dictionary));
        }
    } else {
        let why = match &reply.failed {
            Some(e) => format!("The dictionary couldn't be read: {e}"),
            None => format!("No entry for “{}”.", view.word),
        };
        muted(ui, t, &why);
    }
}

/// A padded block across the card.
fn section(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .inner_margin(Margin::symmetric(PAD, PAD_Y))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// A hairline from edge to edge.
fn rule(ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, t.line));
}

fn muted(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).color(t.muted));
}

/// This frame's wheel, if the pointer is over the card.
fn wheel_over(ctx: &Context, id: Id) -> f32 {
    let card = ctx.memory(|m| m.area_rect(id));
    match (card, ctx.pointer_hover_pos()) {
        (Some(card), Some(at)) if card.contains(at) => ctx.input(wheel_points),
        _ => 0.0,
    }
}

/// The wheel's own travel this frame, before egui smooths it.
fn wheel_points(i: &egui::InputState) -> f32 {
    let one = |e: &egui::Event| match e {
        egui::Event::MouseWheel { unit, delta, .. } => match unit {
            egui::MouseWheelUnit::Point => delta.y,
            egui::MouseWheelUnit::Line => delta.y * LINE,
            egui::MouseWheelUnit::Page => delta.y * STEP * 4.0,
        },
        _ => 0.0,
    };
    i.events.iter().map(one).sum()
}

/// One flick turns one entry, once a scrolled entry has reached its edge.
fn turn(p: &mut Pager, n: usize, dy: f32, now: f64) {
    if dy == 0.0 {
        return;
    }
    if now - p.last > SETTLE {
        (p.spent, p.pull) = (false, 0.0);
    }
    p.last = now;
    let onward = dy < 0.0;
    let edge = if onward { p.edges.1 } else { p.edges.0 };
    if !edge {
        // The flick scrolls the entry; the next one may turn it.
        p.spent = true;
        return;
    }
    if p.spent {
        return;
    }
    p.pull += dy;
    if p.pull.abs() < STEP {
        return;
    }
    let to = if onward {
        Some(p.at + 1).filter(|&to| to < n)
    } else {
        p.at.checked_sub(1)
    };
    if let Some(to) = to {
        p.at = to;
        p.edges = (true, true);
    }
    (p.spent, p.pull) = (true, 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `p` after wheel `steps` of `(dy, seconds)`, `n` entries.
    fn wheel(mut p: Pager, n: usize, steps: &[(f32, f64)]) -> Pager {
        for &(dy, now) in steps {
            turn(&mut p, n, dy, now);
        }
        p
    }

    fn fits() -> Pager {
        Pager {
            edges: (true, true),
            last: -1.0,
            ..Pager::default()
        }
    }

    #[test]
    fn each_notch_turns_one_entry_and_stops_at_the_ends() {
        let p = wheel(fits(), 3, &[(-50.0, 0.0), (-50.0, 0.3)]);
        assert_eq!(p.at, 2);
        let p = wheel(p, 3, &[(-50.0, 0.6)]);
        assert_eq!(p.at, 2, "no entry past the last");
        let p = wheel(p, 3, &[(50.0, 0.9), (50.0, 1.2), (50.0, 1.5)]);
        assert_eq!(p.at, 0, "and none before the first");
    }

    #[test]
    fn a_long_trackpad_flick_turns_only_one_entry() {
        let flick: Vec<(f32, f64)> = (0..60).map(|i| (-6.0, f64::from(i) / 60.0)).collect();
        assert_eq!(wheel(fits(), 5, &flick).at, 1);
        // A small nudge turns nothing.
        assert_eq!(wheel(fits(), 5, &[(-6.0, 0.0), (-6.0, 0.02)]).at, 0);
    }

    #[test]
    fn a_tall_entry_scrolls_to_its_end_before_the_next_flick_turns() {
        let tall = Pager {
            edges: (true, false),
            ..fits()
        };
        let p = wheel(tall, 3, &[(-50.0, 0.0)]);
        assert_eq!(p.at, 0, "the entry scrolls instead");
        // Scrolled to its end, the same flick still doesn't turn.
        let p = wheel(
            Pager {
                edges: (false, true),
                ..p
            },
            3,
            &[(-50.0, 0.05)],
        );
        assert_eq!(p.at, 0);
        let p = wheel(p, 3, &[(-50.0, 0.5)]);
        assert_eq!(p.at, 1, "a fresh flick at the end turns");
    }
}
