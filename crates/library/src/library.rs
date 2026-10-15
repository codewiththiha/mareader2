//! The library: books by id, their reading states, and what changed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use crate::book::{Book, FileStamp, Format, Location, Origin, Position, State};
use crate::clock::{Clock, Field, Stamp};
use crate::id::{BookId, ContentId, DeviceId};

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
    /// Each location's path to its book; rebuilt, never saved.
    at: HashMap<PathBuf, BookId>,
    dirty: Dirty,
}

impl Library {
    /// An empty library written by `device`.
    pub fn new(device: DeviceId) -> Self {
        Self::from_parts(Clock::new(device), BTreeMap::new(), BTreeMap::new())
    }

    pub(crate) fn from_parts(
        clock: Clock,
        books: BTreeMap<BookId, Book>,
        states: BTreeMap<BookId, State>,
    ) -> Self {
        let mut library = Self {
            clock,
            books,
            states,
            at: HashMap::new(),
            dirty: Dirty::default(),
        };
        library.index();
        library
    }

    fn index(&mut self) {
        self.at.clear();
        for book in self.books.values().filter(|b| b.alive()) {
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
        let at = self.clock.tick(wall_ms);
        let content = opened.measure.content;
        let by_content = self.books().find(|b| b.content == content).map(|b| b.id);
        // A file edited in place is the same book with new content.
        let id = by_content.or_else(|| self.book_at(&opened.path));
        let id = match id.and_then(|id| self.books.get_mut(&id)) {
            Some(book) => {
                book.content = content;
                book.opened = Field::new(wall_ms, at);
                book.id
            }
            None => self.add(&opened, wall_ms, at),
        };
        self.place(id, &opened);
        self.dirty.library = true;
        id
    }

    /// A new book, or a removed one of this content brought back.
    fn add(&mut self, opened: &Opened, wall_ms: u64, at: Stamp) -> BookId {
        let id = BookId::original(opened.measure.content);
        let book = self.books.entry(id).or_insert_with(|| Book {
            id,
            content: opened.measure.content,
            format: opened.format,
            title: Field::new(opened.title.clone(), at),
            author: Field::new(None, at),
            added: wall_ms,
            opened: Field::new(wall_ms, at),
            locations: Vec::new(),
            removed: None,
            restored: None,
        });
        if !book.alive() {
            book.restored = Some(at);
            book.opened = Field::new(wall_ms, at);
        }
        id
    }

    /// Makes the path the book's newest location, and only its own.
    fn place(&mut self, id: BookId, opened: &Opened) {
        if let Some(old) = self.at.insert(opened.path.clone(), id)
            && old != id
            && let Some(other) = self.books.get_mut(&old)
        {
            other.locations.retain(|l| l.path != opened.path);
        }
        let Some(book) = self.books.get_mut(&id) else {
            return;
        };
        let kept = book.locations.iter().position(|l| l.path == opened.path);
        let origin = kept
            .and_then(|i| book.locations.get(i))
            .map_or(Origin::Opened, |l| l.origin);
        if let Some(i) = kept {
            book.locations.remove(i);
        }
        book.locations.insert(
            0,
            Location {
                path: opened.path.clone(),
                origin,
                stamp: opened.measure.stamp,
                present: true,
            },
        );
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
    }

    /// What changed since the last call, now to be written.
    pub fn take_dirty(&mut self) -> Dirty {
        std::mem::take(&mut self.dirty)
    }

    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }
}
