//! Folder runs: imports, quiet rescans, loose files and restores.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use egui::Context;
use mareader_library::disk::Known;
use mareader_library::gate::Gate;
use mareader_library::{
    Asked, Book, BookId, ContentId, Copied, FolderId, Found, Landed, Options, Origin, Placement,
    Plan, Planned, ShelfId,
};

use super::asks::Question;
use super::find::folder_name;
use super::io::{Copies, CopyIn, Files, Job, Measured, Tag, Walk, Walked};
use super::moving::CopyWork;
use super::runs::{Phase, RunId};
use super::{LibraryState, Pick, Sheet, Status, wall_ms};
use crate::command::{Command, Commands};
use crate::toast::Toast;

/// Seconds between rescans when the window keeps regaining focus.
const RESCAN_GAP: f64 = 2.0;
/// Seconds before looking again at files still being written.
const SETTLE_WAIT: f64 = 3.5;

/// A walk on its way: what it walks, as what, and its card.
#[derive(Clone, Debug)]
struct Going {
    root: PathBuf,
    opts: Options,
    plan: Plan,
    asked: Asked,
    run: Option<RunId>,
    /// Files the walk found, for the run's last word.
    found: usize,
}

/// Work waiting on the worker, under the tag its news carries.
#[derive(Debug)]
enum Pending {
    /// A folder walking; a rescan names the folder it walks.
    Walk(Going, Option<FolderId>),
    /// A planned walk's copies, landed once made.
    Land(Box<Planned>, Going),
    /// Loose files measured, then screened onto the level.
    Files(Option<ShelfId>, Option<RunId>),
    /// Screened files copied, then landed onto the level.
    Loose(Box<Loose>),
    /// An answer's copy, then the answer written.
    Keep(Box<Question>, Placement),
    /// Books read in place copied, then the gesture that owed them.
    Depart(Box<CopyWork>),
    /// A removed book's file measured, then copied if its folder copies.
    Restore(FolderId, ContentId),
    Restored(FolderId, ContentId, Box<Found>),
}

#[derive(Debug)]
struct Loose {
    to: Option<ShelfId>,
    run: Option<RunId>,
    files: Vec<Found>,
    /// Books the screen brought back, and questions it queued.
    restored: usize,
    asked: usize,
}

/// When folders are walked, what they said, and what waits on the worker.
#[derive(Debug, Default)]
pub(super) struct Folders {
    next: Tag,
    pending: HashMap<Tag, Pending>,
    /// Watched folders walking quietly now.
    walking: BTreeSet<FolderId>,
    /// A rescan asked for while one ran.
    again: bool,
    /// When the last rescan left, in egui seconds.
    last: Option<f64>,
    settle_at: Option<f64>,
    focused: bool,
    offline: BTreeSet<FolderId>,
    pub(super) sheet: Option<Sheet>,
    /// Folders dropped while a sheet was up, asked about in turn.
    later: VecDeque<PathBuf>,
    /// The import sheet's last answers, the next one's start.
    pub(super) opts: Option<Options>,
    pub(super) asks: VecDeque<Question>,
}

impl LibraryState {
    /// Whether a rescan is walking.
    pub fn scanning(&self) -> bool {
        !self.folders.walking.is_empty()
    }

    /// Folders whose drive wasn't there at the last look.
    pub fn offline(&self) -> &BTreeSet<FolderId> {
        &self.folders.offline
    }

    /// Questions waiting, the next first; shown when no sheet is up.
    pub fn questions(&self) -> impl Iterator<Item = &Question> {
        self.folders.asks.iter()
    }

    /// The import sheet's start: the last answers, else the defaults.
    pub fn last_options(&self) -> Options {
        self.folders.opts.clone().unwrap_or_default()
    }

    fn tag(&mut self, pending: Pending) -> Tag {
        self.folders.next += 1;
        let tag = self.folders.next;
        self.folders.pending.insert(tag, pending);
        tag
    }

