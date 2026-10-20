//! A folder run: a walk's files decided, then landed on their rungs.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::book::{Book, Origin, display_title};
use crate::clock::Field;
use crate::conflict::{Arrival, Entry, Level, collide};
use crate::extra::Extra;
use crate::folder::Found;
use crate::folder::{Folder, Options, key_chain};
use crate::id::{BookId, FolderId, ShelfId};
use crate::ledger::{self, Action, Known, Registry};
use crate::library::{Library, Measure, Opened};
use crate::shelves::{Shelf, ShelfKind};

/// Who asked for the walk: a rescan stays quiet, an import may add.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    OnFocus,
    Explicitly,
}

/// What the import sheet's questions settled before the walk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Merge: the folder's books file into this standing shelf.
    pub into: Option<ShelfId>,
    /// Add as new: a tree of its own, its root under this name.
    pub rename: Option<String>,
    /// A pick inside a tree read in place: the shelf the run lights.
    pub continuation: Option<ShelfId>,
    /// The pick's own tree folds into the tree it stands inside.
    pub fold: Option<Fold>,
    /// The rung the pick answers for; empty is the tree's own ground.
    pub rung: String,
    /// Copies beside a tree read in place, its ledger left alone.
    pub beside: bool,
}

/// A pick folded into its tree: that tree and the pick's rung in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fold {
    pub tree: FolderId,
    pub rel: String,
}

impl Plan {
    /// A merge or an "as new": the run files onto other shelves.
    pub fn reshelves(&self) -> bool {
        self.into.is_some() || self.rename.is_some()
    }

    /// The rung the pick's answers are about, in the tree they land in.
    pub fn answered_rung(&self) -> &str {
        match &self.fold {
            Some(fold) => &fold.rel,
            None => &self.rung,
        }
    }
}

/// A merge's question about one arriving file and the row it meets.
#[derive(Clone, Debug, PartialEq)]
pub struct MergeAsk {
    pub arrival: Arrival,
    pub found: Found,
    pub existing: Entry,
    pub folder: FolderId,
}

/// A walk decided, waiting for its copies; nothing is written yet.
#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
    pub folder: FolderId,
    /// Files that owe a book; a copying folder copies these first.
    pub adds: Vec<Found>,
    pub relinks: Vec<(BookId, Found)>,
    /// Books already held, filed on the rung their file names.
    pub returned: Vec<(BookId, Found)>,
    pub asks: Vec<MergeAsk>,
    /// A tree read in place inside this one, folded in as it lands.
    pub member: Option<Fold>,
    /// Addresses read in place that this copying run copies over.
    pub copy_over: BTreeSet<PathBuf>,
    pub plan: Plan,
}

impl Planned {
    /// A rescan of an unchanged folder: nothing to land.
    pub fn is_empty(&self) -> bool {
        self.adds.is_empty()
            && self.relinks.is_empty()
            && self.returned.is_empty()
            && self.asks.is_empty()
    }

    /// The files to copy into the store before landing.
    pub fn to_copy(&self, mode_copies: bool) -> Vec<&Found> {
        match mode_copies {
            true => self.adds.iter().collect(),
            false => Vec::new(),
        }
    }
}

/// A file copied into the library's own folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copied {
    pub path: PathBuf,
    pub measure: Measure,
}

/// What a landing did, for the run's card.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Landed {
    pub placed: Vec<BookId>,
    pub relinked: usize,
    /// Held books whose file changed in place.
    pub healed: usize,
    pub returned: usize,
    /// The shelf the run lights: the pick's own, else the root's.
    pub root: Option<ShelfId>,
}

impl Landed {
    /// The run's count: everything that landed or moved.
    pub fn total(&self) -> usize {
        self.placed.len() + self.relinked + self.healed + self.returned
    }
}

impl Library {
    pub fn folders(&self) -> impl Iterator<Item = &Folder> {
        self.folders.values()
    }

    pub fn folder(&self, id: FolderId) -> Option<&Folder> {
        self.folders.get(&id)
    }

