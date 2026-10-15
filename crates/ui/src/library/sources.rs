//! Read-in-place folders: adding, rescanning, and finding lost books.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use egui::Context;
use mareader_library::disk::{FoundAgain, Known, Sought};
use mareader_library::{
    AddError, BookId, Format, Measure, RelinkError, Scan, ShelfId, Source, SourceEdit, SourceId,
};

use super::io::{Job, Relink, Rescan, Rescanned};
use super::runs::Phase;
use super::{LibraryState, Status, wall_ms};
use crate::command::{Command, Commands};
use crate::toast::Toast;

/// Seconds between rescans when the window keeps regaining focus.
const RESCAN_GAP: f64 = 2.0;
/// Seconds before looking again at files still being written.
const SETTLE_WAIT: f64 = 3.5;

/// Which books the library page lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    Missing,
    Folder(SourceId),
}

/// Folder and find-again work asked for by the page and its sheets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Shows the first folder and opens its menu.
    ShowFolders,
    /// Opens the plus card's menu.
    ShowAdd,
    /// Shows the folder dialog; the pick returns as `AskFolders`.
    PickFolder,
    /// Asks how to keep these folders' books.
    AskFolders(Vec<PathBuf>),
    AddFolders {
        roots: Vec<PathBuf>,
        plan: super::FolderPlan,
    },
    /// One folder looked through again, or every watched one.
    Rescan(Option<SourceId>),
    EditFolder(SourceId, SourceEdit),
    /// Asks before `RemoveFolder`.
    AskRemoveFolder(SourceId),
    RemoveFolder(SourceId),
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
    Shelf(super::ShelfAction),
    Pick(super::PickAction),
    Book(super::BookAction),
    Search(super::SearchAction),
}

/// The sheet over the page, if any.
#[derive(Clone, Debug, PartialEq)]
pub enum Sheet {
    Find(Finding),
    Remove(SourceId),
    RemoveShelf(mareader_library::ShelfId),
    /// Removing books, and maybe shelves, with what that does.
    Removal {
        books: Vec<BookId>,
        shelves: Vec<mareader_library::ShelfId>,
        receipt: mareader_library::Receipt,
    },
    RenameBook(BookId),
    /// How to keep the books of folders being added.
    AddFolders(Vec<PathBuf>),
}

/// Finding a lost book: what it's doing, or what it found.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub book: BookId,
    pub step: Step,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Ask,
    Working(String),
    /// Why that didn't work; the choices stay.
    Note(String),
    /// A file under its name with other content, offered.
    Differs {
        path: PathBuf,
        format: Format,
        measure: Measure,
    },
}

/// What a rescan walks: named folders, and maybe every watched one.
#[derive(Debug, Default)]
struct Want {
    /// Watched folders too, after re-checking every known file.
    all: bool,
    folders: BTreeSet<SourceId>,
}

/// When folders are looked through, and what they said.
#[derive(Debug, Default)]
pub(super) struct Folders {
    scanning: bool,
    /// What was asked for while a rescan ran.
    again: Option<Want>,
    /// When the last rescan left, in egui seconds.
    last: Option<f64>,
    settle_at: Option<f64>,
    focused: bool,
    /// The folder just added or rescanned by name, and books it brought.
    announce: Option<(SourceId, usize)>,
    offline: BTreeSet<SourceId>,
    pub(super) sheet: Option<Sheet>,
}

impl LibraryState {
    pub fn scanning(&self) -> bool {
        self.folders.scanning
    }

    /// Folders whose drive wasn't there at the last look.
    pub fn offline(&self) -> &BTreeSet<SourceId> {
        &self.folders.offline
    }

    pub fn sheet(&self) -> Option<&Sheet> {
        self.folders.sheet.as_ref()
    }

