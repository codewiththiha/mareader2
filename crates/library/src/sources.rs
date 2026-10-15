//! Folders read in place: what counts in them, scans, and relinking.

use std::collections::{BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::book::{Book, FileStamp, Format, Location, Origin, display_title};
use crate::id::{BookId, ContentId, SourceId};
use crate::library::{Library, Measure, Opened};

/// A folder the library reads in place, and what counts as a book in it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    /// Canonical; every location under it is the folder's.
    pub root: PathBuf,
    pub formats: BTreeSet<Format>,
    /// Smaller files are skipped: stubs and placeholders.
    #[serde(default)]
    pub min_size: u64,
    /// Rescanned at launch and whenever the window comes back.
    pub watch: bool,
    /// When it was added, in wall milliseconds.
    pub added: u64,
    /// Books removed from it, which rescans leave out.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden: BTreeSet<BookId>,
}

impl Source {
    /// The folder's own name, as shown.
    pub fn name(&self) -> String {
        match self.root.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => self.root.display().to_string(),
        }
    }

    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }

    /// Whether a file of `format` and `size` counts as a book here.
    pub fn counts(&self, format: Format, size: u64) -> bool {
        self.formats.contains(&format) && size >= self.min_size.max(1)
    }
}

/// What a folder's settings may be changed to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEdit {
    pub formats: BTreeSet<Format>,
    pub min_size: u64,
    pub watch: bool,
}

/// Why a folder can't be added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddError {
    /// The same as, or inside, a folder already read: that folder's name.
    Inside(String),
    /// A whole disk is no folder of books.
    Disk,
}

/// A folder added, and the names of folders inside it it took over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub id: SourceId,
    pub absorbed: Vec<String>,
}

/// A file a walk found and measured, and what it says of itself.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    pub format: Format,
    pub measure: Measure,
    /// Read only for content the library hasn't met.
    pub title: Option<String>,
    pub author: Option<String>,
}

/// One folder's walk.
#[derive(Clone, Debug, PartialEq)]
pub struct Scan {
    pub source: SourceId,
    pub found: Vec<Found>,
    /// The folder couldn't be read: an unplugged drive, a deleted folder.
    pub offline: bool,
}

/// What a scan changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub added: Vec<BookId>,
    /// Found at a new path after the old one went.
    pub followed: Vec<BookId>,
    /// The same path with new bytes: edited in place.
    pub edited: Vec<BookId>,
    /// Removed once, found again.
    pub back: Vec<BookId>,
}

impl ScanReport {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.followed.is_empty()
            && self.edited.is_empty()
            && self.back.is_empty()
    }
}

/// Why a file can't stand in for a book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelinkError {
    Removed,
    /// The file is already this other book.
    OtherBook(String),
    /// Different content in another format.
    Format {
        file: Format,
        book: Format,
    },
    /// Different content: another edition, or an edit. Asked first.
    Differs,
}

impl Library {
    pub fn all_books(&self) -> impl Iterator<Item = &Book> {
        self.books.values()
    }

    pub fn sources(&self) -> impl Iterator<Item = &Source> {
        self.sources.values()
    }

    pub fn source(&self, id: SourceId) -> Option<&Source> {
        self.sources.get(&id)
    }

    /// The folder holding `path`; folders never nest.
    pub fn source_of(&self, path: &Path) -> Option<&Source> {
        self.sources.values().find(|s| s.contains(path))
    }

    pub(crate) fn origin_of(&self, path: &Path) -> Origin {
        self.source_of(path)
            .map_or(Origin::Opened, |s| Origin::Source(s.id))
    }

    /// Books with a file in folder `id`.
    pub fn books_in(&self, id: SourceId) -> impl Iterator<Item = &Book> {
        let here = move |b: &&Book| b.locations.iter().any(|l| l.origin == Origin::Source(id));
        self.books().filter(here)
    }