    pub(crate) fn folder_list(&self) -> Vec<Folder> {
        self.folders.values().cloned().collect()
    }

    /// Each content to the one book that answers for it.
    pub fn registry(&self) -> Registry {
        let shared = self.books().filter(|b| b.shared());
        let known = |b: &Book| {
            let home = b.home()?;
            Some(Known {
                id: b.id,
                path: home.path.clone(),
                missing: b.missing(),
                source: home.from.clone(),
            })
        };
        shared
            .filter_map(|b| Some((b.content, known(b)?)))
            .collect()
    }

    /// The folder a run walks: the standing one at `root`, else a new one.
    pub fn open_folder(
        &mut self,
        root: PathBuf,
        opts: Options,
        plan: &Plan,
        wall_ms: u64,
    ) -> FolderId {
        let groups = opts.groups;
        let standing = match plan.beside {
            true => None,
            false => self.folder_at(&root, opts.in_place),
        };
        let id = standing.unwrap_or_else(|| self.new_folder_id(&root, wall_ms));
        let mut folder = (self.folders.get(&id).cloned())
            .unwrap_or_else(|| Folder::new(id, root, opts.clone(), wall_ms));
        // The tree's own shape stands; the pick's rung is reshaped below.
        let shape = folder.opts.groups;
        folder.opts = opts;
        folder.opts.groups = shape;
        folder.sanitize();
        if folder.mode().in_place() {
            folder.set_tracking(&plan.rung, folder.opts.watch);
        }
        let shelves = &self.shelves;
        let standing_shelf = |s: &ShelfId| shelves.get(s).is_some_and(Shelf::alive);
        folder.shelf_map.retain(|_, s| standing_shelf(s));
        if let Some(into) = plan.into {
            folder.shelf_map.insert(String::new(), into);
        }
        // An answer of "as new" owes a tree of its own.
        if plan.rename.is_some() {
            folder.shelf_map.clear();
        }
        let reshape = folder.shape_at(&plan.rung) != groups;
        self.folders.insert(id, folder);
        // A fold's shape is the tree's to take, once folded.
        if reshape && plan.fold.is_none() {
            self.reshape(id, &plan.rung, groups, wall_ms);
        }
        id
    }

    /// The folder at `root`: one reading the same way, else any.
    pub fn folder_at(&self, root: &Path, in_place: bool) -> Option<FolderId> {
        let at = || self.folders.values().filter(|f| f.root == root);
        let same = at().find(|f| f.mode().in_place() == in_place);
        same.or_else(|| at().next()).map(|f| f.id)
    }

