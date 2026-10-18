//! Removal's alert: docked low, one line of meaning, then Remove.

use egui::{
    Align, Align2, Button, Color32, Context, Frame, Id, Key, Label, Layout, Margin, Modal,
    RichText, Stroke, Ui, vec2,
};
use mareader_library::{BookId, Library, LinkId, Receipt, Removing, ShelfId};

use super::moving::listed;
use super::{Action, BookAction};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets;

const KEY: &str = "library-remove";
/// Fixed, so the box never jumps as its words change.
const WIDTH: f32 = 440.0;
/// Clear of the window's foot.
const LIFT: f32 = 24.0;
/// Room for the switch beside a toggle's words.
const SWITCH_ROOM: f32 = 48.0;

/// Both start off on each opening: erasing is never a habit.
#[derive(Clone, Copy, Default)]
struct Switches {
    cascade: bool,
    forget: bool,
}

type Asked<'a> = (&'a [BookId], &'a [ShelfId], &'a [LinkId]);

/// The alert; Enter removes, Escape or Cancel keeps everything.
pub(super) fn show(ctx: &Context, model: &Library, asked: Asked<'_>, t: &Tokens) -> Option<Action> {
    let key = Id::new(KEY);
    let mut on = ctx
        .data(|d| d.get_temp::<Switches>(key))
        .unwrap_or_default();
    let (books, shelves, links) = asked;
    let going = model.removing(books, shelves, on.cascade);
    let receipt = model.receipt(&going.books);
    let frame = Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius_lg)
        .inner_margin(Margin::same(16))
        .shadow(ctx.global_style().visuals.window_shadow);
    let area =
        Modal::default_area(key.with("area")).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -LIFT));
    let backdrop = Color32::from_black_alpha(if t.dark { 60 } else { 28 });
    let mut act = None;
    let modal = Modal::new(key.with("modal"))
        .area(area)
        .frame(frame)
        .backdrop_color(backdrop)
        .show(ctx, |ui| {
            let width = WIDTH.min(ui.ctx().content_rect().width() - 64.0);
            ui.set_width(width.max(200.0));
            ui.spacing_mut().item_spacing.y = 12.0;
            heading(
                ui,
                &title(model, asked),
                message(asked, &going, &receipt),
                t,
            );
            if going.inside_books + going.inside_shelves > 0 {
                let note = inside(&going, on.cascade);
                toggle(
                    ui,
                    &mut on.cascade,
                    "Remove everything inside",
                    &note,
                    None,
                    t,
                );
            }
            let data = receipt.has_data();
            if data {
                let note = match on.forget {
                    true => "Erased for good. This can't be undone.",
                    false => "Kept for when the book comes back.",
                };
                let label = "Erase highlights and reading position";
                toggle(ui, &mut on.forget, label, note, Some(t.danger), t);
            }
            let erase = data && on.forget;
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if erase { "Erase & remove" } else { "Remove" };
                    let free = ui.ctx().memory(|m| m.focused().is_none());
                    let enter = free && ui.input(|i| i.key_pressed(Key::Enter));
                    if confirm(ui, label, t).clicked() || enter {
                        act = Some(Action::Book(BookAction::Remove {
                            books: books.to_vec(),
                            shelves: shelves.to_vec(),
                            links: links.to_vec(),
                            cascade: on.cascade,
                            forget: erase,
                        }));
                    }
                    if plain(ui, "Cancel", t).clicked() {
                        act = Some(Action::CloseSheet);
                    }
                });
            });
        });
    if act.is_none() && modal.should_close() {
        act = Some(Action::CloseSheet);
    }
    match act {
        Some(_) => ctx.data_mut(|d| d.remove::<Switches>(key)),
        None => ctx.data_mut(|d| {
            d.insert_temp(key, on);
        }),
    }
    act
}

/// The bin, the question, and a quieter line under it.
fn heading(ui: &mut Ui, title: &str, message: Option<String>, t: &Tokens) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.add(Icon::Remove.image(18.0).tint(t.danger));
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let text = RichText::new(title).font(theme::medium(15.0)).color(t.ink);
            ui.add(Label::new(text).wrap());
            if let Some(message) = message {
                ui.add(Label::new(RichText::new(message).color(t.muted)).wrap());
            }
        });
    });
}

