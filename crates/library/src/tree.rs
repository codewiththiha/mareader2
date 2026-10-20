//! Whole-shelf work: taking apart, removing with what's inside, links
//! and duplicates.

use std::collections::{BTreeSet, HashMap};

use crate::book::{Book, Origin, State};
use crate::clock::Field;
use crate::conflict::Level;
use crate::extra::Extra;
use crate::ground;
use crate::id::{BookId, LinkId, ShelfId};
use crate::library::Library;
use crate::link::{Link, LinkTo};
use crate::marks::Mark;
use crate::removal::Data;
use crate::run::Copied;
use crate::shelves::ShelfKind;
use crate::words::free_name;

/// What duplicating a shelf made, by kind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Duplicated {
    pub shelf: Option<ShelfId>,
    pub shelves: usize,
    pub books: usize,
    pub links: usize,
}

impl Library {
    /// Every shelf below `roots`, at any depth.
    pub fn subtree(&self, roots: &[ShelfId]) -> Vec<ShelfId> {
        ground::subtree(&self.shelves, roots)
    }

    /// Books on `shelves`, each once, in no order.
    pub(crate) fn books_on(&self, shelves: &[ShelfId]) -> Vec<BookId> {
        let on = |s: &ShelfId| {
            self.shelf_books(*s)
                .into_iter()
                .map(|b| b.id)
                .collect::<Vec<_>>()
        };
        let all: BTreeSet<BookId> = shelves.iter().flat_map(on).collect();
        all.into_iter().collect()
    }

    /// Removes shelves deepest first; `inside` takes what's on and below.
    pub fn remove_shelves(
        &mut self,
        ids: &[ShelfId],
        inside: bool,
        data: Data,
        wall_ms: u64,
    ) -> usize {
        let going = self.removing(&[], ids, inside);
        let done = self.remove_books(&going.books, data, wall_ms);
        for id in &going.shelves {
            self.remove_shelf(*id, wall_ms);
        }
        done
    }

    /// A link at a book or a shelf, on `level`, named as its target.
    pub fn link_to(&mut self, target: LinkTo, level: Level, wall_ms: u64) -> Option<LinkId> {
        let name = match target {
            LinkTo::Book(b) => self.book(b)?.title.value.clone(),
            LinkTo::Shelf(s) => self.shelf(s)?.name.value.clone(),
        };
        if level.is_some_and(|l| self.shelf(l).is_none()) {
            return None;
        }
        let at = self.clock.tick(wall_ms);
        let id = self.new_link_id(at);
        let key = self.next_key(level);
        let link = Link {
            id,
            name: Field::new(name, at),
            target,
            level: Field::new(level, at),
            key: Field::new(key, at),
            added: wall_ms,
            removed: None,
            extra: Extra::default(),
        };
        self.links.insert(id, link);
        self.bump();
        Some(id)
    }

    pub fn link(&self, id: LinkId) -> Option<&Link> {
        self.links.get(&id).filter(|l| l.alive())
    }

    pub fn links(&self) -> impl Iterator<Item = &Link> {
        self.links.values().filter(|l| l.alive())
    }