    /// The walk's files to decisions; the ledger's own notes are written.
    pub fn plan_walk(
        &mut self,
        id: FolderId,
        found: Vec<Found>,
        asked: Asked,
        plan: Plan,
    ) -> Option<Planned> {
        let registry = self.registry();
        let mut folder = self.folders.get(&id)?.clone();
        let quiet = asked == Asked::OnFocus;
        let copy_over: BTreeSet<PathBuf> = match folder.mode().copies() && !quiet {
            true => self.read_in_place(&found, &registry),
            false => BTreeSet::new(),
        };
        ledger::prune(&mut folder, &registry);
        folder.record_seen(found.iter().map(|f| (f.measure.content, f.path.as_path())));
        let mut planned = Planned {
            folder: id,
            adds: Vec::new(),
            relinks: Vec::new(),
            returned: Vec::new(),
            asks: Vec::new(),
            member: None,
            copy_over,
            plan,
        };
        if asked == Asked::Explicitly && planned.plan.fold.is_none() {
            planned.member = self.displaced_member(id, &found);
        }
        for file in &found {
            let content = file.measure.content;
            let action = match asked {
                Asked::OnFocus => ledger::decide(&folder, &registry, &file.path, content),
                Asked::Explicitly => ledger::decide_import(&folder, &registry, &file.path, content),
            };
            match action {
                Action::Add => planned.adds.push(file.clone()),
                Action::Relink(book) => planned.relinks.push((book, file.clone())),
                Action::Skip => {}
            }
        }
        // A relink onto an address another book reads is dropped.
        planned
            .relinks
            .retain(|(book, f)| self.book_at(&f.path).is_none_or(|b| b == *book));
        for file in found.iter().filter(|f| planned.copy_over.contains(&f.path)) {
            if !planned.adds.iter().any(|a| a.path == file.path) {
                planned.adds.push(file.clone());
            }
        }
        self.plan_trees(&folder, &found, &registry, &mut planned);
        // A file at an address the library holds is that book, edited.
        let healed: Vec<(BookId, Found)> = (planned.adds.iter())
            .filter(|f| !planned.copy_over.contains(&f.path))
            .filter_map(|f| Some((self.book_at(&f.path)?, f.clone())))
            .collect();
        planned
            .adds
            .retain(|f| !healed.iter().any(|(_, h)| h.path == f.path));
        for (book, file) in healed {
            planned.relinks.push((book, file));
        }
        let mut seen: HashSet<_> = match asked {
            Asked::OnFocus => registry.keys().copied().collect(),
            Asked::Explicitly => HashSet::new(),
        };
        planned.adds.retain(|f| seen.insert(f.measure.content));
        // Content held elsewhere lands as its held book, never a copy.
        let held: Vec<(BookId, Found)> = (planned.adds.iter())
            .filter(|f| !planned.copy_over.contains(&f.path))
            .filter_map(|f| Some((registry.get(&f.measure.content)?.id, f.clone())))
            .collect();
        planned
            .adds
            .retain(|f| !held.iter().any(|(_, h)| h.path == f.path));
        planned.returned.extend(held);
        self.screen_merge(&folder, &mut planned);
        if !quiet && folder.mode().in_place() && !planned.plan.reshelves() {
            let back = self.off_the_tree(&folder, &found, &planned);
            for (_, file) in &back {
                folder.placed.insert(file.measure.content);
            }
            planned.returned.extend(back);
        }
        self.folders.insert(id, folder);
        Some(planned)
    }

    /// Addresses the library reads in place among `found`.
    fn read_in_place(&self, found: &[Found], registry: &Registry) -> BTreeSet<PathBuf> {
        let in_place = |b: &Book| {
            b.home()
                .is_some_and(|l| matches!(l.origin, Origin::Folder(_)))
        };
        let held = |f: &&Found| {
            let known = registry.get(&f.measure.content);
            known.is_some_and(|k| k.path == f.path && self.book(k.id).is_some_and(in_place))
        };
        found.iter().filter(held).map(|f| f.path.clone()).collect()
    }

    /// A planned tree files the books it already holds, asking on a merge.
    fn plan_trees(
        &self,
        folder: &Folder,
        found: &[Found],
        registry: &Registry,
        planned: &mut Planned,
    ) {
        if !planned.plan.reshelves() {
            return;
        }
        let copy_over = &planned.copy_over;
        planned
            .adds
            .retain(|f| !registry.contains_key(&f.measure.content) || copy_over.contains(&f.path));
        for file in found {
            if copy_over.contains(&file.path) {
                continue;
            }
            let known = registry.get(&file.measure.content).map(|k| k.id);
            let Some(book) = known.or_else(|| self.book_at(&file.path)) else {
                continue;
            };
            match self.merge_ask(folder, &planned.plan, file) {
                Some(ask) => planned.asks.push(ask),
                None => planned.returned.push((book, file.clone())),
            }
        }
    }

    /// Files a merge would land on a row of the same name ask first.
    fn screen_merge(&self, folder: &Folder, planned: &mut Planned) {
        let plan = planned.plan.clone();
        let mut asks = Vec::new();
        planned
            .adds
            .retain(|file| match self.merge_ask(folder, &plan, file) {
                Some(ask) => {
                    asks.push(ask);
                    false
                }
                None => true,
            });
        planned.asks.extend(asks);
    }

