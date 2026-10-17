//! The scope chips over the books, and each folder's own menu.

use std::collections::BTreeSet;

use egui::{Button, Id, IntoAtoms, Popup, PopupCloseBehavior, RectAlign, RichText, Ui};
use mareader_library::{Book, BookId, Format, Library, Origin, Source, SourceEdit, SourceId};

use super::{Action, BookAction, Scope};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::{self, Tokens};

const WIDTH: f32 = 340.0;

/// What the chips and folder menus read from the library.
pub struct Shown<'a> {
    pub model: &'a Library,
    pub offline: &'a BTreeSet<SourceId>,
    pub scanning: bool,
    pub scope: Scope,
    /// Inside the chosen folder, the subfolders walked into.
    pub trail: &'a [String],
}

/// Whether `book` belongs on the page under `scope`.
pub fn in_scope(book: &Book, scope: Scope) -> bool {
    match scope {
        Scope::All => true,
        Scope::Missing => book.missing(),
        Scope::Folder(id) => book
            .locations
            .iter()
            .any(|l| l.origin == Origin::Source(id)),
    }
}

/// A missing book whose folder's drive wasn't there at the last look.
pub fn offline(book: &Book, offline: &BTreeSet<SourceId>) -> bool {
    book.missing()
        && book.locations.iter().any(|l| match l.origin {
            Origin::Source(id) => offline.contains(&id),
            Origin::Opened | Origin::Store => false,
        })
}

/// The popup that manages one folder, opened from its chip.
pub fn menu_id(id: SourceId) -> Id {
    Id::new(("library-folder", id))
}

fn folder(ui: &mut Ui, shown: &Shown<'_>, source: &Source, t: &Tokens, out: &mut Commands) {
    let id = source.id;
    let gone = shown.offline.contains(&id);
    ui.horizontal(|ui| {
        let icon = if gone { Icon::Offline } else { Icon::Folder };
        ui.add(icon.image(18.0).tint(t.muted));
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let name = RichText::new(source.name()).font(theme::medium(13.0));
            ui.label(name.color(t.ink));
            let count = shown.model.books_in(id).count();
            let status = match (gone, count) {
                (true, _) => "Not connected".to_owned(),
                (false, 1) => "1 book".to_owned(),
                (false, n) => format!("{n} books"),
            };
            ui.label(RichText::new(status).size(12.0).color(t.muted));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let remove = crate::widgets::icon_button(ui, Icon::Remove, "Remove folder", false);
            if remove.clicked() {
                out.push(Command::Library(Action::AskRemoveFolder(id)));
            }
            let rescan = crate::widgets::icon_button(ui, Icon::Rescan, "Rescan", false);
            if rescan.clicked() {
                out.push(Command::Library(Action::Rescan(Some(id))));
            }
        });
    });
    let path = source.root.display().to_string();
    ui.add(egui::Label::new(RichText::new(path).size(11.5).color(t.muted)).truncate());
    egui::CollapsingHeader::new(RichText::new("What counts").size(12.5))
        .id_salt(("folder-counts", id))
        .show(ui, |ui| counts(ui, source, out));
    let hidden = shown.model.hidden_in(id);
    if !hidden.is_empty() {
        let label = format!("Removed · {}", hidden.len());
        egui::CollapsingHeader::new(RichText::new(label).size(12.5))
            .id_salt(("folder-removed", id))
            .show(ui, |ui| removed(ui, id, &hidden, t, out));
    }
}

/// Books removed from a folder, each with a way back.
fn removed(ui: &mut Ui, id: SourceId, hidden: &[&Book], t: &Tokens, out: &mut Commands) {
    let restore = |out: &mut Commands, books: Vec<BookId>| {
        out.push(Command::Library(Action::Book(BookAction::Restore(books))));
    };
    let note = "Their files are still here; restored, they come back as they were.";
    ui.label(RichText::new(note).size(11.5).color(t.muted));
    egui::ScrollArea::vertical()
        .id_salt(("folder-removed-list", id))
        .max_height(160.0)
        .show(ui, |ui| {
            for book in hidden {
                ui.horizontal(|ui| {
                    let title = RichText::new(&book.title.value).size(12.5).color(t.ink);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Restore").clicked() {
                            restore(out, vec![book.id]);
                        }
                        ui.add(egui::Label::new(title).truncate());
                    });
                });
            }
        });
    if hidden.len() > 1 && ui.button("Restore all").clicked() {
        restore(out, hidden.iter().map(|b| b.id).collect());
    }
}

/// The formats, size floor and watching that pick a folder's books.
fn counts(ui: &mut Ui, source: &Source, out: &mut Commands) {
    let mut edit = SourceEdit {
        formats: source.formats.clone(),
        exclude: source.exclude,
        min_size: source.min_size,
        watch: source.watch,
    };
    what_counts(ui, &mut edit.formats, &mut edit.exclude, &mut edit.min_size);
    ui.checkbox(
        &mut edit.watch,
        "Look for new books when Mareader comes back",
    );
    let changed = edit.formats != source.formats
        || edit.exclude != source.exclude
        || edit.min_size != source.min_size
        || edit.watch != source.watch;
    if changed {
        out.push(Command::Library(Action::EditFolder(source.id, edit)));
    }
}

/// The size floor's step and ceiling, in bytes.
const SIZE_STEP: u64 = 10 * 1_024;
const SIZE_MAX: u64 = 500 * 1_024;