    fn store(&self) -> Option<PathBuf> {
        self.folder.as_ref().map(|f| f.store())
    }

    /// The import sheet for `root`, or after the sheet that's up.
    pub(super) fn ask_folder(&mut self, root: PathBuf) {
        // "Change…" on the import sheet swaps its folder.
        match self.folders.sheet {
            Some(Sheet::Import(_)) | None => self.folders.sheet = Some(Sheet::Import(root)),
            Some(_) => self.folders.later.push_back(root),
        }
    }

    /// Closes the sheet; a folder dropped meanwhile asks next.
    pub(super) fn close_sheet(&mut self) {
        self.folders.sheet = self.folders.later.pop_front().map(Sheet::Import);
    }

    /// The import sheet's answer: through the gate, then a walk.
    pub(super) fn import_folder(
        &mut self,
        root: PathBuf,
        opts: Options,
        now: f64,
        out: &mut Commands,
    ) {
        self.close_sheet();
        self.folders.opts = Some(opts.clone());
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        if self.avoid().iter().any(|own| own.starts_with(&root)) {
            let note = "That folder holds Mareader's own files; choose one inside it.";
            out.push(Command::Notify(Toast::error(note)));
            return;
        }
        if opts.mode().copies() && self.store().is_none() {
            let note = "The library has no folder to keep books in.";
            out.push(Command::Notify(Toast::error(note)));
            return;
        }
        match self.model.gate(&root, &opts) {
            Gate::Walk { root, plan } => self.walk(root, opts, plan, now),
            Gate::Clash(clash) => self.folders.asks.push_back(Question::Clash { clash, opts }),
        }
    }

    /// An import's walk, with a card.
    pub(super) fn walk(&mut self, root: PathBuf, opts: Options, plan: Plan, now: f64) {
        let run = Some(self.runs.start(folder_name(&root), Phase::Scanning));
        let going = Going {
            root,
            opts,
            plan,
            asked: Asked::Explicitly,
            run,
            found: 0,
        };
        self.send_walk(going, None);
        self.folders.last = Some(now);
    }

    fn send_walk(&mut self, going: Going, folder: Option<FolderId>) {
        let (root, opts, run) = (going.root.clone(), going.opts.clone(), going.run);
        let (known, contents) = self.known();
        let tag = self.tag(Pending::Walk(going, folder));
        self.io.send(Job::Walk(Box::new(Walk {
            tag,
            run,
            root,
            opts,
            known,
            contents,
            avoid: self.avoid(),
            now_ms: wall_ms(),
        })));
    }

    /// Content ids by path, and every content the library has met.
    fn known(&self) -> (Known, HashSet<ContentId>) {
        let mut known = Known::new();
        let mut contents = HashSet::new();
        for book in self.model.all_books() {
            contents.insert(book.content);
            for at in &book.locations {
                known.insert(at.path.clone(), (at.stamp, book.content));
            }
        }
        (known, contents)
    }

    /// Re-checks known files and walks watched folders, quietly.
    pub fn rescan(&mut self, now: f64) {
        if self.status == Status::Loading {
            return;
        }
        if !self.folders.walking.is_empty() {
            self.folders.again = true;
            return;
        }
        let living = (self.model.books()).flat_map(|b| b.locations.iter().map(|l| l.path.clone()));
        self.io.send(Job::Check(living.collect()));
        let watched: Vec<(FolderId, PathBuf, Options)> = (self.model.folders())
            .filter(|f| f.owes_walk())
            .map(|f| (f.id, f.root.clone(), f.opts.clone()))
            .collect();
        for (id, root, opts) in watched {
            let going = Going {
                root,
                opts,
                plan: Plan::default(),
                asked: Asked::OnFocus,
                run: None,
                found: 0,
            };
            self.folders.walking.insert(id);
            self.send_walk(going, Some(id));
        }
        self.folders.last = Some(now);
        self.folders.settle_at = None;
    }

