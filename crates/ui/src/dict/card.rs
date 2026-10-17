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
use crate::shaped;
use crate::theme::Tokens;
use crate::widgets;

const WIDTH: f32 = 320.0;
const GAP: f32 = 8.0;
/// Below the words unless less than this is left there.
const ROOM_BELOW: f32 = 240.0;
/// An entry taller than the room left scrolls, never below this.
const ENTRY_MIN: f32 = 120.0;
/// The word, the languages and the card's margins above the entry.
const HEAD: f32 = 96.0;
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
    /// The shown entry's full height, last frame; 0 before it is drawn.
    need: f32,
}

fn pager_id(word: &str, n: usize) -> Id {
    Id::new(("dict-page", word, n))
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
    let (at, room) = (view.at, view.room);
    let n = view.reply.map_or(0, |r| r.entries.len());
    let need = ctx.data(|d| d.get_temp::<Pager>(pager_id(view.word, n)));
    let need = need.map_or(0.0, |p| p.need);
    let rooms = (room.bottom() - at.bottom(), at.top() - room.top());
    let below = side(rooms, HEAD + GAP + need);
    let half = WIDTH / 2.0 + GAP;
    let x = at.center().x.clamp(
        room.left() + half,
        (room.right() - half).max(room.left() + half),
    );
    let (pos, pivot) = match below {
        true => (pos2(x, at.bottom() + GAP), Align2::CENTER_TOP),
        false => (pos2(x, at.top() - GAP), Align2::CENTER_BOTTOM),
    };
    let mut space = match below {
        true => room.bottom() - pos.y,
        false => pos.y - room.top(),
    };
    if HEAD + need > space {
        // Too tall for either side: the card slides over the words instead.
        space = room.height() - 2.0 * GAP;
    }
    let list = (space - HEAD).max(ENTRY_MIN);
    egui::Area::new(Id::new(CARD))
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
                    body(ui, view, list, t, out);
                });
        })
        .response
        .rect
}

/// Below the words if the card fits there, or fits above neither; else above.
fn side((below, above): (f32, f32), need: f32) -> bool {
    if need <= GAP + HEAD {
        // Not measured yet: below unless that leaves too little.
        return below >= ROOM_BELOW || above < ROOM_BELOW;
    }
    below >= need || (above < need && below >= above)
}

fn body(ui: &mut Ui, view: &View<'_>, list: f32, t: &Tokens, out: &mut Commands) {
    ui.horizontal(|ui| {
        // The close button claims its room first; the word takes the rest.
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if widgets::small_icon_button(ui, Icon::Close, "Close").clicked() {
                out.push(Command::Dict(Action::CloseCard));
            }
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                if shaped::needs_shaping(view.word) {
                    shaped::label(ui, view.word, t.text * 1.35, t.ink);
                } else {
                    let word = RichText::new(view.word)
                        .size(t.text * 1.35)
                        .strong()
                        .color(t.ink);
                    ui.add(egui::Label::new(word).truncate());
                }
            });
        });
    });
    let Some(reply) = view.reply else {
        muted(ui, t, "Looking up…");
        return;
    };
    let n = reply.entries.len();
    let id = pager_id(view.word, n);
    let mut p: Pager = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    p.at = p.at.min(n.saturating_sub(1));
    let way = match (&reply.from, &reply.to) {
        (Some(from), Some(to)) => Some(format!("{} → {}", language(from), language(to))),
        _ => None,
    };
    let paged = n > 1 && view.attached;
    if way.is_some() || paged {
        ui.horizontal(|ui| {
            if let Some(way) = way {
                ui.label(RichText::new(way).size(t.text_small).color(t.muted));
            }
            if paged {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    pager(ui, &mut p, n, t);
                });
            }
        });
    }
    let shown = show_entry(ui, view, reply, (&mut p, list), t);
    ui.data_mut(|d| d.insert_temp(id, p));
    if shown {
        return;
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
    }
}