    fn merge_ask(&self, folder: &Folder, plan: &Plan, file: &Found) -> Option<MergeAsk> {
        let into = plan.into?;
        let key = folder.rung_of(&file.path)?;
        let level = match key.is_empty() {
            true => into,
            false => *folder.shelf_map.get(&key)?,
        };
        let arrival = Arrival::import(file.path.clone(), Some(level), None);
        let rows = self.level_rows(Some(level));
        let existing = collide(rows.iter().map(|(e, n)| (*e, n.as_str())), &arrival)?;
        Some(MergeAsk {
            arrival,
            found: file.clone(),
            existing,
            folder: folder.id,
        })
    }

    /// Held books the walk found that stand on no rung of this tree.
    fn off_the_tree(
        &self,
        folder: &Folder,
        found: &[Found],
        planned: &Planned,
    ) -> Vec<(BookId, Found)> {
        let registry = self.registry();
        let rungs: BTreeSet<ShelfId> = folder.shelf_map.values().copied().collect();
        let taken = |f: &Found| {
            planned.adds.iter().any(|a| a.path == f.path)
                || planned.relinks.iter().any(|(_, r)| r.path == f.path)
                || planned.copy_over.contains(&f.path)
        };
        let mut out = Vec::new();
        for file in found.iter().filter(|f| !taken(f)) {
            let known = registry.get(&file.measure.content).map(|k| k.id);
            let Some(book) = known.or_else(|| self.book_at(&file.path)) else {
                continue;
            };
            let on_ours = self.shelves_of(book).iter().any(|s| {
                let kind = self.shelf(*s).map(|s| s.kind.value.clone());
                rungs.contains(s) || kind.and_then(|k| k.rung().map(|(f, _)| f)) == Some(folder.id)
            });
            if !on_ours {
                out.push((book, file.clone()));
            }
        }
        out
    }

    /// Books and links on a level with the names they show.
    pub fn level_rows(&self, level: Level) -> Vec<(Entry, String)> {
        let shelved = self.shelved();
        let books = self.books().filter(|b| match level {
            Some(shelf) => self
                .members
                .get(&(shelf, b.id))
                .is_some_and(|m| m.alive.value),
            None => !shelved.contains(&b.id),
        });
        let mut out: Vec<(Entry, String)> = books
            .map(|b| (Entry::Book(b.id), b.title.value.clone()))
            .collect();
        let links = self
            .links
            .values()
            .filter(|l| l.alive() && l.level.value == level);
        out.extend(links.map(|l| (Entry::Link(l.id), l.name.value.clone())));
        out
    }

    /// Writes a planned walk; a copying run passes what it copied.
    pub fn land_walk(
        &mut self,
        planned: Planned,
        copies: &HashMap<PathBuf, Copied>,
        wall_ms: u64,
    ) -> Landed {
        let mut landed = Landed::default();
        let Some(mut folder) = self.folders.get(&planned.folder).cloned() else {
            return landed;
        };
        self.rehang(planned.folder, wall_ms);
        for (book, file) in &planned.relinks {
            if self.relink_found(*book, file) {
                landed.relinked += 1;
            }
        }
        // A member's rungs seed the map, so the walk reuses them.
        if let Some(Fold { tree: member, rel }) = &planned.member
            && folder.cuts(rel)
        {
            for (key, shelf) in self.rungs_of(*member) {
                let key = match key.is_empty() {
                    true => rel.clone(),
                    false => format!("{rel}/{key}"),
                };
                folder.shelf_map.insert(key, shelf);
            }
        }
        let rename = planned.plan.rename.clone();
        let merged = planned.plan.into.is_some();
        for file in &planned.adds {
            let copy = planned.copy_over.contains(&file.path);
            let Some(book) = self.mint_found(&mut folder, file, copies, copy, wall_ms) else {
                continue;
            };
            let Some(key) = folder.rung_of(&file.path) else {
                continue;
            };
            let shelf = self.rung_chain(&mut folder, &key, rename.as_deref(), merged, wall_ms);
            self.file(book, shelf, wall_ms);
            landed.placed.push(book);
        }
        for (book, file) in &planned.returned {
            let Some(key) = folder.rung_of(&file.path) else {
                continue;
            };
            let shelf = self.rung_chain(&mut folder, &key, rename.as_deref(), merged, wall_ms);
            folder.placed.insert(file.measure.content);
            self.file(*book, shelf, wall_ms);
            landed.returned += 1;
        }
        // What landed is what the next walk is measured against.
        let walked = planned
            .adds
            .iter()
            .chain(planned.returned.iter().map(|(_, f)| f));
        for file in walked {
            let entry = (file.measure.content, file.path.clone());
            if folder.placed.contains(&entry.0) && !folder.last_seen.contains(&entry) {
                folder.last_seen.push(entry);
            }
        }
        landed.root = folder.shelf_map.get("").copied();
        self.folders.insert(planned.folder, folder);
        self.rehang(planned.folder, wall_ms);
        if let Some(seat) = self.fold_run(&planned, wall_ms) {
            landed.root = Some(seat);
        }
        if let Some(lit) = planned
            .plan
            .continuation
            .filter(|s| self.shelf(*s).is_some())
        {
            landed.root = Some(lit);
        }
        self.mirror_changed();
        self.index();
        self.bump();
        landed
    }