    /// Rescans when the window comes back, and after files settle.
    pub fn tick(&mut self, ctx: &Context) {
        // Removals and replacements free store copies; they go now.
        let swept = self.model.take_swept();
        if let Some(store) = self.folder.as_ref().map(|f| f.store())
            && !swept.is_empty()
        {
            self.io.send(Job::Delete {
                store,
                paths: swept,
            });
        }
        let (now, focused) = ctx.input(|i| (i.time, i.focused));
        let back = focused && !self.folders.focused;
        self.folders.focused = focused;
        let rested = self.folders.last.is_none_or(|t| now - t >= RESCAN_GAP);
        let watched = self.model.folders().any(|f| f.owes_walk());
        if back && rested && watched && self.status != Status::Loading {
            self.rescan(now);
        }
        if let Some(at) = self.folders.settle_at {
            match now >= at {
                true => self.rescan(now),
                false => ctx.request_repaint_after(std::time::Duration::from_secs_f64(at - now)),
            }
        }
    }

    /// The library's own folders, which no walk enters.
    pub(super) fn avoid(&self) -> Vec<PathBuf> {
        let folder = self.folder.as_ref().map(|f| f.root().to_path_buf());
        folder.into_iter().chain(self.covers.cache_dir()).collect()
    }

    pub(super) fn checked(&mut self, gone: &HashSet<PathBuf>) {
        self.model.check_present(|p| !gone.contains(p));
    }

    pub(super) fn walked(&mut self, done: Walked, now: f64, out: &mut Commands) {
        let Some(Pending::Walk(mut going, folder)) = self.folders.pending.remove(&done.tag) else {
            return;
        };
        let name = folder_name(&going.root);
        if done.settling {
            self.folders.settle_at = Some(now + SETTLE_WAIT);
        }
        if let Some(id) = folder {
            self.rescanned(id, done, now);
            return;
        }
        if done.offline {
            self.runs.finish(going.run);
            let note = format!("“{name}” couldn't be read.");
            out.push(Command::Notify(Toast::error(note)));
            return;
        }
        if done.truncated {
            let note = format!("“{name}” is very large; only part of it was read.");
            out.push(Command::Notify(Toast::info(note)));
        }
        going.found = done.found.len();
        let wall = wall_ms();
        let (root, opts) = (going.root.clone(), going.opts.clone());
        let id = self.model.open_folder(root, opts, &going.plan, wall);
        let plan = going.plan.clone();
        let Some(planned) = self.model.plan_walk(id, done.found, going.asked, plan) else {
            self.runs.finish(going.run);
            return;
        };
        self.land_or_copy(planned, going, now, out);
    }

    /// A quiet walk of a watched folder, landed as it found.
    fn rescanned(&mut self, id: FolderId, done: Walked, now: f64) {
        self.folders.walking.remove(&id);
        match done.offline {
            true => self.folders.offline.insert(id),
            false => self.folders.offline.remove(&id),
        };
        if !done.offline {
            let plan = Plan::default();
            let planned = self.model.plan_walk(id, done.found, Asked::OnFocus, plan);
            if let Some(planned) = planned.filter(|p| !p.is_empty()) {
                let asks = planned.asks.clone();
                self.model.land_walk(planned, &HashMap::new(), wall_ms());
                self.folders
                    .asks
                    .extend(asks.into_iter().map(Question::Merge));
            }
        }
        if self.folders.walking.is_empty() && std::mem::take(&mut self.folders.again) {
            self.rescan(now);
        }
    }

    /// Copies first if the folder copies; else lands at once.
    fn land_or_copy(&mut self, planned: Planned, going: Going, now: f64, out: &mut Commands) {
        let copies = self
            .model
            .folder(planned.folder)
            .is_some_and(|f| f.mode().copies());
        let files: Vec<Found> = planned.to_copy(copies).into_iter().cloned().collect();
        let store = self.store();
        match (files.is_empty(), store) {
            (false, Some(store)) => {
                let run = going.run;
                self.runs.phase(run, Phase::Copying);
                let tag = self.tag(Pending::Land(Box::new(planned), going));
                self.io.send(Job::Copy(Box::new(CopyIn {
                    tag,
                    run,
                    store,
                    files,
                })));
            }
            _ => self.land(planned, &HashMap::new(), going, now, out),
        }
    }

