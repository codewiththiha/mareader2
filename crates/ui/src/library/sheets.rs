//! Sheets over the library, and the questions waiting behind them.

use std::path::Path;

use egui::{Button, Color32, Context, Frame, Id, Margin, Modal, RichText, Stroke, Ui};
use mareader_library::{
    Book, BookId, FolderId, Format, Library, MIN_SIZE_MAX, Mode, Options, Origin, Shelf, ShelfId,
    human_size,
};

use super::asks::{self, Question};
use super::books::counted;
use super::{Action, BookAction, Finding, LibraryState, Named, Pick, Sheet, Step};
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
                Sheet::Removal { books, shelves } => removal(ui, model, (books, shelves), &t),
                Sheet::Rename(named) => rename(ui, library, *named, &t),
                Sheet::Copy(work) => super::copying::show(ui, model, work, &t),
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
        Origin::Folder(id) => Some(id),
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

/// What removing books and shelves takes, itemised before it's done.
fn removal(
    ui: &mut Ui,
    model: &Library,
    (books, shelves): (&[BookId], &[ShelfId]),
    t: &Tokens,
) -> Option<Action> {
    // Both switches start off each time: forgetting is never a habit.
    let (cascade_key, forget_key) = (
        Id::new("library-remove-cascade"),
        Id::new("library-remove-forget"),
    );
    let mut cascade = ui
        .data(|d| d.get_temp::<bool>(cascade_key))
        .unwrap_or(false);
    let mut forget = ui.data(|d| d.get_temp::<bool>(forget_key)).unwrap_or(false);
    let going = model.removing(books, shelves, cascade);
    let receipt = model.receipt(&going.books);
    let asked: Vec<&Shelf> = shelves.iter().filter_map(|s| model.shelf(*s)).collect();
    let n = going.books.len();
    let many = n + asked.len() != 1;
    let heading = match (asked.as_slice(), going.books.as_slice()) {
        ([only], _) => only.name.value.clone(),
        ([], [one]) => model
            .book(*one)
            .map(|b| b.title.value.clone())
            .unwrap_or_default(),
        ([], _) => counted(n, "book", "books"),
        (_, _) => counted(asked.len(), "shelf", "shelves"),
    };
    title(ui, &heading, t);
    let subtitle = match n {
        0 => {
            let kept: usize = going.shelves.iter().map(|s| model.counts(*s).0).sum();
            match kept {
                0 => "Nothing else goes with them".to_owned(),
                k => format!("{} stay in the library", counted(k, "book", "books")),
            }
        }
        _ => {
            let kinds: Vec<&str> = receipt.formats.iter().map(|f| f.label()).collect();
            format!("{} · {}", kinds.join(" · "), human_size(receipt.size))
        }
    };
    ui.label(RichText::new(subtitle).size(12.5).color(t.muted));
    let mut rows: Vec<(&str, String)> = Vec::new();
    if many && n > 0 {
        rows.push(("Books", counted(n, "book", "books")));
    }
    if let Some(at) = receipt.position.filter(|_| !many) {
        rows.push((
            "Reading position",
            format!("Page {} of {}", at.page + 1, at.pages),
        ));
    }
    if receipt.marks > 0 {
        rows.push(("Highlights", counted(receipt.marks, "mark", "marks")));
    }
    if receipt.copies > 0 {
        let label = match receipt.copies {
            1 => "The app's own copy",
            _ => "The app's own copies",
        };
        rows.push((label, human_size(receipt.copies_size)));
    }
    if !receipt.filed.is_empty() {
        rows.push(("Filed on", receipt.filed.join(", ")));
    }
    for s in going.shelves.iter().filter_map(|s| model.shelf(*s)) {
        let (on, inside) = model.counts(s.id);
        let mut detail = match (on, cascade) {
            (0, _) => "empty".to_owned(),
            (b, true) => format!("{} go with it", counted(b, "book", "books")),
            (b, false) => counted(b, "book", "books"),
        };
        if !cascade && inside > 0 {
            detail.push_str(&format!(
                " · {} move up",
                counted(inside, "shelf", "shelves")
            ));
        }
        rows.push(("Shelf taken apart", format!("{} — {detail}", s.name.value)));
    }
    receipt_rows(ui, &rows, t);
    if going.inside_books + going.inside_shelves > 0 {
        ui.checkbox(&mut cascade, "Remove everything inside");
        let note = match (cascade, going.inside_books, going.inside_shelves) {
            (false, ..) => "Off: the books inside stay in the library and the shelves inside \
                            move up a level."
                .to_owned(),
            (true, 0, s) => format!(
                "{} inside are taken apart with it, instead of moving up a level.",
                counted(s, "shelf", "shelves")
            ),
            (true, b, 0) => format!(
                "{} inside go with the shelf, and are itemised above.",
                counted(b, "book", "books")
            ),
            (true, b, s) => format!(
                "{} inside go with the shelf and {} inside are taken apart too.",
                counted(b, "book", "books"),
                counted(s, "shelf", "shelves")
            ),
        };
        ui.label(RichText::new(note).size(12.0).color(t.muted));
    }
    if !going.copies.is_empty() {
        let note = "A shelf here came from a watched folder. Removing it asks what the books it \
                    reads in place become: copies of your own, or books the folder makes again \
                    on its next import.";
        ui.label(RichText::new(note).color(t.ink));
    }
    if receipt.hidden > 0 {
        let note = "Some of these came from a watched folder. Removing keeps them out of future \
                    auto-imports — the folder's Add menu can give them back.";
        ui.label(RichText::new(note).color(t.ink));
    }
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
    let label = match (n, asked.len()) {
        (1, 0) => "Remove".to_owned(),
        (0, 1) if cascade && going.inside_shelves > 0 => {
            "Remove the shelf and the ones inside".to_owned()
        }
        (0, 1) => "Remove the shelf".to_owned(),
        (0, s) if cascade && going.inside_shelves > 0 => {
            format!("Remove {s} shelves and the ones inside")
        }
        (0, s) => format!("Remove {s} shelves"),
        (b, 0) => format!("Remove {b} books"),
        (b, s) => format!(
            "Remove {} and {}",
            counted(b, "book", "books"),
            counted(s, "shelf", "shelves")
        ),
    };
    let mut act = None;
    buttons(ui, |ui| {
        if primary(ui, &label, t.danger, t).clicked() {
            act = Some(Action::Book(BookAction::Remove {
                books: books.to_vec(),
                shelves: shelves.to_vec(),
                cascade,
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
            d.remove::<bool>(cascade_key);
            d.remove::<bool>(forget_key);
        }),
        None => ui.data_mut(|d| {
            d.insert_temp(cascade_key, cascade);
            d.insert_temp(forget_key, forget);
        }),
    }
    act
}

/// A receipt: what goes, a label and its value per line.
fn receipt_rows(ui: &mut Ui, rows: &[(&str, String)], t: &Tokens) {
    if rows.is_empty() {
        return;
    }
    Frame::new()
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            egui::Grid::new("library-receipt")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for (label, value) in rows {
                        ui.label(RichText::new(*label).color(t.muted));
                        ui.add(egui::Label::new(RichText::new(value).color(t.ink)).wrap());
                        ui.end_row();
                    }
                });
        });
}