/// The previous and next buttons around "2 / 5", laid right to left.
fn pager(ui: &mut Ui, p: &mut Pager, n: usize, t: &Tokens) {
    ui.spacing_mut().item_spacing.x = 2.0;
    let next = ui.add_enabled_ui(p.at + 1 < n, |ui| {
        widgets::small_icon_button(ui, Icon::ChevronDown, "Next entry").clicked()
    });
    let count = format!("{} / {n}", p.at + 1);
    ui.label(RichText::new(count).size(t.text_small).color(t.muted));
    let back = ui.add_enabled_ui(p.at > 0, |ui| {
        widgets::small_icon_button(ui, Icon::ChevronUp, "Previous entry").clicked()
    });
    if next.inner {
        p.at += 1;
    }
    if back.inner {
        p.at = p.at.saturating_sub(1);
    }
}

/// The entry turned to, sized to its text; false if there is none.
fn show_entry(
    ui: &mut Ui,
    view: &View<'_>,
    reply: &Reply,
    (p, list): (&mut Pager, f32),
    t: &Tokens,
) -> bool {
    let Some(e) = reply.entries.get(p.at).filter(|_| view.attached) else {
        return false;
    };
    // An entry fits whole; only one taller than the room scrolls.
    let out = ScrollArea::vertical()
        .id_salt(("dict-entry", p.at))
        .max_height(list)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if let Some(word) = near(e, view.word) {
                heading(ui, word, t);
            }
            entry_rows(ui, e, t);
        });
    p.need = out.content_size.y;
    let room = (out.content_size.y - out.inner_rect.height()).max(0.0);
    let off = out.state.offset.y;
    p.edges = (off <= 0.5, off >= room - 0.5);
    let wheel = wheel_over_card(ui.ctx());
    turn(p, reply.entries.len(), wheel, ui.input(|i| i.time));
    true
}

/// This frame's wheel, if the pointer is over the card.
fn wheel_over_card(ctx: &Context) -> f32 {
    let card = ctx.memory(|m| m.area_rect(Id::new(CARD)));
    let over = ctx.pointer_hover_pos();
    match (card, over) {
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

/// The headword an entry stands for when it isn't the word asked.
fn near<'a>(e: &'a Entry, asked: &str) -> Option<&'a str> {
    // A bridge's asked side is the English between, not the ask.
    let other = e.via.is_none() && fold(e.asked()) != fold(asked);
    (e.fit != WordMatch::Exact || other).then(|| e.asked())
}

fn heading(ui: &mut Ui, word: &str, t: &Tokens) {
    let text = format!("≈ {word}");
    shaped::label(ui, &text, t.text_small, t.muted).on_hover_text("The nearest entry");
}

/// Entries under one heading per nearest word; `after` adds to each.
pub fn entries(
    ui: &mut Ui,
    list: &[Entry],
    asked: &str,
    t: &Tokens,
    mut after: impl FnMut(&mut Ui, &Entry),
) {
    let mut shown: Option<&str> = None;
    for (i, e) in list.iter().enumerate() {
        let near = near(e, asked);
        let new_group = near.is_some() && near != shown;
        if i > 0 {
            ui.separator();
        }
        if new_group && let Some(word) = near {
            heading(ui, word, t);
        }
        shown = near;
        entry_rows(ui, e, t);
        after(ui, e);
    }
}

/// One entry: tags, answer, then qualifiers, each row as tall as its text.
fn entry_rows(ui: &mut Ui, e: &Entry, t: &Tokens) {
    let tags = e.tags();
    if !tags.is_empty() {
        ui.label(
            RichText::new(tags)
                .italics()
                .size(t.text_small)
                .color(t.muted),
        );
    }
    shaped::label(ui, e.answer(), t.text, t.ink);
    if let Some(r) = e.romanization.as_deref().filter(|r| !r.is_empty()) {
        shaped::label(ui, r, t.text_small, t.muted);
    }
    if let Some(s) = e.sense.as_deref().filter(|s| !s.is_empty()) {
        if shaped::needs_shaping(s) {
            shaped::label(ui, s, t.text_small, t.muted);
        } else {
            ui.label(RichText::new(s).italics().size(t.text_small).color(t.muted));
        }
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
    fn a_tall_entry_takes_the_side_it_fits() {
        // Burmese lines run tall: 400 points fit above, not below.
        assert!(!side((300.0, 500.0), 400.0));
        assert!(side((500.0, 300.0), 400.0));
        // Neither fits: the roomier side, the card sliding over the words.
        assert!(side((350.0, 300.0), 600.0));
        assert!(!side((300.0, 350.0), 600.0));
        // Unmeasured, below wins while it leaves room.
        assert!(side((300.0, 500.0), 0.0));
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
