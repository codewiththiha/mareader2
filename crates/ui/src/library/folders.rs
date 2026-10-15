//! The Folders popover and the scope chips over the books.

use std::collections::BTreeSet;

use egui::{Button, Id, Popup, PopupCloseBehavior, RectAlign, RichText, Stroke, Ui};
use mareader_library::{Book, Format, Library, Origin, Source, SourceEdit, SourceId};

use super::{Action, Scope};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::{self, Tokens};

pub const FOLDERS_POPOVER: &str = "library-folders";
const WIDTH: f32 = 360.0;

/// What the popover and chips read from the library.
pub struct Shown<'a> {
    pub model: &'a Library,
    pub offline: &'a BTreeSet<SourceId>,
    pub scanning: bool,
    pub scope: Scope,
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

/// The header's Folders button and its popover.
pub fn button(ui: &mut Ui, shown: &Shown<'_>, t: &Tokens, out: &mut Commands) {
    let id = Id::new(FOLDERS_POPOVER);
    let label = match shown.model.sources().count() {
        0 => "Folders".to_owned(),
        n => format!("Folders · {n}"),
    };
    let button = Button::image_and_text(Icon::Folders.image(16.0), label)
        .image_tint_follows_text_color(true)
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius)
        .min_size(egui::vec2(96.0, 32.0));
    let r = ui.add(button);
    Popup::from_toggle_button_response(&r)
        .id(id)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_END)
        .gap(6.0)
        .width(WIDTH)
        .show(|ui| contents(ui, shown, t, out));
}

fn contents(ui: &mut Ui, shown: &Shown<'_>, t: &Tokens, out: &mut Commands) {
    ui.set_width(WIDTH);
    ui.spacing_mut().item_spacing.y = 6.0;
    ui.horizontal(|ui| {
        let title = RichText::new("Folders").font(theme::medium(14.0));
        ui.label(title.color(t.ink));
        if shown.scanning {
            ui.spinner();
        }
    });
    let mut sources: Vec<&Source> = shown.model.sources().collect();
    sources.sort_by_key(|s| s.name().to_lowercase());
    if sources.is_empty() {
        let hint = "Add a folder to read its books where they are. \
                    Books put there later appear when you come back.";
        ui.label(RichText::new(hint).size(12.5).color(t.muted));
    }
    for source in sources {
        ui.separator();
        folder(ui, shown, source, t, out);
    }
    ui.separator();
    ui.horizontal(|ui| {
        let add = Button::image_and_text(Icon::FolderPlus.image(16.0), "Add folder…")
            .image_tint_follows_text_color(true);
        if ui.add(add).clicked() {
            out.push(Command::Library(Action::PickFolder));
        }
        let any = shown.model.sources().next().is_some();
        let all = Button::image_and_text(Icon::Rescan.image(16.0), "Rescan all")
            .image_tint_follows_text_color(true);
        if ui.add_enabled(any && !shown.scanning, all).clicked() {
            out.push(Command::Library(Action::Rescan(None)));
        }
    });
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
}

/// The formats, size floor and watching that pick a folder's books.
fn counts(ui: &mut Ui, source: &Source, out: &mut Commands) {
    let mut edit = SourceEdit {
        formats: source.formats.clone(),
        min_size: source.min_size,
        watch: source.watch,
    };
    ui.horizontal(|ui| {
        for format in Format::ALL {
            let on = edit.formats.contains(&format);
            // A folder reads at least one format.
            let last = on && edit.formats.len() == 1;
            let chip = Button::selectable(on, format.label());
            if ui.add_enabled(!last, chip).clicked() {
                match on {
                    true => edit.formats.remove(&format),
                    false => edit.formats.insert(format),
                };
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Skip files under");
        let mut kb = edit.min_size / 1_024;
        let drag = egui::DragValue::new(&mut kb)
            .range(0..=100_000)
            .suffix(" KB");
        if ui.add(drag).changed() {
            edit.min_size = kb * 1_024;
        }
    });
    ui.checkbox(
        &mut edit.watch,
        "Look for new books when Mareader comes back",
    );
    let changed = edit.formats != source.formats
        || edit.min_size != source.min_size
        || edit.watch != source.watch;
    if changed {
        out.push(Command::Library(Action::EditFolder(source.id, edit)));
    }
}

/// All, Missing and each folder, when there's more than one place.
pub fn chips(ui: &mut Ui, shown: &Shown<'_>, out: &mut Commands) {
    let missing = shown.model.missing();
    let mut sources: Vec<&Source> = shown.model.sources().collect();
    if sources.is_empty() && missing == 0 && shown.scope == Scope::All {
        return;
    }
    sources.sort_by_key(|s| s.name().to_lowercase());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let mut chip = |ui: &mut Ui, scope: Scope, label: String| {
            let on = shown.scope == scope;
            if ui.add(Button::selectable(on, label)).clicked() && !on {
                out.push(Command::Library(Action::Scope(scope)));
            }
        };
        chip(ui, Scope::All, "All books".into());
        if missing > 0 || shown.scope == Scope::Missing {
            chip(ui, Scope::Missing, format!("Missing · {missing}"));
        }
        for source in sources {
            let n = shown.model.books_in(source.id).count();
            chip(
                ui,
                Scope::Folder(source.id),
                format!("{} · {n}", source.name()),
            );
        }
    });
    ui.add_space(10.0);
}