/// What was asked for, by name when it's one thing.
fn title(model: &Library, (books, shelves, links): Asked<'_>) -> String {
    let book = |id| model.book(id).map(|b| b.title.value.clone());
    let shelf = |id| model.shelf(id).map(|s| s.name.value.clone());
    let link = |id| model.link(id).map(|l| l.name.value.clone());
    let named = match (books, shelves, links) {
        ([one], [], []) => book(*one).map(|n| format!("Remove “{n}”?")),
        ([], [one], []) => shelf(*one).map(|n| format!("Remove the shelf “{n}”?")),
        ([], [], [one]) => link(*one).map(|n| format!("Remove the link “{n}”?")),
        _ => None,
    };
    named.unwrap_or_else(|| {
        let counts = [
            (books.len(), "book", "books"),
            (shelves.len(), "shelf", "shelves"),
            (links.len(), "link", "links"),
        ];
        format!("Remove {}?", listed(&counts))
    })
}

/// The one thing worth knowing first: what happens to the files.
fn message(asked: Asked<'_>, going: &Removing, receipt: &Receipt) -> Option<String> {
    let (books, shelves, links) = asked;
    let n = going.books.len();
    if n == 0 {
        return match (shelves.len(), links.len()) {
            (0, 1) => Some("The book it points to stays.".to_owned()),
            (0, _) => Some("The books they point to stay.".to_owned()),
            _ if going.inside_books + going.inside_shelves > 0 => None,
            (1, _) => Some("Nothing is inside it.".to_owned()),
            _ => Some("Nothing is inside them.".to_owned()),
        };
    }
    let one = n == 1 && books.len() == 1;
    let files = match (receipt.copies, one) {
        (0, true) => "The file stays on disk.",
        (0, false) => "The files stay on disk.",
        (c, true) if c >= n => "The library's copy is deleted.",
        (c, false) if c >= n => "The library's copies are deleted.",
        _ => "Library copies are deleted; files read in place stay.",
    };
    Some(match (receipt.hidden, one) {
        (0, _) => files.to_owned(),
        (_, true) => format!("{files} Its folder won't add it back."),
        (_, false) => format!("{files} Their folders won't add them back."),
    })
}

/// What the inside switch means, on or off.
fn inside(going: &Removing, cascade: bool) -> String {
    let (b, s) = (going.inside_books, going.inside_shelves);
    let what = listed(&[(b, "book", "books"), (s, "shelf", "shelves")]);
    let verb = if b + s == 1 { "goes" } else { "go" };
    match (cascade, b, s) {
        (true, ..) => format!("{what} {verb} too."),
        (false, _, 0) => "Off, its books stay in the library.".to_owned(),
        (false, 0, _) => "Off, the shelves inside move up.".to_owned(),
        (false, ..) => "Off, books stay and shelves inside move up.".to_owned(),
    }
}

/// A switch with its meaning; `warn` tints the card while it's on.
fn toggle(ui: &mut Ui, on: &mut bool, label: &str, note: &str, warn: Option<Color32>, t: &Tokens) {
    let hot = warn.filter(|_| *on);
    let fill = hot.map_or(Color32::TRANSPARENT, |c| c.gamma_multiply(0.1));
    let line = hot.map_or(t.line, |c| c.gamma_multiply(0.5));
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, line))
        .corner_radius(t.radius)
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let room = (ui.available_width() - SWITCH_ROOM).max(0.0);
                let layout = Layout::top_down(Align::Min);
                ui.allocate_ui_with_layout(vec2(room, 0.0), layout, |ui| {
                    ui.set_width(room);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let ink = hot.unwrap_or(t.ink);
                    ui.add(Label::new(RichText::new(label).color(ink)).wrap());
                    let small = RichText::new(note).size(t.text_small).color(t.muted);
                    ui.add(Label::new(small).wrap());
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    widgets::switch(ui, on, label);
                });
            });
        });
}

/// The red button that does it.
fn confirm(ui: &mut Ui, label: &str, t: &Tokens) -> egui::Response {
    let text = RichText::new(label).color(Color32::WHITE);
    let button = Button::new(text)
        .fill(t.danger)
        .corner_radius(t.radius)
        .min_size(vec2(84.0, 30.0));
    ui.add(button)
}

/// A quiet button beside it.
fn plain(ui: &mut Ui, label: &str, t: &Tokens) -> egui::Response {
    let button = Button::new(RichText::new(label).color(t.ink))
        .corner_radius(t.radius)
        .min_size(vec2(72.0, 30.0));
    ui.add(button)
}
