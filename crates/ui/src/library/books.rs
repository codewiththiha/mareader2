//! Book work from menus, the selection bar and drops: removal,
//! twins and imports.

use std::path::PathBuf;

use mareader_library::{BookId, Move, ShelfId};

use super::{LibraryState, Sheet, wall_ms};
use crate::command::{Command, Commands};
use crate::toast::Toast;

/// What menus, the selection bar and drops ask of books.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookAction {
    /// Asks first, telling what goes and what stays.
    AskRemove {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
    },
    /// `cascade` takes what's inside the shelves; `forget`, marks and places.
    Remove {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
        cascade: bool,
        forget: bool,
    },
    /// A separate book of the same file, with its own place.
    Twin(BookId),
    /// Shows its file in the system's file manager.
    Reveal(BookId),
    /// Shows the file dialog; the picks come back as `Import`.
    PickImport,
    /// Copies in files onto a shelf if `to`; a folder asks how.
    Import {
        paths: Vec<PathBuf>,
        to: Option<ShelfId>,
    },
    /// A new shelf where the reader is, holding these.
    ShelveTogether {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
    },
}

impl LibraryState {
    pub(super) fn book_act(&mut self, action: BookAction, out: &mut Commands) {
        let now = wall_ms();
        match action {
            BookAction::AskRemove { books, shelves } => {
                self.folders.sheet = Some(Sheet::Removal { books, shelves });
            }
            BookAction::Remove {
                books,
                shelves,
                cascade,
                forget,
            } => self.remove_entries((&books, &shelves), cascade, forget, out),
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
                out.push(Command::Notify(Toast::info(note)));
            }
            BookAction::Import { paths, to } => self.import_files(paths, to, out),
            BookAction::ShelveTogether { books, shelves } => {
                self.shelve_together(&books, &shelves, out);
            }
            // Dialogs and the file manager are the app's.
            BookAction::Reveal(_) | BookAction::PickImport => {}
        }
    }

    /// A new shelf where the reader is, the picks added to it.
    fn shelve_together(&mut self, books: &[BookId], shelves: &[ShelfId], out: &mut Commands) {
        let name = super::shelving::NEW_SHELF;
        let Ok(id) = self.model.create_shelf(name, self.at, wall_ms()) else {
            return;
        };
        let mv = Move {
            books: books.to_vec(),
            shelves: shelves.to_vec(),
            from: self.at,
            to: Some(id),
            keep: true,
            before: None,
        };
        self.carry(mv, out);
    }
}

/// "3 books", "1 shelf": a count and its noun.
pub fn counted(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}