    fn land(
        &mut self,
        planned: Planned,
        copies: &HashMap<PathBuf, Copied>,
        going: Going,
        now: f64,
        out: &mut Commands,
    ) {
        let asks = planned.asks.clone();
        let folder = planned.folder;
        let landed = self.model.land_walk(planned, copies, wall_ms());
        self.runs.finish(going.run);
        let waiting = asks.len();
        self.folders
            .asks
            .extend(asks.into_iter().map(Question::Merge));
        let name =
            (self.model.folder(folder)).map_or_else(|| folder_name(&going.root), |f| f.name());
        let note = landed_note(&name, &landed, going.found, waiting);
        out.push(Command::Notify(Toast::info(note)));
        if let Some(shelf) = landed.root {
            self.reveal(Pick::Shelf(shelf), now);
        }
    }

    pub(super) fn copied(&mut self, done: Copies, now: f64, out: &mut Commands) {
        if let Some((path, why)) = done.failed.first() {
            let more = match done.failed.len() {
                1 => String::new(),
                n => format!(" ({} more failed)", n - 1),
            };
            let note = format!("Couldn't copy “{}”: {why}{more}.", file_name(path));
            out.push(Command::Notify(Toast::error(note)));
        }
        match self.folders.pending.remove(&done.tag) {
            Some(Pending::Land(planned, going)) => {
                self.land(*planned, &done.copies, going, now, out);
            }
            Some(Pending::Loose(loose)) => self.land_loose(*loose, &done.copies, now, out),
            Some(Pending::Keep(question, answer)) => {
                let copy = question.found().and_then(|f| done.copies.get(&f.path));
                self.write_answer(*question, answer, copy, now, out);
            }
            Some(Pending::Depart(work)) => self.departed(*work, &done.copies, out),
            Some(Pending::Restored(folder, content, found)) => {
                let copy = done.copies.get(&found.path);
                self.restored(folder, content, &found, copy, now, out);
            }
            _ => {}
        }
    }

    /// Copies books read in place, then finishes the gesture.
    pub(super) fn copy_out(&mut self, work: Box<CopyWork>, files: Vec<Found>, out: &mut Commands) {
        let Some(store) = self.store() else {
            let note = "The library has no folder to keep books in.";
            out.push(Command::Notify(Toast::error(note)));
            return;
        };
        let tag = self.tag(Pending::Depart(work));
        let run = None;
        self.io.send(Job::Copy(Box::new(CopyIn {
            tag,
            run,
            store,
            files,
        })));
    }

    /// Copies an answer's file first, then writes the answer.
    pub(super) fn copy_for(&mut self, question: Question, answer: Placement, out: &mut Commands) {
        let (Some(store), Some(found)) = (self.store(), question.found().cloned()) else {
            let note = "The library has no folder to keep books in.";
            out.push(Command::Notify(Toast::error(note)));
            return;
        };
        let tag = self.tag(Pending::Keep(Box::new(question), answer));
        let files = vec![found];
        let (run, store) = (None, store);
        self.io.send(Job::Copy(Box::new(CopyIn {
            tag,
            run,
            store,
            files,
        })));
    }

