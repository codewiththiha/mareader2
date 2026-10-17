//! Right-click menus on the page, the selection bar, and picked marks.

use egui::{Button, Color32, Id, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use mareader_library::{Book, Library, Shelf, ShelfId};

use super::books::counted;
use super::{Action, BookAction, Pick, PickAction, Picks, ShelfAction};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::{self, Tokens};

const BAR_H: f32 = 44.0;

fn act(out: &mut Commands, action: Action) {
    out.push(Command::Library(action));
}

fn book(out: &mut Commands, action: BookAction) {
    act(out, Action::Book(action));
}

fn shelf(out: &mut Commands, action: ShelfAction) {
    act(out, Action::Shelf(action));
}

/// The file manager's own name, as its menus say it.
fn file_manager() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else if cfg!(windows) {
        "Show in Explorer"
    } else {
        "Show in folder"
    }
}

/// A removal entry, in the theme's warning red.
fn danger(ui: &mut Ui, label: &str, t: &Tokens) -> bool {
    ui.button(RichText::new(label).color(t.danger)).clicked()
}

/// One book's menu; `at` is the shelf open on the page.
pub fn book_menu(ui: &mut Ui, b: &Book, at: Option<ShelfId>, t: &Tokens, out: &mut Commands) {
    let id = b.id;
    let open = if b.missing() { "Find…" } else { "Open" };
    if ui.button(open).clicked() {
        out.push(Command::OpenBook(id));
    }
    if ui.button("Select").clicked() {
        act(out, Action::Pick(PickAction::Toggle(Pick::Book(id))));
    }
    ui.separator();
    if ui.button("Rename…").clicked() {
        book(out, BookAction::AskRename(id));
    }
    if ui.button("Read separately").clicked() {
        book(out, BookAction::Twin(id));
    }
    if b.path().is_some() && ui.button(file_manager()).clicked() {
        book(out, BookAction::Reveal(id));
    }
    ui.separator();
    if at.is_some() && ui.button("Take off this shelf").clicked() {
        book(out, BookAction::TakeOff(vec![id]));
    }
    if danger(ui, "Remove…", t) {
        let books = vec![id];
        let shelves = Vec::new();
        book(out, BookAction::AskRemove { books, shelves });
    }
}

/// One shelf's menu.
pub fn shelf_menu(ui: &mut Ui, s: &Shelf, t: &Tokens, out: &mut Commands) {
    let id = s.id;
    if ui.button("Open").clicked() {
        shelf(out, ShelfAction::GoTo(Some(id)));
    }
    if ui.button("Select").clicked() {
        act(out, Action::Pick(PickAction::Toggle(Pick::Shelf(id))));
    }
    ui.separator();
    if ui.button("Rename…").clicked() {
        shelf(out, ShelfAction::GoTo(Some(id)));
        shelf(out, ShelfAction::StartRename(id));
    }
    if ui.button("New shelf inside").clicked() {
        shelf(out, ShelfAction::GoTo(Some(id)));
        shelf(out, ShelfAction::New);
    }
    ui.separator();
    if danger(ui, "Remove shelf…", t) {
        shelf(out, ShelfAction::AskRemove(id));
    }
}

/// The selection's menu, on any picked card.
pub fn picks_menu(ui: &mut Ui, picks: &Picks, at: Option<ShelfId>, t: &Tokens, out: &mut Commands) {
    let label = format!("{} selected", picks.len());
    ui.label(RichText::new(label).size(12.0).color(t.muted));
    ui.separator();
    together(ui, picks, at, t, out);
    if ui.button("Clear selection").clicked() {
        act(out, Action::Pick(PickAction::Clear));
    }
}

/// What a selection can do together: a menu's or the bar's entries.
fn together(ui: &mut Ui, picks: &Picks, at: Option<ShelfId>, t: &Tokens, out: &mut Commands) {
    let (books, shelves) = (picks.books(), picks.shelves());
    if ui.button("New shelf from these").clicked() {
        let (books, shelves) = (books.clone(), shelves.clone());
        book(out, BookAction::ShelveTogether { books, shelves });
    }
    if at.is_some() && !books.is_empty() && ui.button("Take off this shelf").clicked() {
        book(out, BookAction::TakeOff(books.clone()));
    }
    if danger(ui, "Remove…", t) {
        book(out, BookAction::AskRemove { books, shelves });
    }
}

/// The page's own menu, on empty space.
pub fn page_menu(ui: &mut Ui, model: &Library, sequence: &[Pick], out: &mut Commands) {
    if ui.button("New shelf").clicked() {
        shelf(out, ShelfAction::New);
    }
    if ui.button("Import books…").clicked() {
        book(out, BookAction::PickImport);
    }
    // Watched folders are walked on return; this asks now, for all.
    let folders = model.sources().next().is_some();
    if folders && ui.button("Rescan folders").clicked() {
        act(out, Action::Rescan(None));
    }
    let all = ui.add_enabled(!sequence.is_empty(), egui::Button::new("Select all"));
    if all.clicked() {
        act(out, Action::Pick(PickAction::Only(sequence.to_vec())));
    }
}