    /// Does what the page asked; dialogs are the app's.
    pub fn act(&mut self, action: Action, now: f64, out: &mut Commands) {
        match action {
            Action::AskFolders(roots) => self.folders.sheet = Some(Sheet::AddFolders(roots)),
            Action::AddFolders { roots, plan } => {
                self.folders.sheet = None;
                self.add_folders(roots, plan, now, out);
            }
            Action::Rescan(only) => {
                // Asked for by name: say what it found.
                self.folders.announce = only.map(|id| (id, 0));
                self.rescan(only, now);
            }
            Action::EditFolder(id, edit) => {
                if self.model.edit_source(id, edit) {
                    self.rescan(Some(id), now);
                }
            }
            Action::AskRemoveFolder(id) => self.folders.sheet = Some(Sheet::Remove(id)),
            Action::RemoveFolder(id) => {
                self.folders.sheet = None;
                let name = self.model.source(id).map(|s| s.name());
                let gone = self.model.remove_source(id, wall_ms());
                self.folders.offline.remove(&id);
                if self.scope == Scope::Folder(id) {
                    self.scope = Scope::All;
                }
                let name = name.unwrap_or_default();
                let note = match gone {
                    0 => format!("Removed “{name}”."),
                    1 => format!("Removed “{name}” and its 1 book."),
                    n => format!("Removed “{name}” and its {n} books."),
                };
                out.push(Command::Notify(Toast::info(note)));
            }
            Action::Scope(scope) => {
                self.scope = scope;
                self.picks.apply(super::PickAction::Clear);
            }
            Action::FindAgain(book) => {
                let step = Step::Ask;
                self.folders.sheet = Some(Sheet::Find(Finding { book, step }));
            }
            Action::Relink { book, path } => {
                let path = std::fs::canonicalize(&path).unwrap_or(path);
                self.finding(book, Step::Working("Checking the file…".into()));
                let force = false;
                self.io.send(Job::Relink(Relink { book, path, force }));
            }
            Action::UseAnyway(book) => {
                if let Some(Step::Differs {
                    path,
                    format,
                    measure,
                }) = self.step_of(book).cloned()
                {
                    let done = self.model.relink(book, &path, format, measure, true);
                    self.relinked(book, done, out);
                }
            }
            Action::Seek { book, folder } => {
                let Some(sought) = self.sought(book) else {
                    return;
                };
                let name = folder_name(&folder);
                self.finding(book, Step::Working(format!("Looking in “{name}”…")));
                self.io.send(Job::Seek {
                    book,
                    folder,
                    sought,
                });
            }
            Action::CloseSheet => self.folders.sheet = None,
            Action::Shelf(action) => self.shelf_act(action, out),
            Action::Pick(action) => self.picks.apply(action),
            Action::Book(action) => self.book_act(action, out),
            Action::Search(action) => self.search_act(action, now),
            // Dialogs need the app's files; it handles them.
            Action::ShowFolders
            | Action::ShowAdd
            | Action::PickFolder
            | Action::PickFile(_)
            | Action::PickSearchFolder(_) => {}
        }
    }

    /// Copies the folders' books in, or reads them where they are.
    fn add_folders(
        &mut self,
        roots: Vec<PathBuf>,
        plan: super::FolderPlan,
        now: f64,
        out: &mut Commands,
    ) {
        let super::FolderPlan { keep, walk } = plan;
        let to = self.at();
        if keep == super::FolderKeep::Copy {
            let import = super::BookAction::Import {
                paths: roots,
                to,
                walk,
            };
            return self.book_act(import, out);
        }
        for root in roots {
            let edit = SourceEdit {
                formats: walk.formats.clone(),
                exclude: walk.exclude,
                min_size: walk.min_size,
                watch: keep == super::FolderKeep::Watch,
            };
            self.add_folder(root, (edit, walk.shelving, to), now, out);
        }
    }

    /// Reads `root` in place, counted and shelved as asked, inside `to`.
    fn add_folder(
        &mut self,
        root: PathBuf,
        (edit, shelving, to): (SourceEdit, mareader_library::Shelving, Option<ShelfId>),
        now: f64,
        out: &mut Commands,
    ) {
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        let inside_library = self.avoid().iter().any(|own| own.starts_with(&root));
        if inside_library {
            let note = "That folder holds Mareader's own files; choose one inside it.";
            out.push(Command::Notify(Toast::error(note)));
            return;
        }
        match self.model.add_source(root, wall_ms()) {
            Ok(added) => {
                self.folders.announce = Some((added.id, 0));
                if !added.absorbed.is_empty() {
                    let names = added.absorbed.join("”, “");
                    let note = format!("It takes in “{names}”, which you'd added before.");
                    out.push(Command::Notify(Toast::info(note)));
                }
                self.model.edit_source(added.id, edit);
                self.model.shelve_source(added.id, shelving, to, wall_ms());
                self.rescan(Some(added.id), now);
            }
            Err(AddError::Inside(name)) => {
                let note = format!("That folder is already read as part of “{name}”.");
                out.push(Command::Notify(Toast::info(note)));
            }
            Err(AddError::Disk) => {
                let note = "That folder couldn't be read.";
                out.push(Command::Notify(Toast::error(note)));
            }
        }
    }