    /// Reads `root` in place from now on; a folder inside takes no part.
    pub fn add_source(&mut self, root: PathBuf, wall_ms: u64) -> Result<Added, AddError> {
        if root.parent().is_none() {
            return Err(AddError::Disk);
        }
        if let Some(outer) = self.source_of(&root) {
            return Err(AddError::Inside(outer.name()));
        }
        let id = self.new_source_id(&root, wall_ms);
        let inner: Vec<SourceId> = (self.sources.values())
            .filter(|s| s.root.starts_with(&root))
            .map(|s| s.id)
            .collect();
        let mut source = Source {
            id,
            root,
            formats: Format::ALL.into(),
            min_size: 0,
            watch: true,
            added: wall_ms,
            hidden: BTreeSet::new(),
        };
        let mut absorbed = Vec::new();
        for old in inner.iter().filter_map(|i| self.sources.remove(i)) {
            source.hidden.extend(old.hidden.iter().copied());
            absorbed.push(old.name());
        }
        // Files already known under it, opened or found, are now its own.
        let locations = self.books.values_mut().flat_map(|b| &mut b.locations);
        for location in locations.filter(|l| source.contains(&l.path)) {
            location.origin = Origin::Source(id);
        }
        self.sources.insert(id, source);
        self.dirty.library = true;
        Ok(Added { id, absorbed })
    }

    fn new_source_id(&self, root: &Path, wall_ms: u64) -> SourceId {
        let mut hasher = DefaultHasher::new();
        (self.device(), root, wall_ms).hash(&mut hasher);
        let mut id = SourceId(hasher.finish());
        while self.sources.contains_key(&id) {
            id = SourceId(id.0.wrapping_add(1));
        }
        id
    }

    /// Changes what counts in a folder; refused with no format left.
    pub fn edit_source(&mut self, id: SourceId, edit: SourceEdit) -> bool {
        let Some(source) = self.sources.get_mut(&id) else {
            return false;
        };
        if edit.formats.is_empty() {
            return false;
        }
        source.formats = edit.formats;
        source.min_size = edit.min_size;
        source.watch = edit.watch;
        self.dirty.library = true;
        true
    }

    /// Stops reading a folder. Books found only there leave, files stay.
    pub fn remove_source(&mut self, id: SourceId, wall_ms: u64) -> usize {
        if self.sources.remove(&id).is_none() {
            return 0;
        }
        let at = self.clock.tick(wall_ms);
        let mut left = 0;
        let here = |o: Origin| o == Origin::Source(id);
        for book in self.books.values_mut().filter(|b| b.alive()) {
            let only_here =
                !book.locations.is_empty() && book.locations.iter().all(|l| here(l.origin));
            if only_here {
                // A tombstone keeps its place for a later return.
                book.removed = Some(at);
                left += 1;
            } else {
                book.locations.retain(|l| !here(l.origin));
            }
        }
        self.index();
        self.dirty.library = true;
        left
    }

