//! Sheets over the library: finding, removing, renaming, adding folders.

use std::path::PathBuf;

use egui::{Button, Color32, Context, Frame, Id, Margin, Modal, RichText, Stroke, Ui};
use mareader_library::{Book, BookId, Library, Receipt, Shelf, ShelfId, Shelving, Source};

use super::books::counted;
use super::{
    Action, BookAction, Finding, FolderKeep, FolderPlan, LibraryState, Sheet, ShelfAction, Step,
};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::{self, Tokens};

const WIDTH: f32 = 420.0;

/// The open sheet, if any, drawn over everything.
pub fn show(ctx: &Context, library: &LibraryState, out: &mut Commands) {
    let Some(sheet) = library.sheet() else {
        return;
    };
    let t = Tokens::get(ctx);
    let model = library.model();
    let frame = Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius_lg)
        .inner_margin(Margin::same(20))
        .shadow(ctx.global_style().visuals.window_shadow);
    let backdrop = Color32::from_black_alpha(if t.dark { 110 } else { 70 });
    let mut act = None;
    let modal = Modal::new(Id::new("library-sheet"))
        .frame(frame)
        .backdrop_color(backdrop)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing.y = 8.0;
            act = match sheet {
                Sheet::Find(finding) => match model.book(finding.book) {
                    Some(book) => find(ui, library, book, finding, &t),
                    None => Some(Action::CloseSheet),
                },
                Sheet::Remove(id) => match model.source(*id) {
                    Some(source) => remove(ui, model, source, &t),
                    None => Some(Action::CloseSheet),
                },
                Sheet::RemoveShelf(id) => match model.shelf(*id) {
                    Some(shelf) => remove_shelf(ui, model, shelf, &t),
                    None => Some(Action::CloseSheet),
                },
                Sheet::Removal {
                    books,
                    shelves,
                    receipt,
                } => removal(ui, model, (books, shelves), receipt, &t),
                Sheet::RenameBook(id) => match model.book(*id) {
                    Some(book) => rename(ui, book, &t),
                    None => Some(Action::CloseSheet),
                },
                Sheet::AddFolders(roots) => {
                    let inside = library.at().and_then(|id| model.shelf(id));
                    let inside = inside.map(|s| s.name.value.as_str());
                    add_folders(ui, roots, inside, &t)
                }
            };
        });
    let working = matches!(sheet, Sheet::Find(f) if matches!(f.step, Step::Working(_)));
    if modal.should_close() && !working {
        act = Some(Action::CloseSheet);
    }
    out.extend(act.map(Command::Library));
}

fn find(
    ui: &mut Ui,
    library: &LibraryState,
    book: &Book,
    finding: &Finding,
    t: &Tokens,
) -> Option<Action> {
    let id = book.id;
    title(ui, &format!("Find “{}”", book.title.value), t);
    let last = book.locations.first().map(|l| &l.path);
    let source = last.and_then(|p| library.model().source_of(p));
    let offline = source.filter(|s| library.offline().contains(&s.id));
    let why = match offline {
        Some(s) => format!(
            "Its folder, “{}”, isn't there: a drive may be disconnected. \
             Connect it, or point to the file elsewhere.",
            s.name()
        ),
        None => "It was moved, renamed or deleted since you last read it.".into(),
    };
    ui.label(RichText::new(why).color(t.ink));
    if let Some(path) = last {
        let path = path.display().to_string();
        ui.label(RichText::new(path).size(12.0).color(t.muted));
    }
    let mut act = None;
    let working = matches!(finding.step, Step::Working(_));
    match &finding.step {
        Step::Ask => {}
        Step::Working(doing) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(doing).color(t.muted));
            });
        }
        Step::Note(note) => {
            ui.label(RichText::new(note).color(t.danger));
        }
        Step::Differs { path, .. } => {
            let name = path.file_name().map(|n| n.to_string_lossy());
            let name = name.unwrap_or_default();
            let note = format!(
                "“{name}” isn't the same file: perhaps another edition, or edited \
                 since. Use it, and your place is kept."
            );
            ui.label(RichText::new(note).color(t.ink));
            ui.add_space(8.0);
            buttons(ui, |ui| {
                if primary(ui, "Use this file", t.accent, t).clicked() {
                    act = Some(Action::UseAnyway(id));
                }
                if ui.button("Choose another…").clicked() {
                    act = Some(Action::PickFile(id));
                }
                if ui.button("Cancel").clicked() {
                    act = Some(Action::CloseSheet);
                }
            });
            return act;
        }
    }
    ui.add_space(8.0);
    buttons(ui, |ui| {
        let choose = ui.add_enabled_ui(!working, |ui| primary(ui, "Choose the file…", t.accent, t));
        if choose.inner.clicked() {
            act = Some(Action::PickFile(id));
        }
        let search = ui.add_enabled(!working, Button::new("Search a folder…"));
        if search.clicked() {
            act = Some(Action::PickSearchFolder(id));
        }
        if ui.button("Cancel").clicked() {
            act = Some(Action::CloseSheet);
        }
    });
    act
}