/// Which of a folder's files are books: formats in or out, and a floor.
pub fn what_counts(
    ui: &mut Ui,
    formats: &mut BTreeSet<Format>,
    exclude: &mut bool,
    min_size: &mut u64,
) {
    // Nothing is offered that would leave no format counting.
    let any = |formats: &BTreeSet<Format>, exclude: bool| {
        Format::ALL.iter().any(|f| formats.contains(f) != exclude)
    };
    ui.horizontal(|ui| {
        for (out, label) in [(false, "Include selected"), (true, "Exclude selected")] {
            let chip = Button::selectable(*exclude == out, label);
            if ui.add_enabled(any(formats, out), chip).clicked() {
                *exclude = out;
            }
        }
    });
    ui.horizontal(|ui| {
        for format in Format::ALL {
            let on = formats.contains(&format);
            let mut next = formats.clone();
            match on {
                true => next.remove(&format),
                false => next.insert(format),
            };
            let chip = Button::selectable(on, format.label());
            if ui.add_enabled(any(&next, *exclude), chip).clicked() {
                *formats = next;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Skip files under");
        let less = ui.add_enabled_ui(*min_size > 0, |ui| {
            crate::widgets::icon_button(ui, Icon::Minus, "Smaller files count too", false)
        });
        if less.inner.clicked() {
            *min_size = min_size.saturating_sub(SIZE_STEP) / SIZE_STEP * SIZE_STEP;
        }
        ui.label(format!("{} KB", *min_size / 1_024));
        let more = ui.add_enabled_ui(*min_size < SIZE_MAX, |ui| {
            crate::widgets::icon_button(ui, Icon::Plus, "Only larger files", false)
        });
        if more.inner.clicked() {
            *min_size = ((*min_size / SIZE_STEP + 1) * SIZE_STEP).min(SIZE_MAX);
        }
    });
}

/// All, Missing and each folder, when there's more than one place.
pub fn chips(ui: &mut Ui, inset: f32, shown: &Shown<'_>, t: &Tokens, out: &mut Commands) {
    let missing = shown.model.missing();
    let mut sources: Vec<&Source> = shown.model.sources().collect();
    if sources.is_empty() && missing == 0 && shown.scope == Scope::All {
        return;
    }
    sources.sort_by_key(|s| s.name().to_lowercase());
    ui.horizontal_wrapped(|ui| {
        ui.add_space(inset);
        ui.spacing_mut().item_spacing.x = 6.0;
        let chip = |ui: &mut Ui, scope: Scope, label: egui::Atoms<'static>, out: &mut Commands| {
            let on = shown.scope == scope;
            let resp = ui.add(Button::selectable(on, label));
            if resp.clicked() && !on {
                out.push(Command::Library(Action::Scope(scope)));
            }
            resp
        };
        chip(ui, Scope::All, "All books".into_atoms(), out);
        if missing > 0 || shown.scope == Scope::Missing {
            let label = format!("Missing · {missing}");
            chip(ui, Scope::Missing, label.into_atoms(), out);
        }
        for source in sources {
            let n = shown.model.books_in(source.id).count();
            let scope = Scope::Folder(source.id);
            let on = shown.scope == scope;
            let label = format!("{} · {n}", source.name());
            // The chosen folder's chip says it has a menu, and opens it.
            let label = match on {
                true => (label, Icon::More.image(14.0).tint(t.muted)).into_atoms(),
                false => label.into_atoms(),
            };
            let resp = chip(ui, scope, label, out);
            let menu = Popup::from_response(&resp)
                .id(menu_id(source.id))
                .sense(crate::widgets::POPUP_SENSE)
                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                .align(RectAlign::BOTTOM_START)
                .gap(6.0)
                .width(WIDTH);
            let toggle = (on && resp.clicked()).then_some(egui::SetOpenCommand::Toggle);
            menu.open_memory(toggle).show(|ui| {
                ui.set_width(WIDTH);
                ui.spacing_mut().item_spacing.y = 6.0;
                folder(ui, shown, source, t, out);
            });
        }
    });
    if let Scope::Folder(id) = shown.scope {
        trail(ui, shown, id, t, out);
    }
    ui.add_space(10.0);
}

/// The chosen folder's trail and its subfolders, walked as in a file manager.
fn trail(ui: &mut Ui, shown: &Shown<'_>, id: SourceId, t: &Tokens, out: &mut Commands) {
    let subs = shown.model.subfolders(id, shown.trail);
    if shown.trail.is_empty() && subs.is_empty() {
        return;
    }
    let root = shown.model.source(id).map(Source::name).unwrap_or_default();
    let go = |trail: Vec<String>| Command::Library(Action::Trail(trail));
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let names = std::iter::once(root).chain(shown.trail.iter().cloned());
        for (depth, name) in names.enumerate() {
            if depth > 0 {
                ui.label(RichText::new("›").color(t.muted));
            }
            // The folder shown is plain text; those above it lead back up.
            if depth == shown.trail.len() {
                ui.label(RichText::new(name).font(theme::medium(t.text)).color(t.ink));
            } else if ui.add(Button::new(name).frame(false)).clicked() {
                out.push(go(shown.trail.iter().take(depth).cloned().collect()));
            }
        }
        ui.add_space(8.0);
        for (name, count) in subs {
            let label = (
                Icon::Folder.image(14.0).tint(t.muted),
                format!("{name} · {count}"),
            );
            if ui.add(Button::new(label.into_atoms())).clicked() {
                let mut next = shown.trail.to_vec();
                next.push(name);
                out.push(go(next));
            }
        }
    });
}