    pub fn rename_link(&mut self, id: LinkId, name: &str, wall_ms: u64) -> bool {
        let Some(name) = crate::shelves::clean(name) else {
            return false;
        };
        if self.link(id).is_none_or(|l| l.name.value == name) {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        if let Some(link) = self.links.get_mut(&id) {
            link.name = Field::new(name, at);
        }
        self.bump();
        true
    }

    fn new_link_id(&self, at: crate::clock::Stamp) -> LinkId {
        use std::hash::{BuildHasher, RandomState};
        let state = RandomState::new();
        let seed = (at.ms, at.counter, self.device().0, "link");
        let half = |salt: u8| u128::from(state.hash_one((seed, salt)));
        let mut id = LinkId(half(1) << 64 | half(2));
        while self.links.contains_key(&id) {
            id = LinkId(id.0.wrapping_add(1));
        }
        id
    }

    /// The names on a level, books, links and shelves alike.
    fn names_on(&self, level: Level) -> Vec<String> {
        let mut names: Vec<String> = self.level_rows(level).into_iter().map(|(_, n)| n).collect();
        names.extend(
            self.children(level)
                .into_iter()
                .map(|s| s.name.value.clone()),
        );
        let links = self.links().filter(|l| l.level.value == level);
        names.extend(links.map(|l| l.name.value.clone()));
        names
    }

    /// The books a duplicate copies: one store copy each, missing aside.
    pub fn duplicate_wants(&self, shelf: ShelfId) -> Vec<BookId> {
        let mut all = vec![shelf];
        all.extend(self.subtree(&[shelf]));
        let books = self.books_on(&all);
        books
            .into_iter()
            .filter(|b| self.book(*b).is_some_and(|b| !b.missing()))
            .collect()
    }

    /// A store copy of a book, its marks and place too, filed beside it.
    pub fn duplicate_book(&mut self, id: BookId, copy: &Copied, wall_ms: u64) -> Option<BookId> {
        let level = self.shelves_of(id).into_iter().next();
        self.duplicate_book_on(id, copy, level, wall_ms)
    }

    /// A book's store copy filed on `level`: where a link to it stood.
    pub fn duplicate_book_on(
        &mut self,
        id: BookId,
        copy: &Copied,
        level: Option<ShelfId>,
        wall_ms: u64,
    ) -> Option<BookId> {
        let book = self.book(id)?.clone();
        let names = self.names_on(level);
        let name = free_name(&book.title.value, names.iter().map(String::as_str));
        let twin = self.copy_book(&book, copy, &name, wall_ms);
        if let Some(shelf) = level {
            self.file(twin, shelf, wall_ms);
        }
        self.bump();
        Some(twin)
    }

    /// A shelf's link again beside it, under the next free name.
    pub fn duplicate_link(&mut self, id: LinkId, wall_ms: u64) -> Option<LinkId> {
        let link = self.link(id)?.clone();
        let level = link.level.value;
        let names = self.names_on(level);
        let name = free_name(&link.name.value, names.iter().map(String::as_str));
        let made = self.link_to(link.target, level, wall_ms)?;
        self.rename_link(made, &name, wall_ms);
        Some(made)
    }

    /// One book's own copy: a new book, its marks and place carried.
    fn copy_book(&mut self, book: &Book, copy: &Copied, name: &str, wall_ms: u64) -> BookId {
        let at = self.clock.tick(wall_ms);
        let id = self.new_book_id(at);
        let key = self.next_key(None);
        let new = Book {
            id,
            title: Field::new(name.to_owned(), at),
            author: Field::new(book.author.value.clone(), at),
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
            ..book.clone()
        };
        self.books.insert(id, new);
        let from = book.home().map(|l| l.path.clone());
        self.place_from(id, &copy.path, copy.measure.stamp, Origin::Store, from);
        if let Some(state) = self.states.get(&book.id) {
            let position = Field::new(state.position.value, at);
            self.states.insert(id, State::new(position));
            self.states_dirty.insert(id);
        }
        let marks: Vec<Mark> = (self.marks.values())
            .filter(|m| m.alive.value && m.book.value == book.id)
            .cloned()
            .collect();
        for mark in marks {
            let mark_id = self.new_mark_id(at);
            let copy = Mark {
                id: mark_id,
                book: Field::new(id, at),
                alive: Field::new(true, at),
                ..mark
            };
            self.marks.insert(mark_id, copy);
        }
        self.index();
        id
    }

    /// A shelf of the reader's own beside it, every book inside copied.
    pub fn duplicate_shelf(
        &mut self,
        id: ShelfId,
        copies: &HashMap<BookId, Copied>,
        wall_ms: u64,
    ) -> Duplicated {
        let mut done = Duplicated::default();
        let Some(shelf) = self.shelf(id) else {
            return done;
        };
        let parent = self.parent_of(shelf);
        let names: Vec<String> = self
            .children(parent)
            .into_iter()
            .map(|s| s.name.value.clone())
            .collect();
        let name = free_name(&shelf.name.value, names.iter().map(String::as_str));
        let mut shelf_map: HashMap<ShelfId, ShelfId> = HashMap::new();
        let mut book_map: HashMap<BookId, BookId> = HashMap::new();
        let top = self.mint_shelf(&name, parent, ShelfKind::Own, false, wall_ms);
        shelf_map.insert(id, top);
        done.shelf = Some(top);
        // Parents before children: a breadth-first walk down the tree.
        let mut queue = vec![id];
        while let Some(old) = queue.pop() {
            let Some(&new) = shelf_map.get(&old) else {
                continue;
            };
            for book in self
                .shelf_books(old)
                .into_iter()
                .map(|b| b.id)
                .collect::<Vec<_>>()
            {
                let made = match book_map.get(&book) {
                    Some(made) => Some(*made),
                    None => self.copy_onto(book, copies, wall_ms),
                };
                if let Some(made) = made {
                    done.books += usize::from(book_map.insert(book, made).is_none());
                    self.file(made, new, wall_ms);
                }
            }
            for child in self
                .children(Some(old))
                .into_iter()
                .map(|s| (s.id, s.name.value.clone()))
                .collect::<Vec<_>>()
            {
                let made = self.mint_shelf(&child.1, Some(new), ShelfKind::Own, false, wall_ms);
                shelf_map.insert(child.0, made);
                done.shelves += 1;
                queue.push(child.0);
            }
        }
        // Links inside the tree point at the copies, else stay as they were.
        let links: Vec<Link> = (self.links.values())
            .filter(|l| l.alive() && l.level.value.is_some_and(|s| shelf_map.contains_key(&s)))
            .cloned()
            .collect();
        for link in links {
            let target = match link.target {
                LinkTo::Book(b) => book_map
                    .get(&b)
                    .map(|b| LinkTo::Book(*b))
                    .or(Some(link.target)),
                LinkTo::Shelf(s) => {
                    Some(shelf_map.get(&s).map_or(link.target, |s| LinkTo::Shelf(*s)))
                }
            };
            let level = link.level.value.and_then(|s| shelf_map.get(&s).copied());
            let alive = target.is_some_and(|t| match t {
                LinkTo::Book(b) => self.book(b).is_some(),
                LinkTo::Shelf(s) => self.shelf(s).is_some(),
            });
            if let (Some(target), Some(level), true) = (target, level, alive)
                && self.link_to(target, Some(level), wall_ms).is_some()
            {
                done.links += 1;
            }
        }
        self.bump();
        done
    }

    fn copy_onto(
        &mut self,
        book: BookId,
        copies: &HashMap<BookId, Copied>,
        wall_ms: u64,
    ) -> Option<BookId> {
        let copy = copies.get(&book)?;
        let original = self.book(book)?.clone();
        let name = original.title.value.clone();
        Some(self.copy_book(&original, copy, &name, wall_ms))
    }
}
