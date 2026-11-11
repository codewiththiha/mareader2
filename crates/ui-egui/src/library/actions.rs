//! What the page and its sheets ask of the library, and who does it.

use super::{LibraryState, Pick, PickAction};
use crate::command::Commands;

pub use mareader_engine::library::{Action, Named, Scope, Sheet};

impl LibraryState {
    pub fn sheet(&self) -> Option<&Sheet> {
        self.folders.sheet.as_ref()
    }

    /// A sheet or a question is over the page, holding the keys.
    pub fn sheet_up(&self) -> bool {
        let review = self.organizer.review().is_some();
        self.folders.sheet.is_some() || !self.folders.asks.is_empty() || review
    }

    /// Does what the page asked; dialogs are the app's.
    pub fn act(&mut self, action: Action, now: f64, out: &mut Commands) {
        match action {
            Action::AskFolder(root) => self.ask_folder(root),
            Action::ImportFolder { root, opts } => self.import_folder(root, opts, now, out),
            Action::Rescan => self.rescan(now),
            Action::Answer(answer, all) => self.answer(answer, all, now, out),
            Action::CancelAsks => self.folders.asks.clear(),
            Action::Restore { folder, content } => self.restore(folder, content, out),
            Action::AskMoved { folder, book } => {
                self.folders.sheet = Some(Sheet::Moved { folder, book });
            }
            Action::ShowMoved { folder, book } => self.show_moved(folder, book, now),
            Action::Reveal(pick) => {
                self.close_sheet();
                self.reveal(pick, now);
            }
            Action::Scope(scope) => {
                self.scope = scope;
                self.picks.apply(PickAction::Clear);
            }
            Action::FindAgain(_)
            | Action::Relink { .. }
            | Action::UseAnyway(_)
            | Action::Seek { .. } => self.find_act(action, out),
            Action::CloseSheet => {
                // A note lights its shelf however it's closed.
                let lit = match &self.folders.sheet {
                    Some(Sheet::Note { shelf, .. }) => Some(*shelf),
                    _ => None,
                };
                self.close_sheet();
                if let Some(shelf) = lit {
                    self.reveal(Pick::Shelf(shelf), now);
                }
            }
            Action::AskRename(named) => {
                if self.name_of(named).is_some() {
                    self.folders.sheet = Some(Sheet::Rename(named));
                }
            }
            Action::Rename(named, name) => self.rename(named, &name),
            Action::Design(book, design) => {
                self.model.set_design(book, design, super::wall_ms());
            }
            Action::Shelf(action) => self.shelf_act(action, out),
            Action::Pick(action) => self.picks.apply(action),
            Action::Book(action) => self.book_act(action, out),
            Action::Search(action) => self.search_act(action, now),
            Action::ApplyOrganized => self.apply_organized(out),
            Action::DiscardOrganized | Action::StopOrganizing => self.organizer_end(),
            Action::UndoOrganized(done) => self.undo_organized(&done),
            // Dialogs need the app's files, and the AI its service.
            Action::Organize(_)
            | Action::OrganizeAnew(_)
            | Action::PickInto(_)
            | Action::ResumeOrganizing
            | Action::DiscardPaused
            | Action::RestorePaused
            | Action::ShowAdd
            | Action::PickFolder
            | Action::PickFrom(_)
            | Action::PickFile(_)
            | Action::PickSearchFolder(_) => {}
        }
    }
}

impl LibraryState {
    /// The name a book, shelf or link shows now; none once it's gone.
    pub fn name_of(&self, named: Named) -> Option<String> {
        let model = &self.model;
        match named {
            Named::Book(id) => model.book(id).map(|b| b.title.value.clone()),
            Named::Shelf(id) => model.shelf(id).map(|s| s.name.value.clone()),
            Named::Link(id) => model.link(id).map(|l| l.name.value.clone()),
        }
    }

    /// Gives it the typed name; a blank one keeps the old.
    fn rename(&mut self, named: Named, name: &str) {
        if matches!(self.folders.sheet, Some(Sheet::Rename(_))) {
            self.close_sheet();
        }
        self.renaming = None;
        let now = super::wall_ms();
        match named {
            Named::Book(id) => {
                self.model.rename_book(id, name, now);
            }
            Named::Shelf(id) => {
                self.model.rename_shelf(id, name, now).ok();
            }
            Named::Link(id) => {
                self.model.rename_link(id, name, now);
            }
        }
    }
}