fn remove(ui: &mut Ui, model: &Library, source: &Source, t: &Tokens) -> Option<Action> {
    title(ui, &format!("Remove “{}”?", source.name()), t);
    let only_here = model
        .books_in(source.id)
        .filter(|b| b.locations.iter().all(|l| source.contains(&l.path)))
        .count();
    let leave = match only_here {
        0 => "No books leave the library: each is also somewhere else.".to_owned(),
        1 => "1 book found only here leaves the library.".to_owned(),
        n => format!("{n} books found only here leave the library."),
    };
    let body = format!(
        "{leave} The files stay where they are; add the folder again to bring \
         them back, with your places."
    );
    ui.label(RichText::new(body).color(t.ink));
    ui.add_space(8.0);
    let mut act = None;
    buttons(ui, |ui| {
        if primary(ui, "Remove", t.danger, t).clicked() {
            act = Some(Action::RemoveFolder(source.id));
        }
        if ui.button("Cancel").clicked() {
            act = Some(Action::CloseSheet);
        }
    });
    act
}

fn remove_shelf(ui: &mut Ui, model: &Library, shelf: &Shelf, t: &Tokens) -> Option<Action> {
    title(ui, &format!("Remove “{}”?", shelf.name.value), t);
    let (books, shelves) = model.counts(shelf.id);
    let up = match model.parent_of(shelf).and_then(|p| model.shelf(p)) {
        Some(parent) => format!("“{}”", parent.name.value),
        None => "the top of the library".into(),
    };
    let books = match books {
        0 => "It holds no books.".to_owned(),
        1 => "Its 1 book stays in the library.".to_owned(),
        n => format!("Its {n} books stay in the library."),
    };
    let body = match shelves {
        0 => books,
        1 => format!("{books} The shelf inside it moves up to {up}."),
        n => format!("{books} The {n} shelves inside it move up to {up}."),
    };
    ui.label(RichText::new(body).color(t.ink));
    ui.add_space(8.0);
    let mut act = None;
    buttons(ui, |ui| {
        if primary(ui, "Remove", t.danger, t).clicked() {
            act = Some(Action::Shelf(ShelfAction::Remove(shelf.id)));
        }
        if ui.button("Cancel").clicked() {
            act = Some(Action::CloseSheet);
        }
    });
    act
}

