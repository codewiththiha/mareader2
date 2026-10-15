//! Removing books, bringing them back, twins, renames and store copies.

use std::collections::{BTreeSet, HashMap};
use std::hash::{BuildHasher, RandomState};
use std::path::PathBuf;

use crate::book::{Book, Origin};
use crate::clock::{Field, Stamp};
use crate::id::{BookId, SourceId};
use crate::library::{Library, Opened};
use crate::shelves::{Membership, clean};

/// What removing some books will do, told before it's done.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Receipt {
    pub books: usize,
    /// Books with a reading position, kept should they return.
    pub positions: usize,
    /// Spots on shelves, kept should they return.
    pub spots: usize,
    /// Copies in the library's own folder, deleted.
    pub copies: usize,
    /// Books in folders read in place: hidden there, restorable.
    pub hidden: usize,
}

/// What a removal did; `delete` are store copies to delete now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Removed {
    pub books: usize,
    pub delete: Vec<PathBuf>,
}

/// What became of a store copy taken in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Imported {
    /// A new book, or a removed one back.
    New(BookId),
    /// Its content is already this book; the copy isn't needed.
    Known(BookId),
}

/// What removing one book does to its files; nothing if a twin stays.
struct Fate {
    copies: Vec<PathBuf>,
    folders: BTreeSet<SourceId>,
}

impl Library {
    /// Each book's fate when all of `removing` go together.
    fn fates(&self, removing: &BTreeSet<BookId>) -> Vec<(BookId, Fate)> {
        // A family's living members that stay: their files stay too.
        let mut staying: HashMap<BookId, usize> = HashMap::new();
        for book in self.books().filter(|b| !removing.contains(&b.id)) {
            *staying.entry(book.original()).or_default() += 1;
        }
        let books = removing.iter().filter_map(|id| self.book(*id));
        let fate = |book: &Book| {
            let mut fate = Fate {
                copies: Vec::new(),
                folders: BTreeSet::new(),
            };
            if staying.contains_key(&book.original()) {
                return fate;
            }
            for location in &book.locations {
                match location.origin {
                    Origin::Store => fate.copies.push(location.path.clone()),
                    Origin::Source(id) => {
                        fate.folders.insert(id);
                    }
                    Origin::Opened => {}
                }
            }
            fate
        };
        books.map(|b| (b.id, fate(b))).collect()
    }

    fn living(&self, ids: &[BookId]) -> BTreeSet<BookId> {
        let ids = ids.iter().copied();
        ids.filter(|id| self.book(*id).is_some()).collect()
    }

    /// What removing `ids` would do, for the reader to confirm.
    pub fn receipt(&self, ids: &[BookId]) -> Receipt {
        let removing = self.living(ids);
        let mut receipt = Receipt::default();
        for (id, fate) in self.fates(&removing) {
            receipt.books += 1;
            receipt.positions += usize::from(self.position(id).is_some_and(|p| p.page > 0));
            receipt.copies += usize::from(!fate.copies.is_empty());
            receipt.hidden += usize::from(!fate.folders.is_empty());
        }
        let spot = |m: &&Membership| m.alive.value && removing.contains(&m.book);
        let spots = self.members.values().filter(spot);
        receipt.spots = spots.filter(|m| self.shelf(m.shelf).is_some()).count();
        receipt
    }

    /// Removes books: folder files are hidden, store copies listed to delete.
    pub fn remove_books(&mut self, ids: &[BookId], wall_ms: u64) -> Removed {
        let removing = self.living(ids);
        let at = self.clock.tick(wall_ms);
        let mut done = Removed::default();
        for (id, fate) in self.fates(&removing) {
            for source in &fate.folders {
                if let Some(source) = self.sources.get_mut(source) {
                    source.hidden.insert(id);
                }
            }
            done.delete.extend(fate.copies);
            if let Some(book) = self.books.get_mut(&id) {
                // Shelf spots and position stay, for a return.
                book.removed = Some(at);
                done.books += 1;
            }
        }
        self.promote_twins(&removing);
        self.index();
        self.touch();
        done
    }

    /// Each removed original's first living twin takes its place.
    fn promote_twins(&mut self, removed: &BTreeSet<BookId>) {
        let mut heirs: HashMap<BookId, BookId> = HashMap::new();
        for book in self.books() {
            if let Some(original) = book.twin_of.filter(|o| removed.contains(o)) {
                heirs.entry(original).or_insert(book.id);
            }
        }
        for book in self.books.values_mut() {
            let Some(&heir) = book.twin_of.and_then(|o| heirs.get(&o)) else {
                continue;
            };
            book.twin_of = (book.id != heir).then_some(heir);
        }
    }

    /// Books removed from folder `id`, by title: these can come back.
    pub fn hidden_in(&self, id: SourceId) -> Vec<&Book> {
        let Some(source) = self.sources.get(&id) else {
            return Vec::new();
        };
        let books = source.hidden.iter().filter_map(|b| self.books.get(b));
        let mut out: Vec<&Book> = books.filter(|b| !b.alive()).collect();
        out.sort_by(|a, b| crate::natural(&a.title.value, &b.title.value));
        out
    }