/// The bar along the page's foot while something is picked.
pub fn selection_bar(
    ui: &mut Ui,
    page: Rect,
    picks: &Picks,
    at: Option<ShelfId>,
    t: &Tokens,
    out: &mut Commands,
) {
    if picks.is_empty() {
        return;
    }
    let (books, shelves) = (picks.books().len(), picks.shelves().len());
    let what = match (books, shelves) {
        (b, 0) => counted(b, "book", "books"),
        (0, s) => counted(s, "shelf", "shelves"),
        (b, s) => format!(
            "{}, {}",
            counted(b, "book", "books"),
            counted(s, "shelf", "shelves")
        ),
    };
    let width = page.width().min(560.0);
    let rect = Rect::from_center_size(
        pos2(page.center().x, page.bottom() - BAR_H / 2.0 - 16.0),
        vec2(width, BAR_H),
    );
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    // Clicks on the bar's own background stay off the page.
    bar.interact(rect, Id::new("library-selection-bar"), Sense::CLICK);
    let shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: t.shadow,
    };
    let p = bar.painter();
    p.add(shadow.as_shape(rect, egui::CornerRadius::from(t.radius_lg)));
    let edge = Stroke::new(1.0, t.line);
    p.rect(rect, t.radius_lg, t.surface, edge, StrokeKind::Inside);
    let inner = rect.shrink2(vec2(12.0, 6.0));
    let row = egui::Layout::left_to_right(egui::Align::Center);
    bar.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(row), |ui| {
        let text = RichText::new(format!("{what} selected")).font(theme::medium(13.0));
        ui.label(text.color(t.ink));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear = crate::widgets::icon_button(ui, Icon::Close, "Clear selection", false);
            if clear.clicked() {
                act(out, Action::Pick(PickAction::Clear));
            }
            // Removal waits last, quiet and apart from the everyday actions.
            let (books, shelves) = (picks.books(), picks.shelves());
            let remove = Button::new(RichText::new("Remove…").color(t.danger)).frame(false);
            if ui.add(remove).clicked() {
                book(out, BookAction::AskRemove { books, shelves });
            }
            ui.separator();
            let books = picks.books();
            if at.is_some() && !books.is_empty() && ui.button("Take off this shelf").clicked() {
                book(out, BookAction::TakeOff(books));
            }
            if ui.button("New shelf from these").clicked() {
                let (books, shelves) = (picks.books(), picks.shelves());
                book(out, BookAction::ShelveTogether { books, shelves });
            }
        });
    });
}

/// A card's pick mark: a filled check if picked, else an empty ring.
pub fn mark(ui: &Ui, center: egui::Pos2, picked: bool, t: &Tokens) {
    let (p, r) = (ui.painter(), 10.0);
    if picked {
        p.circle(center, r, t.accent, Stroke::new(1.5, Color32::WHITE));
        let tick = Rect::from_center_size(center, vec2(13.0, 13.0));
        Icon::Check.paint(ui, tick, Color32::WHITE);
    } else {
        let fill = t.surface.gamma_multiply(0.85);
        p.circle(center, r, fill, Stroke::new(1.5, t.muted));
    }
}

/// The page's look while files from outside hover over it.
pub fn file_hint(ui: &Ui, page: Rect, onto: Option<&str>, t: &Tokens) {
    let p = ui.painter();
    p.rect_filled(page, t.radius_lg, t.accent.gamma_multiply(0.06));
    let ring = Stroke::new(2.0, t.accent.gamma_multiply(0.7));
    p.rect_stroke(page.shrink(4.0), t.radius_lg, ring, StrokeKind::Inside);
    let text = match onto {
        Some(name) => format!("Drop to add to “{name}”"),
        None => "Drop to add to your library".into(),
    };
    let g = p.layout_no_wrap(text, theme::medium(14.0), Color32::WHITE);
    let pill = Rect::from_center_size(
        pos2(page.center().x, page.bottom() - 48.0),
        g.size() + vec2(28.0, 14.0),
    );
    p.rect_filled(pill, pill.height() / 2.0, t.accent);
    p.galley(pill.min + vec2(14.0, 7.0), g, Color32::WHITE);
}

/// The model's shelf name, for the drop hint.
pub fn shelf_name(model: &Library, id: Option<ShelfId>) -> Option<String> {
    id.and_then(|id| model.shelf(id))
        .map(|s| s.name.value.clone())
}
