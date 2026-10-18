//! What the page and its sheets ask of the library, and who does it.

use std::path::PathBuf;

use mareader_library::{BookId, ContentId, FolderId, LinkId, Options, Placement, ShelfId};

use super::find::Finding;
use super::moving::CopyWork;
use super::{LibraryState, Pick, PickAction};
use crate::command::Commands;

/// Which books the library page lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    Missing,
}

/// Work asked for by the page, its menus and its sheets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Opens the plus card's menu.
    ShowAdd,
    /// Shows the folder dialog; the pick returns as `AskFolder`.
    PickFolder,
    /// Shows the file dialog at a folder's root.
    PickFrom(PathBuf),
    /// Asks how a folder's books are taken in.
    AskFolder(PathBuf),
    ImportFolder {
        root: PathBuf,
        opts: Options,
    },
    /// Walks every watched folder again.
    Rescan,
    /// Answers the waiting question; `all` answers its kind waiting too.
    Answer(Placement, bool),
    /// Leaves every waiting question unanswered.
    CancelAsks,
    /// Brings back a book the reader removed from a folder.
    Restore {
        folder: FolderId,
        content: ContentId,
    },
    /// Asks where a book that left the folder's shelves shows.
    AskMoved {
        folder: FolderId,
        book: BookId,
    },
    /// Files that book on its folder shelf again, as well.
    ShowMoved {
        folder: FolderId,
        book: BookId,
    },
    /// Goes where a row stands and lights it.
    Reveal(Pick),
    Scope(Scope),
    FindAgain(BookId),
    /// Shows the file dialog for a lost book.
    PickFile(BookId),
    /// Shows the folder dialog to search for a lost book.
    PickSearchFolder(BookId),
    Relink {
        book: BookId,
        path: PathBuf,
    },
    /// Takes the file offered after "isn't the same file".
    UseAnyway(BookId),
    Seek {
        book: BookId,
        folder: PathBuf,
    },
    CloseSheet,
    /// Opens the one rename sheet for a book, a shelf or a link.
    AskRename(Named),
    Rename(Named, String),
    Shelf(super::ShelfAction),
    Pick(PickAction),
    Book(super::BookAction),
    Search(super::SearchAction),
}

/// Anything the library shows under a name of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Named {
    Book(BookId),
    Shelf(ShelfId),
    Link(LinkId),
}

/// The sheet over the page, if any; questions wait behind it.
#[derive(Clone, Debug, PartialEq)]
pub enum Sheet {
    Find(Finding),
    /// Removing books and shelves, with what that does.
    Removal {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
    },
    Rename(Named),
    /// Books read in place about to leave their folder: copy them first?
    Copy(Box<CopyWork>),
    /// How a folder's books are taken in.
    Import(PathBuf),
    /// A book that left its folder's shelves: shown here, or gone to.
    Moved {
        folder: FolderId,
        book: BookId,
    },
}

impl LibraryState {
    pub fn sheet(&self) -> Option<&Sheet> {
        self.folders.sheet.as_ref()
    }

    /// A sheet or a question is over the page, holding the keys.
    pub fn sheet_up(&self) -> bool {
        self.folders.sheet.is_some() || !self.folders.asks.is_empty()
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
            Action::CloseSheet => self.close_sheet(),
            Action::AskRename(named) => {
                if self.name_of(named).is_some() {
                    self.folders.sheet = Some(Sheet::Rename(named));
                }
            }
            Action::Rename(named, name) => self.rename(named, &name),
            Action::Shelf(action) => self.shelf_act(action, out),
            Action::Pick(action) => self.picks.apply(action),
            Action::Book(action) => self.book_act(action, out),
            Action::Search(action) => self.search_act(action, now),
            // Dialogs need the app's files; it handles them.
            Action::ShowAdd
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
