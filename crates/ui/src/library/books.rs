//! Book work from menus, the selection bar and drops: removal,
//! twins, renames and imports.

use std::path::PathBuf;

use mareader_library::{BookId, Data, ShelfId};

use super::io::Job;
use super::{LibraryState, PickAction, Sheet, wall_ms};
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
    /// `forget` deletes their marks and places with them.
    Remove {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
        forget: bool,
    },
    /// A separate book of the same file, with its own place.
    Twin(BookId),
    AskRename(BookId),
    Rename(BookId, String),
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
    /// Takes books off the shelf open on the page.
    TakeOff(Vec<BookId>),
}

impl LibraryState {
    pub(super) fn book_act(&mut self, action: BookAction, out: &mut Commands) {
        let now = wall_ms();
        match action {
            BookAction::AskRemove { books, shelves } => {
                // One shelf alone keeps its own sheet.
                if let ([], [shelf]) = (books.as_slice(), shelves.as_slice()) {
                    self.folders.sheet = Some(Sheet::RemoveShelf(*shelf));
                    return;
                }
                let receipt = self.model.receipt(&books);
                let sheet = Sheet::Removal {
                    books,
                    shelves,
                    receipt,
                };
                self.folders.sheet = Some(sheet);
            }
            BookAction::Remove {
                books,
                shelves,
                forget,
            } => self.remove(&books, &shelves, forget, out),
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
            BookAction::AskRename(id) => {
                if self.model.book(id).is_some() {
                    self.folders.sheet = Some(Sheet::RenameBook(id));
                }
            }
            BookAction::Rename(id, title) => {
                self.folders.sheet = None;
                self.model.rename_book(id, &title, now);
            }
            BookAction::Import { paths, to } => self.import_files(paths, to, out),
            BookAction::ShelveTogether { books, shelves } => {
                self.shelve_together(&books, &shelves, out);
            }
            BookAction::TakeOff(books) => self.take_off(&books, out),
            // Dialogs and the file manager are the app's.
            BookAction::Reveal(_) | BookAction::PickImport => {}
        }
    }

    fn remove(&mut self, books: &[BookId], shelves: &[ShelfId], forget: bool, out: &mut Commands) {
        self.folders.sheet = None;
        let now = wall_ms();
        let first = books.first().and_then(|id| self.model.book(*id));
        let first = first.map(|b| b.title.value.clone()).unwrap_or_default();
        let receipt = self.model.receipt(books);
        let data = match forget {
            true => Data::Delete,
            false => Data::Keep,
        };
        let done = self.model.remove_books(books, data, now);
        if !done.delete.is_empty()
            && let Some(store) = self.folder.as_ref().map(|f| f.store())
        {
            self.io.send(Job::Delete {
                store,
                paths: done.delete,
            });
        }
        let shelves = (shelves.iter())
            .filter(|id| self.model.remove_shelf(**id, now).is_some())
            .count();
        self.picks.apply(PickAction::Clear);
        let gone = match (done.books, shelves) {
            (0, 0) => return,
            (1, 0) => format!("Removed “{first}”."),
            (n, 0) => format!("Removed {n} books."),
            (0, s) => format!("Removed {}.", counted(s, "shelf", "shelves")),
            (n, s) => format!(
                "Removed {} and {}.",
                counted(n, "book", "books"),
                counted(s, "shelf", "shelves")
            ),
        };
        let note = match receipt.hidden {
            0 => gone,
            _ => format!("{gone} The folder's Add menu can give them back."),
        };
        out.push(Command::Notify(Toast::info(note)));
    }

    fn shelve_together(&mut self, books: &[BookId], shelves: &[ShelfId], out: &mut Commands) {
        let now = wall_ms();
        let name = self.model.fresh_shelf_name(self.at);
        let Ok(id) = self.model.create_shelf(&name, self.at, now) else {
            return;
        };
        for &book in books {
            self.model.file(book, id, now);
            // Moved in from the shelf they were on.
            if let Some(here) = self.at {
                self.model.unfile(book, here, now);
            }
        }
        let refused = (shelves.iter())
            .filter(|s| self.model.move_shelf(**s, Some(id), now).is_err())
            .count();
        if refused > 0 {
            let note = "A shelf whose name is taken there stayed where it was.";
            out.push(Command::Notify(Toast::info(note)));
        }
        self.picks.apply(PickAction::Clear);
        self.at = Some(id);
        self.renaming = Some(id);
    }

    fn take_off(&mut self, books: &[BookId], out: &mut Commands) {
        let Some(shelf) = self.at else {
            return;
        };
        let now = wall_ms();
        let off = (books.iter())
            .filter(|b| self.model.unfile(**b, shelf, now))
            .count();
        self.picks.apply(PickAction::Clear);
        let name = self.model.shelf(shelf).map(|s| s.name.value.clone());
        let name = name.unwrap_or_default();
        let note = match (off, books.first()) {
            (0, _) => return,
            (1, Some(id)) => {
                let title = self.model.book(*id).map(|b| b.title.value.clone());
                format!("Took “{}” off “{name}”.", title.unwrap_or_default())
            }
            (n, _) => format!("Took {n} books off “{name}”."),
        };
        out.push(Command::Notify(Toast::info(note)));
    }
}

/// "3 books", "1 shelf": a count and its noun.
pub fn counted(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}