/// What removing books does, said before it's done.
fn removal(
    ui: &mut Ui,
    model: &Library,
    (books, shelves): (&[BookId], &[ShelfId]),
    receipt: &Receipt,
    t: &Tokens,
) -> Option<Action> {
    let n = receipt.books;
    let one = books
        .first()
        .and_then(|id| model.book(*id))
        .filter(|_| n == 1);
    let what = match (one, shelves.len()) {
        (Some(book), 0) => format!("“{}”", book.title.value),
        (_, 0) => counted(n, "book", "books"),
        (_, s) if n == 0 => counted(s, "shelf", "shelves"),
        (_, s) => format!(
            "{} and {}",
            counted(n, "book", "books"),
            counted(s, "shelf", "shelves")
        ),
    };
    title(ui, &format!("Remove {what}?"), t);
    let mut lines = Vec::new();
    match receipt.hidden {
        0 => {}
        1 if n == 1 => {
            lines.push("Its file stays in its folder; restore it from the folder's menu.".into())
        }
        h => lines.push(format!(
            "{} stay in their folders as files; restore them from each folder's menu.",
            counted(h, "book", "books")
        )),
    }
    match receipt.copies {
        0 => {}
        1 => lines.push("The copy Mareader keeps is deleted.".to_owned()),
        c => lines.push(format!("The {c} copies Mareader keeps are deleted.")),
    }
    let elsewhere = receipt.hidden == 0 && receipt.copies == 0;
    if n > 0 && elsewhere {
        lines.push("Files stay where they are; open one again to bring it back.".into());
    }
    if receipt.positions > 0 || receipt.spots > 0 {
        lines.push("Places and shelf spots are kept, should books come back.".into());
    }
    if !shelves.is_empty() {
        let they = if shelves.len() == 1 { "its" } else { "their" };
        lines.push(format!(
            "Books on the shelves stay in the library, off {they} shelf."
        ));
    }
    for line in lines {
        ui.label(RichText::new(line).color(t.ink));
    }
    ui.add_space(8.0);
    let mut act = None;
    buttons(ui, |ui| {
        if primary(ui, "Remove", t.danger, t).clicked() {
            let (books, shelves) = (books.to_vec(), shelves.to_vec());
            act = Some(Action::Book(BookAction::Remove { books, shelves }));
        }
        if ui.button("Cancel").clicked() {
            act = Some(Action::CloseSheet);
        }
    });
    act
}

/// A book's own name, edited; Enter saves it.
fn rename(ui: &mut Ui, book: &Book, t: &Tokens) -> Option<Action> {
    title(ui, &format!("Rename “{}”", book.title.value), t);
    let key = Id::new(("rename-book", book.id));
    let first = ui.data(|d| d.get_temp::<String>(key).is_none());
    let mut draft = ui.data_mut(|d| {
        d.get_temp_mut_or_insert_with(key, || book.title.value.clone())
            .clone()
    });
    let edit = egui::TextEdit::singleline(&mut draft)
        .id(key)
        .desired_width(f32::INFINITY)
        .char_limit(mareader_library::NAME_MAX);
    let resp = ui.add(edit);
    if first {
        super::crumbs::select_all(&resp, key, &draft);
    }
    ui.data_mut(|d| d.insert_temp(key, draft.clone()));
    let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.add_space(8.0);
    let mut act = None;
    buttons(ui, |ui| {
        let blank = draft.trim().is_empty();
        let save = ui.add_enabled_ui(!blank, |ui| primary(ui, "Rename", t.accent, t));
        if save.inner.clicked() || (entered && !blank) {
            act = Some(Action::Book(BookAction::Rename(book.id, draft.clone())));
        }
        if ui.button("Cancel").clicked() {
            act = Some(Action::CloseSheet);
        }
    });
    // The next rename starts from the book's name again.
    if act.is_some() {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    act
}

/// One sheet for a folder: which files count, how they're kept, shelved.
fn add_folders(ui: &mut Ui, roots: &[PathBuf], inside: Option<&str>, t: &Tokens) -> Option<Action> {
    match roots {
        [_] => title(ui, "Add a folder", t),
        many => title(ui, &format!("Add {} folders", many.len()), t),
    }
    let key = Id::new("add-folders-plan");
    // The answers outlive the sheet: the next folder often matches.
    let mut plan = ui.data_mut(|d| {
        d.get_temp_mut_or_insert_with(key, FolderPlan::default)
            .clone()
    });
    let mut act = None;
    egui::ScrollArea::vertical()
        .max_height(ui.ctx().content_rect().height() * 0.7)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            section(ui, "Folder", t);
            if folder_row(ui, roots, t) {
                act = Some(Action::PickFolder);
            }
            section(ui, "Formats", t);
            let walk = &mut plan.walk;
            let (formats, exclude, min) =
                (&mut walk.formats, &mut walk.exclude, &mut walk.min_size);
            super::folders::what_counts(ui, formats, exclude, min);
            section(ui, "Books", t);
            ui.label(RichText::new("How the books are held").color(t.ink));
            let keeps = [
                (FolderKeep::Copy, "Copy into the library"),
                (FolderKeep::InPlace, "Read in place"),
                (FolderKeep::Watch, "Read in place, and watch for new books"),
            ];
            choices(ui, &mut plan.keep, &keeps, t);
            let held = match plan.keep {
                FolderKeep::Copy => {
                    "Books are copied into Mareader's own folder, so they keep \
                     working if this folder moves or is deleted."
                }
                FolderKeep::InPlace => {
                    "Books stay where they are; the library remembers where. \
                     Books added later wait for a rescan from the folder's menu."
                }
                FolderKeep::Watch => {
                    "Books stay where they are, and the folder is checked for \
                     new ones when Mareader opens or you come back to it."
                }
            };
            note(ui, held, t);
            ui.add_space(4.0);
            ui.label(RichText::new("Folder structure").color(t.ink));
            let shapes = [
                (Shelving::Each, "A shelf for each folder"),
                (Shelving::One, "One shelf for everything"),
                (Shelving::None, "No shelf"),
            ];
            choices(ui, &mut plan.walk.shelving, &shapes, t);
            let shaped = match plan.walk.shelving {
                Shelving::Each => {
                    "Every subfolder gets its own shelf, inside one named after the folder."
                }
                Shelving::One => "One shelf, named after the folder, holds all its books.",
                Shelving::None => "Books join the library without a shelf.",
            };
            let shaped = match (inside, plan.walk.shelving) {
                (Some(shelf), Shelving::Each | Shelving::One) => {
                    format!("{shaped} It's made inside “{shelf}”.")
                }
                (Some(shelf), Shelving::None) => format!("Books go on “{shelf}”."),
                (None, _) => shaped.to_owned(),
            };
            note(ui, &shaped, t);
        });
    ui.data_mut(|d| d.insert_temp(key, plan.clone()));
    ui.add_space(8.0);
    buttons(ui, |ui| {
        let ready = plan.walk.counts_any();
        let add = ui.add_enabled_ui(ready, |ui| primary(ui, "Add", t.accent, t));
        if add.inner.clicked() {
            let roots = roots.to_vec();
            act = Some(Action::AddFolders {
                roots,
                plan: plan.clone(),
            });
        }
        if ui.button("Cancel").clicked() {
            act = Some(Action::CloseSheet);
        }
    });
    act
}

