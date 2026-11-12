//! Book work from menus, the selection bar and drops: removal,
//! twins and imports.

use mareader_library::{LinkId, Move};

use super::{LibraryState, Pick, Sheet, wall_ms};
use crate::toast::Toast;
use crate::ui_command::{Commands, Sent, UiCommand};

pub use mareader_engine::library::BookAction;

impl LibraryState {
    pub(super) fn book_act(&mut self, action: BookAction, out: &mut Commands) {
        let now = wall_ms();
        match action {
            BookAction::AskRemove {
                books,
                shelves,
                links,
            } => {
                let asked = Sheet::Removal {
                    books,
                    shelves,
                    links,
                };
                self.sheet = Some(asked);
            }
            BookAction::Remove {
                books,
                shelves,
                links,
                cascade,
                forget,
            } => {
                self.remove_links(&links, books.is_empty() && shelves.is_empty(), out);
                self.remove_entries((&books, &shelves), cascade, forget, out);
            }
            BookAction::Twin(id) => {
                let Some(twin) = self.model.twin(id, now) else {
                    return;
                };
                let title = |id| self.model.book(id).map(|b| b.title.value.clone());
                let (made, from) = (title(twin), title(id));
                let note = format!(
                    "“{}” reads separately from “{}”.",
                    made.unwrap_or_default(),
                    from.unwrap_or_default()
                );
                out.push(Sent::Ui(UiCommand::Notify(Toast::info(note))));
            }
            BookAction::Import { paths, to } => self.import_files(paths, to, out),
            BookAction::ShelveTogether(picks) => {
                let picks = self.unheld_picks(picks, out);
                if !picks.is_empty() {
                    self.shelve_together(&picks, out);
                }
            }
            BookAction::Duplicate(picks) => self.duplicate(picks, out),
            // Dialogs and the file manager are the app's.
            BookAction::Reveal(_) | BookAction::PickImport => {}
        }
    }

    /// Links go at once: what they point at stays. `alone` tells of it.
    fn remove_links(&mut self, links: &[LinkId], alone: bool, out: &mut Commands) {
        let first = links.first().and_then(|l| self.model.link(*l));
        let first = first.map(|l| l.name.value.clone()).unwrap_or_default();
        let gone = self.model.remove_links(links, wall_ms());
        if !alone || gone == 0 {
            return;
        }
        self.sheet = None;
        self.picks.apply(super::PickAction::Clear);
        let note = match gone {
            1 => format!("Removed the link “{first}”."),
            n => format!("Removed {n} links."),
        };
        out.push(Sent::Ui(UiCommand::Notify(Toast::info(note))));
    }

    /// A new shelf where the reader is, the picks added to it.
    fn shelve_together(&mut self, picks: &[Pick], out: &mut Commands) {
        let name = super::shelving::NEW_SHELF;
        let Ok(id) = self
            .state
            .model
            .create_shelf(name, self.state.at, wall_ms())
        else {
            return;
        };
        let mv = Move {
            from: self.at,
            to: Some(id),
            keep: true,
            ..Move::of(picks)
        };
        self.carry(mv, out);
    }
}

pub use mareader_engine::library::counted;
