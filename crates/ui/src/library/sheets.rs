//! Sheets over the library, and the questions waiting behind them.

use egui::{Button, Color32, Context, Frame, Id, Margin, Modal, RichText, Stroke, Ui};
use mareader_library::{Book, FolderId, Library, Origin, ShelfId};

use super::asks::{self, Question};
use super::{Action, Finding, LibraryState, Named, Pick, Sheet, Step};
use crate::command::{Command, Commands};
use crate::theme::{self, Tokens};

const WIDTH: f32 = 420.0;

/// The open sheet, else the next question, drawn over everything.
pub fn show(ctx: &Context, library: &LibraryState, out: &mut Commands) {
    let sheet = library.sheet();
    let asks: Vec<&Question> = library.questions().collect();
    // The AI's plan waits behind any other sheet, as questions do.
    let review = library.organizer().review().filter(|_| sheet.is_none());
    if sheet.is_none() && asks.is_empty() && review.is_none() {
        return;
    }
    let t = Tokens::get(ctx);
    let model = library.model();
    if let Some(Sheet::Removal {
        books,
        shelves,
        links,
    }) = sheet
    {
        let asked = (books.as_slice(), shelves.as_slice(), links.as_slice());
        let act = super::remove_alert::show(ctx, model, asked, &t);
        out.extend(act.map(Command::Library));
        return;
    }
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
                act = match review {
                    Some(plan) => super::review::show(ui, model, plan, &t),
                    None => asks::show(ui, model, &asks, &t),
                };
                return;
            };
            act = match sheet {
                Sheet::Find(finding) => match model.book(finding.book) {
                    Some(book) => find(ui, library, book, finding, &t),
                    None => Some(Action::CloseSheet),
                },
                // Drawn by its own alert, above.
                Sheet::Removal { .. } => None,
                Sheet::Rename(named) => rename(ui, library, *named, &t),
                Sheet::Copy(work) => super::copying::show(ui, model, work, &t),
                Sheet::Import(root) => super::import_sheet::show(ui, library, root, &t),
                Sheet::Moved { folder, book } => match model.book(*book) {
                    Some(book) => moved(ui, model, *folder, book, &t),
                    None => Some(Action::CloseSheet),
                },
                Sheet::Note { shelf, returned } => import_note(ui, model, *shelf, *returned, &t),
            };
        });
    let working = matches!(sheet, Some(Sheet::Find(f)) if matches!(f.step, Step::Working(_)));
    // The plan is only let go by its button, never a stray click.
    if modal.should_close() && !working && review.is_none() {
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
        let pick = "Pick the document this book reads from now";
        if choose.inner.on_hover_text(pick).clicked() {
            act = Some(Action::PickFile(id));
        }
        let search = ui.add_enabled(!working, Button::new("Search a folder…"));
        let name = &book.title.value;
        let walk = format!("Walk a folder you pick and open the first “{name}” inside it");
        if search.on_hover_text(walk).clicked() {
            act = Some(Action::PickSearchFolder(id));
        }
        if ui.button("Leave the book missing").clicked() {
            act = Some(Action::CloseSheet);
        }
    });
    act
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

/// What an import that found only what was there says.
fn import_note(
    ui: &mut Ui,
    model: &Library,
    shelf: ShelfId,
    returned: bool,
    t: &Tokens,
) -> Option<Action> {
    let Some(name) = model.shelf(shelf).map(|s| s.name.value.as_str()) else {
        return Some(Action::CloseSheet);
    };
    title(ui, name, t);
    let (said, sentence) = match returned {
        true => (
            "Back where its folder names",
            format!(
                "“{name}” went back inside the folder it belongs to, onto the shelf its \
                 directory names. Nothing was copied, and nothing on disk moved. It lights up \
                 where it stands now when you close this."
            ),
        ),
        false => (
            "Nothing new to import",
            "The import walked the folder again and found nothing new — every book is \
             already on the shelf. The shelf lights up when you close this."
                .to_owned(),
        ),
    };
    ui.label(RichText::new(said).size(12.0).color(t.muted));
    ui.label(RichText::new(sentence).size(12.0).color(t.ink));
    ui.add_space(8.0);
    let mut act = None;
    buttons(ui, |ui| {
        let show = primary(ui, "Show the shelf", t.accent, t);
        if show.on_hover_text("Close and light the shelf up").clicked() {
            act = Some(Action::CloseSheet);
        }
    });
    act
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