    /// A held book's file found at a new address, or edited in place.
    pub(crate) fn relink_found(&mut self, id: BookId, file: &Found) -> bool {
        let Some(book) = self.books.get(&id) else {
            return false;
        };
        // A stored copy keeps its bytes; only its source moves.
        if let Some(home) = book.home().filter(|l| l.origin == Origin::Store) {
            let (path, stamp) = (home.path.clone(), home.stamp);
            if home.from.as_deref() == Some(&file.path) {
                return false;
            }
            self.place_from(id, &path, stamp, Origin::Store, Some(file.path.clone()));
            return true;
        }
        let origin = self.origin_at(&file.path);
        if let Some(book) = self.books.get_mut(&id) {
            book.content = file.measure.content;
            book.format = file.format;
            book.locations.retain(|l| l.present && l.path != file.path);
        }
        self.place(id, &file.path, file.measure.stamp, origin);
        true
    }

    /// One walked file made a book: in place, or its copy in the store.
    pub(crate) fn mint_found(
        &mut self,
        folder: &mut Folder,
        file: &Found,
        copies: &HashMap<PathBuf, Copied>,
        copy_over: bool,
        wall_ms: u64,
    ) -> Option<BookId> {
        let content = file.measure.content;
        let stone = folder.tombstone(content).cloned();
        // Removed with its data, its name went too.
        let record = self.books.get(&BookId::original(content));
        let forgot =
            record.is_some_and(|b| !b.alive() && b.forgot.is_some() && b.forgot >= b.removed);
        let title = match stone.as_ref().filter(|s| !s.title.is_empty() && !forgot) {
            Some(stone) => stone.title.clone(),
            None => display_title(file.title.as_deref(), &file.path),
        };
        let copied = copies.get(&file.path);
        let (path, measure, origin, from) = match folder.mode().in_place() {
            true => (
                file.path.clone(),
                file.measure,
                Origin::Folder(folder.id),
                None,
            ),
            false => {
                let copied = copied?;
                let from = Some(file.path.clone());
                (copied.path.clone(), copied.measure, Origin::Store, from)
            }
        };
        let opened = Opened {
            path: path.clone(),
            measure,
            format: file.format,
            title,
            author: file.author.clone(),
        };
        let at = self.clock.tick(wall_ms);
        let id = match copy_over || self.beside_own_copy(file, folder) {
            true => self.add_independent(&opened, wall_ms),
            false => self.add(&opened, wall_ms, at, false),
        };
        self.place_from(id, &path, measure.stamp, origin, from);
        folder.placed.insert(content);
        folder.lift(content);
        Some(id)
    }

