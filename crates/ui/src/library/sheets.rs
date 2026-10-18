//! Sheets over the library, and the questions waiting behind them.

use std::path::Path;

use egui::{Button, Color32, Context, Frame, Id, Margin, Modal, RichText, Stroke, Ui};
use mareader_library::{
    Book, BookId, Format, Library, MIN_SIZE_MAX, Mode, Options, Origin, Receipt, Shelf, ShelfId,
    SourceId,
};

use super::asks::{self, Question};
use super::books::counted;
use super::{Action, BookAction, Finding, LibraryState, Pick, Sheet, ShelfAction, Step};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::{self, Tokens};

const WIDTH: f32 = 420.0;

/// The open sheet, else the next question, drawn over everything.
pub fn show(ctx: &Context, library: &LibraryState, out: &mut Commands) {
    let sheet = library.sheet();
    let asks: Vec<&Question> = library.questions().collect();
    if sheet.is_none() && asks.is_empty() {
        return;
    }
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
            let Some(sheet) = sheet else {
                act = asks::show(ui, model, &asks, &t);
                return;
            };
            act = match sheet {
                Sheet::Find(finding) => match model.book(finding.book) {
                    Some(book) => find(ui, library, book, finding, &t),
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
                Sheet::Import(root) => import(ui, library, root, &t),
                Sheet::Moved { folder, book } => match model.book(*book) {
                    Some(book) => moved(ui, model, *folder, book, &t),
                    None => Some(Action::CloseSheet),
                },
            };
        });
    let working = matches!(sheet, Some(Sheet::Find(f)) if matches!(f.step, Step::Working(_)));
    if modal.should_close() && !working {
        act = Some(match sheet {
            Some(_) => Action::CloseSheet,
            None => Action::CancelAsks,
        });
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
    let first = book.locations.first();
    let last = first.map(|l| &l.path);
    let folder = first.and_then(|l| match l.origin {
        Origin::Source(id) => Some(id),
        Origin::Store | Origin::Opened => None,
    });
    let offline = folder.filter(|id| library.offline().contains(id));
    let offline = offline.and_then(|id| library.model().folder(id));
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
    match receipt.copies {
        0 => {}
        1 => lines.push("The copy Mareader keeps is deleted.".to_owned()),
        c => lines.push(format!("The {c} copies Mareader keeps are deleted.")),
    }
    if receipt.hidden > 0 {
        lines.push(
            "Some of these came from a watched folder. Removing keeps them out of future \
             auto-imports — the folder's Add menu can give them back."
                .into(),
        );
    }
    let elsewhere = receipt.hidden == 0 && receipt.copies == 0;
    if n > 0 && elsewhere {
        lines.push("Files stay where they are; open one again to bring it back.".into());
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
    // Off each time the sheet opens: forgetting is never a habit.
    let key = Id::new("library-remove-forget");
    let mut forget = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    if receipt.has_data() {
        ui.add_space(4.0);
        ui.checkbox(&mut forget, "Delete the highlights and reading position");
        let said = match forget {
            true => "They go with the book: importing the file again lands it as a new book.",
            false => {
                "Kept: the marks, the place and any name you gave a book come back if the \
                 file is imported again."
            }
        };
        ui.label(RichText::new(said).size(12.0).color(t.muted));
    }
    ui.add_space(8.0);
    let label = match (n, shelves.len()) {
        (0 | 1, 0) | (0, 1) => "Remove".to_owned(),
        (n, 0) => format!("Remove {n} books"),
        (0, s) => format!("Remove {s} shelves"),
        (n, s) => format!(
            "Remove {} and {}",
            counted(n, "book", "books"),
            counted(s, "shelf", "shelves")
        ),
    };
    let mut act = None;
    buttons(ui, |ui| {
        if primary(ui, &label, t.danger, t).clicked() {
            let (books, shelves) = (books.to_vec(), shelves.to_vec());
            act = Some(Action::Book(BookAction::Remove {
                books,
                shelves,
                forget,
            }));
        }
        let cancel = ui.button("Cancel").on_hover_text("Keep these books");
        if cancel.clicked() {
            act = Some(Action::CloseSheet);
        }
    });
    match act {
        Some(_) => ui.data_mut(|d| {
            d.remove::<bool>(key);
        }),
        None => ui.data_mut(|d| {
            d.insert_temp(key, forget);
        }),
    }
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
    crate::widgets::name(&resp, "Title");
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

/// The one import sheet: which files count, how they're held, shelved.
fn import(ui: &mut Ui, library: &LibraryState, root: &Path, t: &Tokens) -> Option<Action> {
    let model = library.model();
    title(ui, "Import a folder", t);
    let key = Id::new(("import-folder", root));
    // The last answers start it; a tree already read here seeds its own.
    let mut opts = ui.data_mut(|d| {
        d.get_temp_mut_or_insert_with(key, || seed(model, root, library.last_options()))
            .clone()
    });
    let inside = model
        .folders()
        .find(|f| f.mode().in_place() && root.starts_with(&f.root) && root != f.root);
    let mut act = None;
    egui::ScrollArea::vertical()
        .max_height(ui.ctx().content_rect().height() * 0.7)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            section(ui, "Folder", t);
            if folder_row(ui, root, t) {
                act = Some(Action::PickFolder);
            }
            section(ui, "Formats", t);
            formats(ui, &mut opts);
            section(ui, "File size larger than", t);
            min_size(ui, &mut opts);
            section(ui, "Books", t);
            ui.label(RichText::new("How the books are held").color(t.ink));
            let mut mode = opts.mode();
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for choice in Mode::ALL {
                    ui.radio_value(
                        &mut mode,
                        choice,
                        RichText::new(choice.label()).color(t.ink),
                    );
                }
            });
            opts.set_mode(mode);
            let held = match (mode, inside.is_some()) {
                (Mode::Copy, _) => {
                    "Books are copied into the app's own files, so they keep working even if \
                     the folder moves or is deleted."
                }
                (Mode::InPlace, _) => {
                    "Books stay where they are — the library just remembers where they live. \
                     Books added to the folder later are not picked up."
                }
                (Mode::Watched, false) => {
                    "Books stay where they are, and the folder is checked for new ones when \
                     the app opens or you come back to it."
                }
                (Mode::Watched, true) => {
                    "Books stay where they are, and this subfolder is checked for new ones. \
                     The rest of the tree keeps its own answer."
                }
            };
            note(ui, held, t);
            ui.add_space(4.0);
            ui.label(RichText::new("Folder structure").color(t.ink));
            let shapes = [
                (true, "A shelf for each folder"),
                (false, "One shelf for everything"),
            ];
            choices(ui, &mut opts.groups, &shapes, t);
            let shaped = match inside {
                Some(_) => {
                    "This answer stands for this folder and the ones under it — the rest of \
                     the tree keeps its own."
                }
                None => {
                    "A shelf for each folder gives every subfolder its own shelf; one shelf \
                     keeps the whole import together."
                }
            };
            note(ui, shaped, t);
        });
    ui.add_space(8.0);
    buttons(ui, |ui| {
        let go = primary(ui, "Import", t.accent, t).on_hover_text("Import this folder");
        if go.clicked() {
            let root = root.to_path_buf();
            let opts = opts.clone();
            act = Some(Action::ImportFolder { root, opts });
        }
        let cancel = ui.button("Cancel").on_hover_text("Close without importing");
        if cancel.clicked() {
            act = Some(Action::CloseSheet);
        }
    });
    match act {
        Some(Action::PickFolder) | None => ui.data_mut(|d| {
            d.insert_temp(key, opts);
        }),
        Some(_) => ui.data_mut(|d| {
            d.remove::<Options>(key);
        }),
    }
    act
}