/// The one rename sheet; Enter gives the name, Escape keeps the old.
fn rename(ui: &mut Ui, library: &LibraryState, named: Named, t: &Tokens) -> Option<Action> {
    let Some(name) = library.name_of(named) else {
        return Some(Action::CloseSheet);
    };
    let model = library.model();
    let (heading, note) = match named {
        Named::Book(_) => (
            "Rename book",
            "Only the name the library shows — the file on disk keeps its own.",
        ),
        Named::Shelf(id)
            if model
                .shelf(id)
                .is_some_and(|s| s.kind.value.rung().is_some()) =>
        {
            (
                "Rename shelf",
                "Only the name the library shows — the folder on disk keeps its own.",
            )
        }
        Named::Shelf(_) => ("Rename shelf", "Only the name the library shows."),
        Named::Link(_) => (
            "Rename link",
            "The name the pointer wears — the book it points at keeps its own.",
        ),
    };
    title(ui, heading, t);
    let key = Id::new(("library-rename", named));
    let first = ui.data(|d| d.get_temp::<String>(key).is_none());
    let mut draft = ui.data_mut(|d| d.get_temp_mut_or_insert_with(key, || name.clone()).clone());
    let edit = egui::TextEdit::singleline(&mut draft)
        .id(key)
        .desired_width(f32::INFINITY)
        .char_limit(mareader_library::NAME_MAX);
    let resp = ui.add(edit);
    crate::widgets::name(&resp, "New name");
    if first {
        super::crumbs::select_all(&resp, key, &draft);
    }
    ui.data_mut(|d| d.insert_temp(key, draft.clone()));
    ui.label(RichText::new(note).size(12.0).color(t.muted));
    let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.add_space(8.0);
    let mut act = None;
    buttons(ui, |ui| {
        let blank = draft.trim().is_empty();
        let save = ui.add_enabled_ui(!blank, |ui| primary(ui, "Rename", t.accent, t));
        let save = save.inner.on_hover_text("Give it this name");
        if save.clicked() || (entered && !blank) {
            act = Some(Action::Rename(named, draft.clone()));
        }
        if ui
            .button("Cancel")
            .on_hover_text("Keep the name it has")
            .clicked()
        {
            act = Some(Action::CloseSheet);
        }
    });
    // The next rename starts from the name it has then.
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
    folder: FolderId,
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

pub(super) fn title(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.label(RichText::new(text).font(theme::medium(16.0)).color(t.ink));
}

/// A right-aligned row: the first button added sits rightmost.
pub(super) fn buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
}

pub(super) fn primary(ui: &mut Ui, label: &str, fill: Color32, t: &Tokens) -> egui::Response {
    let text = RichText::new(label).color(Color32::WHITE);
    ui.add(Button::new(text).fill(fill).corner_radius(t.radius))
}