    /// An in-place file whose content the library holds as its own copy.
    fn beside_own_copy(&self, file: &Found, folder: &Folder) -> bool {
        let copy_of = |b: &Book| {
            b.shared()
                && b.content == file.measure.content
                && b.home().is_some_and(|l| {
                    l.origin == Origin::Store && l.from.as_deref() == Some(&file.path)
                })
        };
        folder.mode().in_place() && self.books().any(copy_of)
    }

    /// A new book read on its own, though its content is held.
    pub(crate) fn add_independent(&mut self, opened: &Opened, wall_ms: u64) -> BookId {
        let at = self.clock.tick(wall_ms);
        let id = self.new_book_id(at);
        let key = self.next_key(None);
        let book = Book {
            id,
            content: opened.measure.content,
            format: opened.format,
            title: Field::new(opened.title.clone(), at),
            author: Field::new(opened.author.clone(), at),
            added: wall_ms,
            opened: Field::new(0, at),
            key: Field::new(key, at),
            locations: Vec::new(),
            removed: None,
            restored: None,
            twin_of: Field::default(),
            independent: true,
            design: Field::default(),
            forgot: None,
            extra: Extra::default(),
        };
        self.books.insert(id, book);
        id
    }

    /// The shelf a rung names, minting every rung up to it.
    pub(crate) fn rung_chain(
        &mut self,
        folder: &mut Folder,
        key: &str,
        rename: Option<&str>,
        moved: bool,
        wall_ms: u64,
    ) -> ShelfId {
        let mut parent: Option<ShelfId> = None;
        let mut id = None;
        for rung in key_chain(key) {
            let standing = folder
                .shelf_map
                .get(rung)
                .copied()
                .filter(|s| self.shelf(*s).is_some());
            let shelf = match standing {
                Some(shelf) => shelf,
                None => {
                    let name = match (rung.is_empty(), rename) {
                        (true, Some(name)) => name.to_owned(),
                        _ => folder.rung_name(rung),
                    };
                    let kind = ShelfKind::Rung {
                        folder: folder.id,
                        key: rung.to_owned(),
                    };
                    let shelf = self.mint_shelf(&name, parent, kind, moved, wall_ms);
                    folder.shelf_map.insert(rung.to_owned(), shelf);
                    shelf
                }
            };
            parent = Some(shelf);
            id = Some(shelf);
        }
        id.unwrap_or_else(|| self.mint_shelf(&folder.name(), None, ShelfKind::Own, moved, wall_ms))
    }

    /// A shelf of any kind, named as given: rungs keep their disk names.
    pub(crate) fn mint_shelf(
        &mut self,
        name: &str,
        parent: Option<ShelfId>,
        kind: ShelfKind,
        moved: bool,
        wall_ms: u64,
    ) -> ShelfId {
        let at = self.clock.tick(wall_ms);
        let id = self.new_shelf_id(at);
        let key = self.next_key(parent);
        let name = crate::shelves::clean(name).unwrap_or_else(|| "Folder".to_owned());
        let shelf = Shelf {
            id,
            name: Field::new(name, at),
            parent: Field::new(parent, at),
            from: None,
            added: wall_ms,
            key: Field::new(key, at),
            kind: Field::new(kind, at),
            moved: Field::new(moved, at),
            removed: None,
            restored: None,
            extra: Extra::default(),
        };
        self.shelves.insert(id, shelf);
        self.bump();
        id
    }

    /// The folder's own shelves hang as the disk does, unless hand-moved.
    pub(crate) fn rehang(&mut self, folder: FolderId, wall_ms: u64) {
        let rungs: HashMap<String, ShelfId> = (self.shelves.values())
            .filter(|s| s.alive())
            .filter_map(|s| {
                let (f, key) = s.kind.value.rung()?;
                (f == folder).then(|| (key.to_owned(), s.id))
            })
            .collect();
        let above = |key: &str| rung_above(&rungs, key);
        let moves: Vec<(ShelfId, Option<ShelfId>)> = (rungs.iter())
            .filter_map(|(key, id)| {
                let shelf = self.shelves.get(id)?;
                let want = above(key);
                (!shelf.moved.value && shelf.parent.value != want).then_some((*id, want))
            })
            .collect();
        if moves.is_empty() {
            return;
        }
        let at = self.clock.tick(wall_ms);
        for (id, want) in moves {
            let key = self.next_key(want);
            if let Some(shelf) = self.shelves.get_mut(&id) {
                shelf.from = shelf.parent.value;
                shelf.parent = Field::new(want, at);
                shelf.key = Field::new(key, at);
            }
        }
        self.bump();
    }

