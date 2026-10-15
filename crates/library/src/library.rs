//! The library: books by id, their reading states, and what changed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::book::{Book, FileStamp, Format, Location, Origin, Position, State};
use crate::clock::{Clock, Field, Stamp};
use crate::id::{BookId, ContentId, DeviceId, ShelfId, SourceId};
use crate::shelves::{Membership, Shelf};
use crate::sources::Source;
use crate::tracked::Tracked;

/// What a measured file is: its content and its size and time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measure {
    pub content: ContentId,
    pub stamp: FileStamp,
}

/// A file the reader opened, measured, with what the document says.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    pub path: PathBuf,
    pub measure: Measure,
    pub format: Format,
    pub title: String,
    /// The document's own author, if it names one.
    pub author: Option<String>,
}

/// What must reach disk since the last save: one row or file each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dirty {
    pub books: BTreeSet<BookId>,
    pub shelves: BTreeSet<ShelfId>,
    pub members: BTreeSet<(ShelfId, BookId)>,
    /// Gone ones included: their rows go too.
    pub sources: BTreeSet<SourceId>,
    /// `state/<id>.json`: one book's position each.
    pub states: BTreeSet<BookId>,
}

impl Dirty {
    pub fn is_empty(&self) -> bool {
        self.store_is_clean() && self.states.is_empty()
    }