    /// Walks `only`, else re-checks known files and walks watched folders.
    pub fn rescan(&mut self, only: Option<SourceId>, now: f64) {
        if self.status == Status::Loading {
            return;
        }
        let want = Want {
            all: only.is_none(),
            folders: only.into_iter().collect(),
        };
        if self.folders.scanning {
            let again = self.folders.again.get_or_insert_default();
            again.all |= want.all;
            again.folders.extend(want.folders);
            return;
        }
        self.start(want, now);
    }

    fn start(&mut self, want: Want, now: f64) {
        let folders = self.model.sources();
        let folders: Vec<Source> = folders
            .filter(|s| want.folders.contains(&s.id) || (want.all && s.watch))
            .cloned()
            .collect();
        let mut locations = Vec::new();
        let mut known = Known::new();
        let mut contents = HashSet::new();
        for book in self.model.all_books() {
            contents.insert(book.content);
            for at in &book.locations {
                known.insert(at.path.clone(), (at.stamp, book.content));
                if book.alive() {
                    locations.push(at.path.clone());
                }
            }
        }
        // Only folders asked for by name show a card; returns stay quiet.
        let named: Vec<String> = (folders.iter())
            .filter(|s| want.folders.contains(&s.id))
            .map(Source::name)
            .collect();
        let run = match named.as_slice() {
            [] => None,
            [one] => Some(self.runs.start(one.clone(), Phase::Scanning)),
            many => Some(
                self.runs
                    .start(format!("{} folders", many.len()), Phase::Scanning),
            ),
        };
        let job = Rescan {
            locations: want.all.then_some(locations),
            folders,
            known,
            contents,
            avoid: self.avoid(),
            now_ms: wall_ms(),
            run,
        };
        self.io.send(Job::Rescan(Box::new(job)));
        self.folders.scanning = true;
        self.folders.last = Some(now);
        self.folders.settle_at = None;
    }

    /// Rescans when the window comes back, and after files settle.
    pub fn tick(&mut self, ctx: &Context) {
        let (now, focused) = ctx.input(|i| (i.time, i.focused));
        let back = focused && !self.folders.focused;
        self.folders.focused = focused;
        let rested = self.folders.last.is_none_or(|t| now - t >= RESCAN_GAP);
        let watched = self.model.sources().any(|s| s.watch);
        if back && rested && watched && self.status != Status::Loading {
            self.rescan(None, now);
        }
        if let Some(at) = self.folders.settle_at {
            match now >= at {
                true => self.rescan(None, now),
                false => ctx.request_repaint_after(std::time::Duration::from_secs_f64(at - now)),
            }
        }
    }

    /// The library's own folders, which no walk enters.
    pub(super) fn avoid(&self) -> Vec<PathBuf> {
        let folder = self.folder.as_ref().map(|f| f.root().to_path_buf());
        folder.into_iter().chain(self.covers.cache_dir()).collect()
    }

    pub(super) fn rescanned(&mut self, mut done: Rescanned, now: f64, out: &mut Commands) {
        self.folders.scanning = false;
        self.runs.finish(done.run);
        // A walk of named folders didn't look at other files.
        if let Some(gone) = &done.gone {
            self.model.check_present(|p| !gone.contains(p));
        }
        for scan in std::mem::take(&mut done.scans) {
            self.apply_scan(scan, &done, out);
        }
        if !done.settling.is_empty() {
            self.folders.settle_at = Some(now + SETTLE_WAIT);
        }
        if let Some(want) = self.folders.again.take() {
            self.start(want, now);
        }
    }