/// The sheet's start: `last`, with the watch and shape of a tree here.
fn seed(model: &Library, root: &Path, last: Options) -> Options {
    let standing = model.folder_at(root, true).and_then(|id| model.folder(id));
    match standing.filter(|f| f.mode().in_place()) {
        Some(tree) => Options {
            in_place: true,
            watch: tree.opts.watch,
            groups: tree.opts.groups,
            ..last
        },
        None => last,
    }
}

/// Format chips, and whether the chosen ones are counted or left out.
fn formats(ui: &mut Ui, opts: &mut Options) {
    ui.horizontal_wrapped(|ui| {
        for format in Format::ALL {
            let on = opts.formats.contains(&format);
            if ui.add(Button::selectable(on, format.label())).clicked() {
                match on {
                    true => opts.formats.remove(&format),
                    false => opts.formats.insert(format),
                };
            }
        }
    });
    ui.horizontal(|ui| {
        for (include, label) in [(true, "Include selected"), (false, "Exclude selected")] {
            if ui
                .add(Button::selectable(opts.include == include, label))
                .clicked()
            {
                opts.include = include;
            }
        }
    });
}

/// The size floor, stepped: files must be larger to count.
fn min_size(ui: &mut Ui, opts: &mut Options) {
    ui.horizontal(|ui| {
        let less = ui.add_enabled_ui(opts.min_size > 0, |ui| {
            crate::widgets::icon_button(ui, Icon::Minus, "Smaller files count too", false)
        });
        if less.inner.clicked() {
            opts.step_min_size(-1);
        }
        ui.label(opts.min_size_label());
        let more = ui.add_enabled_ui(opts.min_size < MIN_SIZE_MAX, |ui| {
            crate::widgets::icon_button(ui, Icon::Plus, "Only larger files", false)
        });
        if more.inner.clicked() {
            opts.step_min_size(1);
        }
    });
}

/// A book that left its folder's shelves: shown here too, or gone to.
fn moved(
    ui: &mut Ui,
    model: &Library,
    folder: SourceId,
    book: &Book,
    t: &Tokens,
) -> Option<Action> {
    title(
        ui,
        &format!("“{}” is on another shelf.", book.title.value),
        t,
    );
    let home = model.shelves_of(book.id).into_iter().next();
    let home = home
        .and_then(|s| model.shelf(s))
        .map(|s| s.name.value.clone());
    let go = match home {
        Some(name) => format!("Show it in {name}"),
        None => "Show it in Home".into(),
    };
    ui.add_space(8.0);
    let mut act = None;
    buttons(ui, |ui| {
        let tip = "Closes this menu and takes you to it";
        if primary(ui, &go, t.accent, t).on_hover_text(tip).clicked() {
            act = Some(Action::Reveal(Pick::Book(book.id)));
        }
        let tip = "One book, two shelves — nothing is copied";
        if ui.button("Also show it here").on_hover_text(tip).clicked() {
            let book = book.id;
            act = Some(Action::ShowMoved { folder, book });
        }
        if ui.button("Back").clicked() {
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

/// The folder being imported, and a way to pick another; true if asked.
fn folder_row(ui: &mut Ui, root: &Path, t: &Tokens) -> bool {
    let frame = Frame::new()
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius)
        .inner_margin(Margin::symmetric(10, 6));
    let inner = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(Icon::Folder.image(16.0).tint(t.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let change = ui.button("Change…").on_hover_text("Choose another folder");
                let full = root.display().to_string();
                let path = RichText::new(&full).color(t.ink);
                ui.add(egui::Label::new(path).truncate())
                    .on_hover_text(full);
                change.clicked()
            })
            .inner
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