    /// Nothing for the store: only positions changed.
    pub fn store_is_clean(&self) -> bool {
        self.books.is_empty()
            && self.shelves.is_empty()
            && self.members.is_empty()
            && self.sources.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Library {
    pub(crate) clock: Clock,
    pub(crate) books: Tracked<BookId, Book>,
    pub(crate) states: BTreeMap<BookId, State>,
    pub(crate) sources: Tracked<SourceId, Source>,
    pub(crate) shelves: Tracked<ShelfId, Shelf>,
    pub(crate) members: Tracked<(ShelfId, BookId), Membership>,
    /// Each location's path to its book; rebuilt, never saved.
    pub(crate) at: HashMap<PathBuf, BookId>,
    /// Positions to write; the maps track their own changes.
    pub(crate) states_dirty: BTreeSet<BookId>,
    /// Changes whenever a book, shelf or folder does; never repeats.
    revision: u64,
}

/// Unique across libraries, so a loaded one never reuses a revision.
fn next_revision() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Library {
    /// An empty library written by `device`.
    pub fn new(device: DeviceId) -> Self {
        Self::from_parts(Parts::new(Clock::new(device)))
    }

    /// A library as it was saved: nothing in it needs writing.
    pub(crate) fn from_parts(parts: Parts) -> Self {
        let mut library = Self {
            clock: parts.clock,
            books: Tracked::new(parts.books),
            states: parts.states,
            sources: Tracked::new(parts.sources),
            shelves: Tracked::new(parts.shelves),
            members: Tracked::new(parts.members),
            at: HashMap::new(),
            states_dirty: BTreeSet::new(),
            revision: next_revision(),
        };
        library.index();
        library
    }

    pub(crate) fn index(&mut self) {
        self.at.clear();
        // A twin shares its original's files; paths name the original.
        let originals = self
            .books
            .values()
            .filter(|b| b.alive() && b.twin_of.is_none());
        for book in originals {
            for location in &book.locations {
                self.at.insert(location.path.clone(), book.id);
            }
        }
    }

    pub fn device(&self) -> DeviceId {
        self.clock.device()
    }

    /// Every book not removed, in no particular order.
    pub fn books(&self) -> impl Iterator<Item = &Book> {
        self.books.values().filter(|b| b.alive())
    }

    pub fn len(&self) -> usize {
        self.books().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn book(&self, id: BookId) -> Option<&Book> {
        self.books.get(&id).filter(|b| b.alive())
    }

    /// The book with a location at `path`, if any.
    pub fn book_at(&self, path: &Path) -> Option<BookId> {
        self.at.get(path).copied()
    }

    pub fn position(&self, id: BookId) -> Option<Position> {
        self.states.get(&id).map(|s| s.position.value)
    }

    /// The books, last opened first; ties by title.
    pub fn recent(&self) -> Vec<&Book> {
        let mut books: Vec<&Book> = self.books().collect();
        books.sort_by(|a, b| {
            let newer = b.opened.value.cmp(&a.opened.value);
            newer.then_with(|| a.title.value.cmp(&b.title.value))
        });
        books
    }

    /// Records an open: the book of that content or path, else a new one.
    pub fn record_open(&mut self, opened: Opened, wall_ms: u64) -> BookId {
        self.record_open_as(opened, wall_ms, None)
    }

    /// As [`Self::record_open`]; a twin of that book read is the one opened.
    pub fn record_open_as(&mut self, opened: Opened, wall_ms: u64, twin: Option<BookId>) -> BookId {
        let at = self.clock.tick(wall_ms);
        let content = opened.measure.content;
        let original = |b: &&Book| b.content == content && b.twin_of.is_none();
        let by_content = self.books().find(original).map(|b| b.id);
        // A file edited in place is the same book with new content.
        let id = by_content.or_else(|| self.book_at(&opened.path));
        let twin_of = |t: &BookId| {
            self.book(*t)
                .is_some_and(|b| id.is_some() && b.twin_of == id)
        };
        let reader = twin.filter(twin_of);
        let id = match id.and_then(|id| self.books.get_mut(&id)) {
            Some(book) => {
                book.content = content;
                book.format = opened.format;
                if reader.is_none() {
                    book.opened = Field::new(wall_ms, at);
                }
                // Filled once; a reader's own edit outlasts reopening.
                if book.author.value.is_none() && opened.author.is_some() {
                    book.author = Field::new(opened.author.clone(), at);
                }
                book.id
            }
            None => self.add(&opened, wall_ms, at, true),
        };
        let origin = self.origin_of(&opened.path);
        self.place(id, &opened.path, opened.measure.stamp, origin);
        self.mirror(id);
        if let Some(book) = reader.and_then(|t| self.books.get_mut(&t)) {
            book.opened = Field::new(wall_ms, at);
        }
        self.bump();
        id
    }

    /// A new book, or a removed one back; found unopened, it reads 0.
    pub(crate) fn add(
        &mut self,
        opened: &Opened,
        wall_ms: u64,
        at: Stamp,
        opening: bool,
    ) -> BookId {
        let id = BookId::original(opened.measure.content);
        let last = if opening { wall_ms } else { 0 };
        let book = self.books.entry(id).or_insert_with(|| Book {
            id,
            content: opened.measure.content,
            format: opened.format,
            title: Field::new(opened.title.clone(), at),
            author: Field::new(opened.author.clone(), at),
            added: wall_ms,
            opened: Field::new(last, at),
            locations: Vec::new(),
            removed: None,
            restored: None,
            twin_of: None,
        });
        if !book.alive() {
            book.restored = Some(at);
            if opening {
                book.opened = Field::new(wall_ms, at);
            }
            // Back on purpose: folders no longer leave it out.
            let hiding = self.sources.values_mut_where(|s| s.hidden.contains(&id));
            for source in hiding {
                source.hidden.remove(&id);
            }
        }
        id
    }

    /// Gives `original`'s twins its files, content and format again.
    pub(crate) fn mirror(&mut self, original: BookId) {
        let Some(book) = self.books.get(&original) else {
            return;
        };
        let (content, format) = (book.content, book.format);
        let locations = book.locations.clone();
        let twins = self.books.values_mut_where(|b| b.twin_of == Some(original));
        for twin in twins {
            twin.content = content;
            twin.format = format;
            twin.locations.clone_from(&locations);
        }
    }

    /// Mirrors each original with twins that changed since the last save.
    pub(crate) fn mirror_changed(&mut self) {
        let originals: BTreeSet<BookId> = self.books.values().filter_map(|b| b.twin_of).collect();
        let changed = self.books.changed();
        let changed: Vec<BookId> = originals.intersection(changed).copied().collect();
        for original in changed {
            self.mirror(original);
        }
    }

    /// Makes the path the book's newest location, and only its own.
    pub(crate) fn place(&mut self, id: BookId, path: &Path, stamp: FileStamp, origin: Origin) {
        if let Some(old) = self.at.insert(path.to_path_buf(), id)
            && old != id
            && let Some(other) = self.books.get_mut(&old)
        {
            other.locations.retain(|l| l.path != path);
        }
        let Some(book) = self.books.get_mut(&id) else {
            return;
        };
        book.locations.retain(|l| l.path != path);
        let location = Location {
            path: path.to_path_buf(),
            origin,
            stamp,
            present: true,
        };
        book.locations.insert(0, location);
    }

    /// Writes down where the reader is; false if nothing changed.
    pub fn record_position(&mut self, id: BookId, position: Position, wall_ms: u64) -> bool {
        if self.book(id).is_none() || self.position(id) == Some(position) {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        let field = Field::new(position, at);
        self.states.insert(id, State { position: field });
        self.states_dirty.insert(id);
        true
    }

    /// Marks each location there or not; done after loading.
    pub fn check_present(&mut self, exists: impl Fn(&Path) -> bool) {
        for book in self.books.quiet_values_mut() {
            for location in &mut book.locations {
                location.present = exists(&location.path);
            }
        }
        self.bump();
    }

    /// Looks again for one book's files, and its twins'.
    pub fn recheck(&mut self, id: BookId, exists: impl Fn(&Path) -> bool) {
        let Some(original) = self.books.get(&id).map(Book::original) else {
            return;
        };
        if let Some(book) = self.books.quiet_get_mut(&original) {
            for location in &mut book.locations {
                location.present = exists(&location.path);
            }
        }
        let twins = self.books.values().filter(|b| b.twin_of == Some(original));
        let twins: Vec<BookId> = twins.map(|b| b.id).collect();
        let locations = self.books.get(&original).map(|b| b.locations.clone());
        let locations = locations.unwrap_or_default();
        for twin in &twins {
            if let Some(twin) = self.books.quiet_get_mut(twin) {
                twin.locations.clone_from(&locations);
            }
        }
        self.bump();
    }

    /// Changes whenever books, shelves or folders do, not positions.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// A change worth showing, seen by [`Self::revision`].
    pub(crate) fn bump(&mut self) {
        self.revision = next_revision();
    }

    /// What changed since the last call, now to be written.
    pub fn take_dirty(&mut self) -> Dirty {
        Dirty {
            books: self.books.take_changed(),
            shelves: self.shelves.take_changed(),
            members: self.members.take_changed(),
            sources: self.sources.take_changed(),
            states: std::mem::take(&mut self.states_dirty),
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.books.has_changes()
            || self.shelves.has_changes()
            || self.members.has_changes()
            || self.sources.has_changes()
            || !self.states_dirty.is_empty()
    }

    /// Marks every row, so the next save writes them all; not positions.
    pub(crate) fn mark_all(&mut self) {
        self.books.mark_all();
        self.shelves.mark_all();
        self.members.mark_all();
        self.sources.mark_all();
    }
}

/// What a saved library is made of, read back from disk.
#[derive(Debug)]
pub(crate) struct Parts {
    pub clock: Clock,
    pub books: BTreeMap<BookId, Book>,
    pub states: BTreeMap<BookId, State>,
    pub sources: BTreeMap<SourceId, Source>,
    pub shelves: BTreeMap<ShelfId, Shelf>,
    pub members: BTreeMap<(ShelfId, BookId), Membership>,
}

impl Parts {
    pub fn new(clock: Clock) -> Self {
        Self {
            clock,
            books: BTreeMap::new(),
            states: BTreeMap::new(),
            sources: BTreeMap::new(),
            shelves: BTreeMap::new(),
            members: BTreeMap::new(),
        }
    }
}