    /// Brings a removed book back, place and shelves and all.
    pub fn restore(&mut self, id: BookId, wall_ms: u64) -> bool {
        let Some(content) = self
            .books
            .get(&id)
            .filter(|b| !b.alive())
            .map(|b| b.content)
        else {
            return false;
        };
        let at = self.clock.tick(wall_ms);
        for source in self.sources.values_mut() {
            source.hidden.remove(&id);
        }
        // Its content may have come back meanwhile: then it reads beside it.
        let holder = |b: &&Book| b.content == content && b.twin_of.is_none();
        let living = self.books().find(holder).map(|b| b.id);
        if let Some(book) = self.books.get_mut(&id) {
            book.restored = Some(at);
            book.twin_of = living;
        }
        if let Some(original) = living {
            self.mirror(original);
        }
        self.index();
        self.touch();
        true
    }

    /// A separate book of the same files, with its own place and shelves.
    pub fn twin(&mut self, id: BookId, wall_ms: u64) -> Option<BookId> {
        let book = self.book(id)?;
        let original = self.book(book.original())?.clone();
        let title = self.copy_title(&book.title.value);
        let at = self.clock.tick(wall_ms);
        let twin = self.new_book_id(at);
        let book = Book {
            id: twin,
            title: Field::new(title, at),
            author: Field::new(original.author.value.clone(), at),
            added: wall_ms,
            opened: Field::new(0, at),
            removed: None,
            restored: None,
            twin_of: Some(original.id),
            ..original
        };
        self.books.insert(twin, book);
        // Right behind the book pointed at, on each of its shelves.
        for shelf in self.shelves_of(id) {
            let order = self.shelf_books(shelf);
            let next = order.iter().skip_while(|b| b.id != id).nth(1);
            let before = next.map(|b| b.id);
            self.place_before(twin, shelf, before, wall_ms);
        }
        self.touch();
        Some(twin)
    }

    /// `Title (n)` with the lowest `n` from 2 no living book has.
    fn copy_title(&self, title: &str) -> String {
        let base = strip_number(title);
        let taken = |name: &str| self.books().any(|b| b.title.value == name);
        let mut n = 2_u32;
        loop {
            let name = format!("{base} ({n})");
            if !taken(&name) || n == u32::MAX {
                return name;
            }
            n += 1;
        }
    }

    fn new_book_id(&self, at: Stamp) -> BookId {
        let state = RandomState::new();
        let seed = (at.ms, at.counter, self.device().0, "book");
        let half = |salt: u8| u128::from(state.hash_one((seed, salt)));
        let mut id = BookId(half(1) << 64 | half(2));
        while self.books.contains_key(&id) {
            id = BookId(id.0.wrapping_add(1));
        }
        id
    }

    /// Renames a book; false if blank or the same.
    pub fn rename_book(&mut self, id: BookId, title: &str, wall_ms: u64) -> bool {
        let Some(title) = clean(title) else {
            return false;
        };
        if self.book(id).is_none_or(|b| b.title.value == title) {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        if let Some(book) = self.books.get_mut(&id) {
            book.title = Field::new(title, at);
        }
        self.touch();
        true
    }

    /// Takes in a file copied into the library's folder.
    pub fn import(&mut self, copy: Opened, wall_ms: u64) -> Imported {
        self.take_in(copy, wall_ms, Origin::Store)
    }

    /// Takes in a file read where it is; a known one gains the place.
    pub fn link(&mut self, file: Opened, wall_ms: u64) -> Imported {
        let origin = self.origin_of(&file.path);
        self.take_in(file, wall_ms, origin)
    }

    fn take_in(&mut self, file: Opened, wall_ms: u64, origin: Origin) -> Imported {
        let content = file.measure.content;
        let holder = |b: &&Book| b.content == content && b.twin_of.is_none();
        let known = self.books().find(holder).map(|b| b.id);
        if let Some(id) = known {
            // A copy is the library's to drop; a file in place is a new place.
            if origin != Origin::Store && self.book_at(&file.path) != Some(id) {
                self.place(id, &file.path, file.measure.stamp, origin);
                self.mirror(id);
                self.touch();
            }
            return Imported::Known(id);
        }
        let at = self.clock.tick(wall_ms);
        let id = self.add(&file, wall_ms, at, false);
        self.place(id, &file.path, file.measure.stamp, origin);
        self.mirror(id);
        self.index();
        self.touch();
        Imported::New(id)
    }

    /// Whether a living book still lists `path`.
    pub fn listed(&self, path: &std::path::Path) -> bool {
        self.books()
            .any(|b| b.locations.iter().any(|l| l.path == path))
    }
}

/// `Title (3)` is `Title`; other titles stay.
fn strip_number(title: &str) -> &str {
    let Some(open) = title.strip_suffix(')').and_then(|t| t.rfind(" (")) else {
        return title;
    };
    let digits = title.get(open + 2..title.len() - 1).unwrap_or_default();
    let numbered = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    match numbered {
        true => title.get(..open).unwrap_or(title),
        false => title,
    }
}