/// A small heading over a group of the sheet's controls.
fn section(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).font(theme::medium(11.5)).color(t.muted));
}

/// The folders being added, and a way to pick another; true if asked.
fn folder_row(ui: &mut Ui, roots: &[PathBuf], t: &Tokens) -> bool {
    let frame = Frame::new()
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius)
        .inner_margin(Margin::symmetric(10, 6));
    let inner = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(Icon::Folder.image(16.0).tint(t.muted));
            let change = ui
                .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let change = ui.button("Change…").on_hover_text("Choose another folder");
                    ui.vertical(|ui| {
                        for root in roots {
                            let path = RichText::new(root.display().to_string()).color(t.ink);
                            let full = root.display().to_string();
                            ui.add(egui::Label::new(path).truncate())
                                .on_hover_text(full);
                        }
                    });
                    change
                })
                .inner;
            change.clicked()
        })
        .inner
    });
    inner.inner
}

/// Radio rows, one per choice.
fn choices<T: Copy + PartialEq>(ui: &mut Ui, value: &mut T, all: &[(T, &str)], t: &Tokens) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for &(choice, label) in all {
            ui.radio_value(value, choice, RichText::new(label).color(t.ink));
        }
    });
}

fn note(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.label(RichText::new(text).size(12.0).color(t.muted));
}

fn title(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.label(RichText::new(text).font(theme::medium(16.0)).color(t.ink));
}

/// A right-aligned row: the first button added sits rightmost.
fn buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
}

fn primary(ui: &mut Ui, label: &str, fill: Color32, t: &Tokens) -> egui::Response {
    let text = RichText::new(label).color(Color32::WHITE);
    ui.add(Button::new(text).fill(fill).corner_radius(t.radius))
}