    /// Where a rung hangs by the disk; `None` for no rung.
    pub fn seat(&self, id: ShelfId) -> Option<Option<ShelfId>> {
        let (folder, key) = self.shelves.get(&id)?.kind.value.rung()?;
        let rungs: HashMap<String, ShelfId> = (self.shelves.values())
            .filter(|s| s.alive() && s.id != id)
            .filter_map(|s| {
                let (f, key) = s.kind.value.rung()?;
                (f == folder).then(|| (key.to_owned(), s.id))
            })
            .collect();
        Some(rung_above(&rungs, key))
    }

    /// Where a path stands: inside a folder read in place, or opened.
    pub(crate) fn origin_at(&self, path: &Path) -> Origin {
        let reads = |f: &&Folder| f.mode().in_place() && path.starts_with(&f.root);
        match self.folders.values().find(reads) {
            Some(f) => Origin::Folder(f.id),
            None => self.origin_of(path),
        }
    }
}

impl Library {
    /// A removed book back from its folder, the filters aside.
    pub fn restore_removed(
        &mut self,
        folder: FolderId,
        content: crate::id::ContentId,
        file: &Found,
        copy: Option<&Copied>,
        wall_ms: u64,
    ) -> Option<BookId> {
        let mut ledger = self.folders.get(&folder)?.clone();
        let stone = ledger.tombstone(content)?.clone();
        let mut copies = HashMap::new();
        if let Some(copy) = copy {
            copies.insert(file.path.clone(), copy.clone());
        }
        let id = self.mint_found(&mut ledger, file, &copies, false, wall_ms)?;
        ledger.lift(content);
        // The file changed: the landing outvotes the old log.
        if file.measure.content != content {
            ledger.ignored.retain(|s| s.content != file.measure.content);
        }
        let standing = |s: &ShelfId| self.shelf(*s).is_some();
        let target =
            (stone.shelf.filter(standing)).or(ledger.shelf_map.get("").copied().filter(standing));
        self.folders.insert(folder, ledger);
        if let Some(shelf) = target {
            self.file(id, shelf, wall_ms);
        }
        self.mirror(id);
        self.index();
        self.bump();
        Some(id)
    }

    /// What a folder's Add menu offers back: removals, then moves.
    pub fn recoverables(&self, folder: FolderId) -> Vec<ledger::Recovered> {
        let Some(folder) = self.folders.get(&folder) else {
            return Vec::new();
        };
        let held = |b: &Book| {
            let shelves = self
                .shelves_of(b.id)
                .into_iter()
                .filter_map(|s| self.shelf(s));
            let shelves =
                shelves.map(|s| (s.name.value.clone(), s.kind.value.rung().map(|(f, _)| f)));
            ledger::Held {
                id: b.id,
                title: b.title.value.clone(),
                missing: b.missing(),
                shelves: shelves.collect(),
            }
        };
        let held: HashMap<_, _> = (self.books().filter(|b| b.shared()))
            .map(|b| (b.content, held(b)))
            .collect();
        ledger::recoverables(folder, &held)
    }
}

/// The nearest living rung above `key`, by the folder's own keys.
fn rung_above(rungs: &HashMap<String, ShelfId>, key: &str) -> Option<ShelfId> {
    let mut at = crate::folder::parent_key(key)?;
    loop {
        if let Some(id) = rungs.get(at) {
            return Some(*id);
        }
        at = crate::folder::parent_key(at)?;
    }
}
