//! What the page asked for: one router for a menu, a key, a sheet or a drop.

use super::{Action, Out, Pick, PickAction, Sheet, State, wall_ms};

impl State {
    /// Does what the page asked; dialogs are the app's.
    pub fn act(&mut self, action: Action, now: f64, out: &mut Vec<Out>) {
        match action {
            Action::AskFolder(root) => self.ask_folder(root),
            Action::ImportFolder { root, opts } => self.import_folder(root, opts, now, out),
            Action::Rescan => self.rescan(now),
            Action::Answer(answer, all) => self.answer(answer, all, now, out),
            Action::CancelAsks => self.asks.clear(),
            Action::Restore { folder, content } => self.restore(folder, content, out),
            Action::AskMoved { folder, book } => {
                self.sheet = Some(Sheet::Moved { folder, book });
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
                let lit = match &self.sheet {
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
                    self.sheet = Some(Sheet::Rename(named));
                }
            }
            Action::Rename(named, name) => self.rename(named, &name),
            Action::Design(book, design) => self.model.set_design(book, design, wall_ms()),
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