    fn apply_scan(&mut self, scan: Scan, done: &Rescanned, out: &mut Commands) {
        let id = scan.source;
        match scan.offline {
            true => self.folders.offline.insert(id),
            false => self.folders.offline.remove(&id),
        };
        let offline = scan.offline;
        let report = self.model.apply_scan(scan, wall_ms());
        let Some((announced, before)) = self.folders.announce.filter(|a| a.0 == id) else {
            return;
        };
        let brought = before + report.added.len() + report.back.len();
        // Files still arriving are counted once they've settled.
        if done.settling.contains(&id) {
            self.folders.announce = Some((announced, brought));
            return;
        }
        self.folders.announce = None;
        let name = self.model.source(id).map(|s| s.name()).unwrap_or_default();
        let total = self.model.books_in(id).count();
        let note = match (offline, brought, total) {
            (true, _, _) => format!("“{name}” isn't there right now."),
            (false, 0, 0) => format!("No books found in “{name}”."),
            (false, 0, _) => format!("No new books in “{name}”."),
            (false, 1, _) => format!("Added 1 book from “{name}”."),
            (false, n, _) => format!("Added {n} books from “{name}”."),
        };
        out.push(Command::Notify(Toast::info(note)));
        if done.truncated.contains(&id) {
            let note = format!("“{name}” is very large; only part of it was read.");
            out.push(Command::Notify(Toast::info(note)));
        }
    }

    fn step_of(&self, book: BookId) -> Option<&Step> {
        match &self.folders.sheet {
            Some(Sheet::Find(f)) if f.book == book => Some(&f.step),
            _ => None,
        }
    }

    /// Moves the find-again sheet on, if it's still for `book`.
    fn finding(&mut self, book: BookId, step: Step) {
        if let Some(Sheet::Find(f)) = &mut self.folders.sheet
            && f.book == book
        {
            f.step = step;
        }
    }

    /// What a lost book's file is looked for by.
    fn sought(&self, book: BookId) -> Option<Sought> {
        let book = self.model.book(book)?;
        let last = book.locations.first();
        Some(Sought {
            content: book.content,
            format: book.format,
            name: last.and_then(|l| l.path.file_name()).map(ToOwned::to_owned),
            size: last.map(|l| l.stamp.size).filter(|&s| s > 0),
        })
    }

    pub(super) fn relink_measured(
        &mut self,
        relink: Relink,
        measured: Option<(Format, Measure)>,
        out: &mut Commands,
    ) {
        let book = relink.book;
        let Some((format, measure)) = measured else {
            self.finding(book, Step::Note("That file couldn't be read.".into()));
            return;
        };
        let done = self
            .model
            .relink(book, &relink.path, format, measure, relink.force);
        match done {
            Err(RelinkError::Differs) => {
                let path = relink.path;
                self.finding(
                    book,
                    Step::Differs {
                        path,
                        format,
                        measure,
                    },
                );
            }
            done => self.relinked(book, done, out),
        }
    }

    pub(super) fn sought_found(
        &mut self,
        book: BookId,
        folder: &Path,
        found: FoundAgain,
        out: &mut Commands,
    ) {
        let format = self.model.book(book).map(|b| b.format);
        match (found, format) {
            (FoundAgain::Same(path, measure), Some(format)) => {
                let done = self.model.relink(book, &path, format, measure, false);
                self.relinked(book, done, out);
            }
            (FoundAgain::Named(path, measure), Some(format)) => {
                let step = Step::Differs {
                    path,
                    format,
                    measure,
                };
                self.finding(book, step);
            }
            _ => {
                let name = folder_name(folder);
                let note = format!("It isn't anywhere in “{name}”.");
                self.finding(book, Step::Note(note));
            }
        }
    }

    /// Closes the sheet and opens the book, or says why not.
    fn relinked(&mut self, book: BookId, done: Result<(), RelinkError>, out: &mut Commands) {
        let note = match done {
            Ok(()) => {
                self.folders.sheet = None;
                out.push(Command::OpenBook(book));
                return;
            }
            Err(RelinkError::Removed) => {
                self.folders.sheet = None;
                return;
            }
            Err(RelinkError::OtherBook(title)) => {
                format!("That file is “{title}”, already in your library.")
            }
            Err(RelinkError::Format { file, book }) => {
                let (file, book) = (file.label(), book.label());
                format!("That's a {file} file; this book is a {book}.")
            }
            Err(RelinkError::Differs) => "That isn't the same file.".into(),
        };
        self.finding(book, Step::Note(note));
    }
}

pub(super) fn folder_name(folder: &Path) -> String {
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned());
    name.unwrap_or_else(|| folder.display().to_string())
}
