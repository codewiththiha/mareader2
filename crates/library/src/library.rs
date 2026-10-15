//! The library: books by id, their reading states, and what changed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::book::{Book, FileStamp, Format, Location, Origin, Position, State};
use crate::clock::{Clock, Field, Stamp};
use crate::id::{BookId, ContentId, DeviceId, ShelfId, SourceId};
use crate::shelves::{Membership, Shelf};
use crate::sources::Source;

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

/// What must reach disk since the last save.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dirty {
    /// `library.json`: books and their locations.
    pub library: bool,
    /// `state/<id>.json`: one book's position each.
    pub states: BTreeSet<BookId>,
}

impl Dirty {
    pub fn is_empty(&self) -> bool {
        !self.library && self.states.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Library {
    pub(crate) clock: Clock,
    pub(crate) books: BTreeMap<BookId, Book>,
    pub(crate) states: BTreeMap<BookId, State>,
    pub(crate) sources: BTreeMap<SourceId, Source>,
    pub(crate) shelves: BTreeMap<ShelfId, Shelf>,
    pub(crate) members: BTreeMap<(ShelfId, BookId), Membership>,
    /// Each location's path to its book; rebuilt, never saved.
    pub(crate) at: HashMap<PathBuf, BookId>,
    pub(crate) dirty: Dirty,
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
        let clock = Clock::new(device);
        Self::from_parts(clock, BTreeMap::new(), BTreeMap::new(), BTreeMap::new())
    }

    pub(crate) fn from_parts(
        clock: Clock,
        books: BTreeMap<BookId, Book>,
        states: BTreeMap<BookId, State>,
        sources: BTreeMap<SourceId, Source>,
    ) -> Self {
        let mut library = Self {
            clock,
            books,
            states,
            sources,
            shelves: BTreeMap::new(),
            members: BTreeMap::new(),
            at: HashMap::new(),
            dirty: Dirty::default(),
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
        self.touch();
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
            for source in self.sources.values_mut() {
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
        let twins = self
            .books
            .values_mut()
            .filter(|b| b.twin_of == Some(original));
        for twin in twins {
            twin.content = content;
            twin.format = format;
            twin.locations.clone_from(&locations);
        }
    }

    /// Mirrors every original that has twins.
    pub(crate) fn mirror_all(&mut self) {
        let originals: BTreeSet<BookId> = self.books.values().filter_map(|b| b.twin_of).collect();
        for original in originals {
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
        self.dirty.states.insert(id);
        true
    }

    /// Marks each location there or not; done after loading.
    pub fn check_present(&mut self, exists: impl Fn(&Path) -> bool) {
        for book in self.books.values_mut() {
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
        if let Some(book) = self.books.get_mut(&original) {
            for location in &mut book.locations {
                location.present = exists(&location.path);
            }
        }
        self.mirror(original);
        self.bump();
    }

    /// Changes whenever books, shelves or folders do, not positions.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// A change `library.json` must keep, seen by [`Self::revision`].
    pub(crate) fn touch(&mut self) {
        self.dirty.library = true;
        self.bump();
    }

    /// A change worth showing that `library.json` needn't keep.
    pub(crate) fn bump(&mut self) {
        self.revision = next_revision();
    }

    /// What changed since the last call, now to be written.
    pub fn take_dirty(&mut self) -> Dirty {
        std::mem::take(&mut self.dirty)
    }

    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }
}