    /// Files picked or dropped: folders ask, files are measured.
    pub(super) fn import_files(
        &mut self,
        paths: Vec<PathBuf>,
        to: Option<ShelfId>,
        out: &mut Commands,
    ) {
        let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) =
            paths.into_iter().partition(|p| p.is_dir());
        for dir in dirs {
            self.ask_folder(dir);
        }
        if files.is_empty() {
            return;
        }
        if self.store().is_none() {
            let note = "The library has no folder to keep books in.";
            out.push(Command::Notify(Toast::error(note)));
            return;
        }
        let label = match files.as_slice() {
            [one] => file_name(one),
            many => format!("{} files", many.len()),
        };
        let run = Some(self.runs.start(label, Phase::Reading));
        let tag = self.tag(Pending::Files(to, run));
        let paths = files;
        self.io
            .send(Job::Files(Box::new(Files { tag, run, paths })));
    }

    pub(super) fn measured(&mut self, done: Measured, now: f64, out: &mut Commands) {
        match self.folders.pending.remove(&done.tag) {
            Some(Pending::Files(to, run)) => self.screen(done, to, run, now, out),
            Some(Pending::Restore(folder, content)) => {
                self.restore_measured(folder, content, done, now, out);
            }
            _ => {}
        }
    }

    /// Screens loose files: covered and held ones ask, the rest copy.
    fn screen(
        &mut self,
        done: Measured,
        to: Option<ShelfId>,
        run: Option<RunId>,
        now: f64,
        out: &mut Commands,
    ) {
        let to = to.filter(|s| self.model.shelf(*s).is_some());
        let mut notes = Vec::new();
        match done.skipped.as_slice() {
            [] => {}
            [path] => notes.push(format!(
                "“{}” isn't a book Mareader reads.",
                file_name(path)
            )),
            many => notes.push(format!("{} files aren't books Mareader reads.", many.len())),
        }
        if let Some((path, why)) = done.failed.first() {
            notes.push(format!("Couldn't read “{}”: {why}.", file_name(path)));
        }
        let screened = self.model.screen_files(done.found, to, wall_ms());
        let (restored, asked) = (screened.restored.len(), screened.asks.len());
        let asks = screened.asks.into_iter().map(Question::File);
        self.folders.asks.extend(asks);
        if !notes.is_empty() {
            out.push(Command::Notify(Toast::error(notes.join(" "))));
        }
        let loose = Loose {
            to,
            run,
            files: screened.clean,
            restored,
            asked,
        };
        match (loose.files.is_empty(), self.store()) {
            (false, Some(store)) => {
                self.runs.phase(run, Phase::Copying);
                let files = loose.files.clone();
                let tag = self.tag(Pending::Loose(Box::new(loose)));
                self.io.send(Job::Copy(Box::new(CopyIn {
                    tag,
                    run,
                    store,
                    files,
                })));
            }
            _ => self.land_loose(loose, &HashMap::new(), now, out),
        }
    }

    fn land_loose(
        &mut self,
        loose: Loose,
        copies: &HashMap<PathBuf, Copied>,
        now: f64,
        out: &mut Commands,
    ) {
        self.runs.finish(loose.run);
        let to = loose.to.filter(|s| self.model.shelf(*s).is_some());
        let pairs: Vec<(Found, Copied)> = (loose.files.into_iter())
            .filter_map(|f| copies.get(&f.path).cloned().map(|c| (f, c)))
            .collect();
        let landed = self.model.land_copies(&pairs, to, wall_ms());
        let title = |id: &BookId| self.model.book(*id).map(|b| b.title.value.clone());
        let onto = (to.and_then(|s| self.model.shelf(s)))
            .map(|s| format!(" to “{}”", s.name.value))
            .unwrap_or_default();
        let mut parts = Vec::new();
        match landed.as_slice() {
            [] => {}
            [id] => parts.push(format!(
                "Imported “{}”{onto}.",
                title(id).unwrap_or_default()
            )),
            many => parts.push(format!("Imported {} books{onto}.", many.len())),
        }
        match loose.restored {
            0 => {}
            1 => parts.push("1 book came back where its folder keeps it.".to_owned()),
            n => parts.push(format!(
                "{n} books came back where their folder keeps them."
            )),
        }
        match loose.asked {
            0 => {}
            1 => parts.push("1 waits for your choice.".to_owned()),
            n => parts.push(format!("{n} wait for your choice.")),
        }
        if !parts.is_empty() {
            out.push(Command::Notify(Toast::info(parts.join(" "))));
        }
        if let [id] = landed.as_slice() {
            self.reveal(Pick::Book(*id), now);
        }
    }

    /// A removed book back: its file looked at, then copied or read.
    pub(super) fn restore(&mut self, folder: FolderId, content: ContentId, out: &mut Commands) {
        let stone = (self.model.folder(folder)).and_then(|f| f.tombstone(content));
        let Some(path) = stone.map(|s| s.last_path.clone()) else {
            return;
        };
        if !path.is_file() {
            let note = format!("“{}” is not there any more.", file_name(&path));
            out.push(Command::Notify(Toast::error(note)));
            return;
        }
        let tag = self.tag(Pending::Restore(folder, content));
        let paths = vec![path];
        let run = None;
        self.io
            .send(Job::Files(Box::new(Files { tag, run, paths })));
    }

    fn restore_measured(
        &mut self,
        folder: FolderId,
        content: ContentId,
        done: Measured,
        now: f64,
        out: &mut Commands,
    ) {
        let Some(found) = done.found.into_iter().next() else {
            let path = done.skipped.first().or(done.failed.first().map(|f| &f.0));
            let name = path.map(|p| file_name(p)).unwrap_or_default();
            let note = format!("“{name}” is not there any more.");
            out.push(Command::Notify(Toast::error(note)));
            return;
        };
        let copies = self.model.folder(folder).is_some_and(|f| f.mode().copies());
        match (copies, self.store()) {
            (true, Some(store)) => {
                let files = vec![found.clone()];
                let pending = Pending::Restored(folder, content, Box::new(found));
                let tag = self.tag(pending);
                let run = None;
                self.io.send(Job::Copy(Box::new(CopyIn {
                    tag,
                    run,
                    store,
                    files,
                })));
            }
            _ => self.restored(folder, content, &found, None, now, out),
        }
    }

    fn restored(
        &mut self,
        folder: FolderId,
        content: ContentId,
        found: &Found,
        copy: Option<&Copied>,
        now: f64,
        out: &mut Commands,
    ) {
        let wall = wall_ms();
        let Some(id) = (self.model).restore_removed(folder, content, found, copy, wall) else {
            return;
        };
        let title = self.model.book(id).map(|b| b.title.value.clone());
        let note = format!("Restored “{}”.", title.unwrap_or_default());
        out.push(Command::Notify(Toast::info(note)));
        self.reveal(Pick::Book(id), now);
    }

    /// A moved book shown on the open folder shelf too.
    pub(super) fn show_moved(&mut self, folder: FolderId, book: BookId, now: f64) {
        self.folders.sheet = None;
        let rung = |s: ShelfId| self.model.shelf(s).and_then(|s| s.kind.value.rung());
        let here = self
            .at
            .filter(|s| rung(*s).is_some_and(|(f, _)| f == folder));
        if let Some(shelf) = here {
            self.model.file(book, shelf, wall_ms());
            self.reveal(Pick::Book(book), now);
        }
    }
}

/// The folder an absent book's last address belongs to, if offline.
pub fn offline(book: &Book, offline: &BTreeSet<FolderId>) -> bool {
    book.missing()
        && book.locations.iter().any(|l| match l.origin {
            Origin::Folder(id) => offline.contains(&id),
            Origin::Opened | Origin::Store => false,
        })
}

/// "Added 3 books from “Essays”.", or why nothing was.
fn landed_note(name: &str, landed: &Landed, found: usize, waiting: usize) -> String {
    let said = match (landed.total(), found) {
        (0, 0) => format!("No books found in “{name}”."),
        (0, _) => format!("Nothing new to import from “{name}”."),
        (1, _) => format!("Added 1 book from “{name}”."),
        (n, _) => format!("Added {n} books from “{name}”."),
    };
    match waiting {
        0 => said,
        1 => format!("{said} 1 waits for your choice."),
        n => format!("{said} {n} wait for your choice."),
    }
}

fn file_name(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    name.unwrap_or_else(|| path.display().to_string())
}