    /// Takes in a folder's walk: new books, moves followed, edits kept.
    pub fn apply_scan(&mut self, scan: Scan, wall_ms: u64) -> ScanReport {
        let mut report = ScanReport::default();
        let Some(source) = self.sources.get(&scan.source).cloned() else {
            return report;
        };
        if scan.offline {
            return report;
        }
        let at = self.clock.tick(wall_ms);
        let origin = Origin::Source(source.id);
        let mut by_content = self.by_content();
        for found in scan.found {
            // Settings may have changed while the walk ran.
            let counts = source.counts(found.format, found.measure.stamp.size);
            if !counts || !source.contains(&found.path) {
                continue;
            }
            let content = found.measure.content;
            let known = by_content.get(&content).copied();
            if known.is_some_and(|id| source.hidden.contains(&id)) {
                continue;
            }
            let stamp = found.measure.stamp;
            let alive = known.filter(|id| self.book(*id).is_some());
            let id = match (alive, self.book_at(&found.path)) {
                (Some(id), _) => {
                    if self.follow(id, &found.path) {
                        report.followed.push(id);
                    }
                    id
                }
                // The same path with new bytes: the user's file, edited.
                (None, Some(id)) => {
                    if let Some(book) = self.books.get_mut(&id) {
                        book.content = content;
                        book.format = found.format;
                    }
                    report.edited.push(id);
                    id
                }
                // Removed once: the same record comes back, place and all.
                (None, None) if let Some(id) = known => {
                    if let Some(book) = self.books.get_mut(&id) {
                        book.restored = Some(at);
                    }
                    report.back.push(id);
                    id
                }
                (None, None) => {
                    let title = display_title(found.title.as_deref(), &found.path);
                    let opened = Opened {
                        path: found.path.clone(),
                        measure: found.measure,
                        format: found.format,
                        title,
                        author: found.author.clone(),
                    };
                    let id = self.add(&opened, wall_ms, at, false);
                    report.added.push(id);
                    id
                }
            };
            by_content.insert(content, id);
            if !self.has_location(id, &found.path, stamp, origin) {
                self.place(id, &found.path, stamp, origin);
                self.dirty.library = true;
            }
        }
        // Brought back in place: the paths index them again.
        if !report.back.is_empty() {
            self.index();
            self.dirty.library = true;
        }
        if self.dirty.library {
            self.mirror_all();
        }
        report
    }

    /// Whether `id` already lists this very file, there now.
    fn has_location(&self, id: BookId, path: &Path, stamp: FileStamp, origin: Origin) -> bool {
        let same = |l: &&Location| l.path == path && l.stamp == stamp && l.origin == origin;
        let book = self.books.get(&id);
        book.is_some_and(|b| b.locations.iter().find(same).is_some_and(|l| l.present))
    }

    /// Each content's original book, living ones over removed ones.
    fn by_content(&self) -> HashMap<ContentId, BookId> {
        let mut map = HashMap::new();
        let originals = self.books.values().filter(|b| b.twin_of.is_none());
        let (alive, removed): (Vec<&Book>, Vec<&Book>) = originals.partition(|b| b.alive());
        for book in removed.into_iter().chain(alive) {
            map.insert(book.content, book.id);
        }
        map
    }

    /// Before `id` is found at `path`: the places it left give way.
    fn follow(&mut self, id: BookId, path: &Path) -> bool {
        let Some(book) = self.books.get_mut(&id) else {
            return false;
        };
        if book.locations.iter().any(|l| l.path == path) {
            return false;
        }
        let before = book.locations.len();
        book.locations.retain(|l| l.present);
        book.locations.len() < before
    }

    /// Points a book at `path` after measuring it; `force` takes an edit.
    pub fn relink(
        &mut self,
        id: BookId,
        path: &Path,
        format: Format,
        measure: Measure,
        force: bool,
    ) -> Result<(), RelinkError> {
        // A twin's files are its original's.
        let id = self.book(id).ok_or(RelinkError::Removed)?.original();
        let book = self.book(id).ok_or(RelinkError::Removed)?;
        let same = book.content == measure.content;
        let holds =
            |b: &Book| b.content == measure.content || b.locations.iter().any(|l| l.path == path);
        let other = |b: &&Book| b.original() != id && holds(b);
        if let Some(other) = self.books().find(other) {
            return Err(RelinkError::OtherBook(other.title.value.clone()));
        }
        if !same && format != book.format {
            let book = book.format;
            return Err(RelinkError::Format { file: format, book });
        }
        if !same && !force {
            return Err(RelinkError::Differs);
        }
        let origin = self.origin_of(path);
        if let Some(book) = self.books.get_mut(&id) {
            book.content = measure.content;
            book.format = format;
            book.locations.retain(|l| l.present);
        }
        self.place(id, path, measure.stamp, origin);
        self.mirror(id);
        self.dirty.library = true;
        Ok(())
    }

    /// Books with no file left to open.
    pub fn missing(&self) -> usize {
        self.books().filter(|b| b.missing()).count()
    }
}
