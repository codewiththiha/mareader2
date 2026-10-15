//! Sheets over the library: finding a lost book, removing a folder.

use egui::{Button, Color32, Context, Frame, Id, Margin, Modal, RichText, Stroke, Ui};
use mareader_library::{Book, Library, Source};

use super::{Action, Finding, LibraryState, Sheet, Step};
use crate::command::{Command, Commands};
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
