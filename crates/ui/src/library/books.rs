//! Book work from menus, the selection bar and drops: removal,
//! restoring, twins, renames and imports.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mareader_library::{BookId, ContentId, Imported, ShelfId};

use super::io::{self, Import, Job};
use super::runs::Phase;
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
    Remove {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
    },
    Restore(Vec<BookId>),
    /// A separate book of the same file, with its own place.
    Twin(BookId),
    AskRename(BookId),
    Rename(BookId, String),
    /// Shows its file in the system's file manager.
    Reveal(BookId),
    /// Shows the file dialog; the picks come back as `Import`.
    PickImport,
    /// Copies files into the library, onto a shelf if `to`.
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
            BookAction::Remove { books, shelves } => self.remove(&books, &shelves, out),
            BookAction::Restore(books) => self.restore(&books, out),
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
            BookAction::Import { paths, to } => self.import(paths, to, out),
            BookAction::ShelveTogether { books, shelves } => {
                self.shelve_together(&books, &shelves, out);
            }
            BookAction::TakeOff(books) => self.take_off(&books, out),
            // Dialogs and the file manager are the app's.
            BookAction::Reveal(_) | BookAction::PickImport => {}
        }
    }

    fn remove(&mut self, books: &[BookId], shelves: &[ShelfId], out: &mut Commands) {
        self.folders.sheet = None;
        let now = wall_ms();
        let first = books.first().and_then(|id| self.model.book(*id));
        let first = first.map(|b| b.title.value.clone()).unwrap_or_default();
        let receipt = self.model.receipt(books);
        let done = self.model.remove_books(books, now);
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
            _ => format!("{gone} Restore from Folders."),
        };
        out.push(Command::Notify(Toast::info(note)));
    }

    fn restore(&mut self, books: &[BookId], out: &mut Commands) {
        let now = wall_ms();
        let back: Vec<BookId> = (books.iter().copied())
            .filter(|id| self.model.restore(*id, now))
            .collect();
        // A few files looked at: cheaper than a walk.
        for id in &back {
            self.model.recheck(*id, Path::is_file);
        }
        let note = match back.as_slice() {
            [] => return,
            [id] => {
                let title = self.model.book(*id).map(|b| b.title.value.clone());
                format!("Restored “{}”.", title.unwrap_or_default())
            }
            many => format!("Restored {} books.", many.len()),
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

    /// Copies files into the library's folder on the worker.
    fn import(&mut self, paths: Vec<PathBuf>, to: Option<ShelfId>, out: &mut Commands) {
        let Some(store) = self.folder.as_ref().map(|f| f.store()) else {
            let note = "The library has no folder to copy books into.";
            out.push(Command::Notify(Toast::error(note)));
            return;
        };
        if paths.is_empty() {
            return;
        }
        let originals = self.model.books().filter(|b| b.twin_of.is_none());
        let known: HashSet<ContentId> = originals.map(|b| b.content).collect();
        let label = match paths.as_slice() {
            [one] => one.file_name().map(|n| n.to_string_lossy().into_owned()),
            many => Some(format!("{} files", many.len())),
        };
        let run = self.runs.start(label.unwrap_or_default(), Phase::Copying);
        let job = Import {
            paths,
            store,
            known,
            to,
            run,
        };
        self.io.send(Job::Import(Box::new(job)));
    }

    /// Takes in the copies, files them, and says what happened.
    pub(super) fn imported(&mut self, done: io::Imported, out: &mut Commands) {
        let now = wall_ms();
        let to = done.to.filter(|&s| self.model.shelf(s).is_some());
        let mut added = Vec::new();
        let mut extra = Vec::new();
        for copy in done.copies {
            let path = copy.path.clone();
            match self.model.import(copy, now) {
                Imported::New(id) => added.push(id),
                Imported::Known(id) => {
                    // Taken in meanwhile, by a scan: the copy isn't needed.
                    if !self.model.listed(&path) {
                        extra.push(path);
                    }
                    self.known_import(id, to);
                }
            }
        }
        if let (false, Some(store)) = (extra.is_empty(), self.folder.as_ref()) {
            let store = store.store();
            self.io.send(Job::Delete {
                store,
                paths: extra,
            });
        }
        let holder = |content: ContentId| {
            let mut books = self.model.books();
            books
                .find(|b| b.content == content && b.twin_of.is_none())
                .map(|b| b.id)
        };
        let known: Vec<BookId> = done.known.iter().filter_map(|(_, c)| holder(*c)).collect();
        for &id in added.iter().chain(&known) {
            if let Some(shelf) = to {
                self.model.file(id, shelf, now);
            }
        }
        let note = self.import_note(&added, &known, to, &done.skipped, &done.failed);
        let toast = match added.is_empty() && known.is_empty() {
            true => Toast::error(note),
            false => Toast::info(note),
        };
        out.push(Command::Notify(toast));
    }

    fn known_import(&mut self, id: BookId, to: Option<ShelfId>) {
        if let Some(shelf) = to {
            self.model.file(id, shelf, wall_ms());
        }
    }

    /// "Imported 3 books to “Essays”. 1 was already in your library."
    fn import_note(
        &self,
        added: &[BookId],
        known: &[BookId],
        to: Option<ShelfId>,
        skipped: &[PathBuf],
        failed: &[(PathBuf, String)],
    ) -> String {
        let title = |id: &BookId| self.model.book(*id).map(|b| b.title.value.clone());
        let onto = to.and_then(|s| self.model.shelf(s));
        let onto = onto
            .map(|s| format!(" to “{}”", s.name.value))
            .unwrap_or_default();
        let mut parts = Vec::new();
        match added {
            [] => {}
            [id] => parts.push(format!(
                "Imported “{}”{onto}.",
                title(id).unwrap_or_default()
            )),
            many => parts.push(format!("Imported {} books{onto}.", many.len())),
        }
        match known {
            [] => {}
            [id] => parts.push(format!(
                "“{}” was already in your library.",
                title(id).unwrap_or_default()
            )),
            many => parts.push(format!("{} were already in your library.", many.len())),
        }
        match skipped {
            [] => {}
            [path] => parts.push(format!("“{}” isn't a book Mareader reads.", name(path))),
            many => parts.push(format!("{} files aren't books Mareader reads.", many.len())),
        }
        if let Some((path, why)) = failed.first() {
            let more = match failed.len() {
                1 => String::new(),
                n => format!(" ({} more failed)", n - 1),
            };
            parts.push(format!("Couldn't copy “{}”: {why}{more}.", name(path)));
        }
        parts.join(" ")
    }
}

fn name(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    name.unwrap_or_else(|| path.display().to_string())
}

/// "3 books", "1 shelf": a count and its noun.
pub fn counted(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}
