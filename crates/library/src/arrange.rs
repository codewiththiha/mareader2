//! The arranged order: shelves, books and links mixed at each level, by key.

use std::cmp::Ordering;

use crate::clock::{Field, Stamp};
use crate::id::{BookId, LinkId, ShelfId};
use crate::key;
use crate::library::Library;
use crate::view::{Home, natural};

/// A card on one level of the library: a shelf, a book or a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Item {
    Shelf(ShelfId),
    Book(BookId),
    Link(LinkId),
}

impl Library {
    /// A level's shelves and books, as arranged; `home` filters the top.
    pub fn arranged(&self, level: Option<ShelfId>, home: Home) -> Vec<Item> {
        let mut items = self.level(level);
        if level.is_none() && home == Home::Unshelved {
            let shelved = self.shelved();
            items.retain(|i| !matches!(i, Item::Book(b) if shelved.contains(b)));
        }
        items
    }

    /// The name a card shows, if it's still there.
    pub fn item_name(&self, item: Item) -> Option<&str> {
        match item {
            Item::Book(id) => self.book(id).map(|b| b.title.value.as_str()),
            Item::Shelf(id) => self.shelf(id).map(|s| s.name.value.as_str()),
            Item::Link(id) => self.link(id).map(|l| l.name.value.as_str()),
        }
    }

    /// Everything on a level in its order; the top holds every book.
    fn level(&self, level: Option<ShelfId>) -> Vec<Item> {
        let shelves = self.children(level).into_iter().map(|s| Item::Shelf(s.id));
        let links = self.links().filter(|l| l.level.value == level);
        let links = links.map(|l| Item::Link(l.id));
        let mut items: Vec<Item> = match level {
            Some(shelf) => {
                let books = self.shelf_books(shelf).into_iter();
                (shelves.chain(books.map(|b| Item::Book(b.id))))
                    .chain(links)
                    .collect()
            }
            None => (shelves.chain(self.books().map(|b| Item::Book(b.id))))
                .chain(links)
                .collect(),
        };
        items.sort_by(|a, b| self.compare(level, *a, *b));
        items
    }

    /// Unkeyed items, from before arranging, come first as they were.
    fn compare(&self, level: Option<ShelfId>, a: Item, b: Item) -> Ordering {
        let (ka, kb) = (self.key_of(level, a), self.key_of(level, b));
        let keyed = (!ka.is_empty()).cmp(&!kb.is_empty());
        keyed
            .then_with(|| ka.cmp(kb))
            .then_with(|| self.fallback(a, b))
    }

    /// Shelves by name, then books and links by when they came.
    fn fallback(&self, a: Item, b: Item) -> Ordering {
        let added = |item| match item {
            Item::Shelf(_) => None,
            Item::Book(id) => self.books.get(&id).map(|b| b.added),
            Item::Link(id) => self.links.get(&id).map(|l| l.added),
        };
        match (a, b) {
            (Item::Shelf(x), Item::Shelf(y)) => {
                let name = |id| self.shelves.get(&id).map(|s| s.name.value.as_str());
                let (nx, ny) = (name(x).unwrap_or(""), name(y).unwrap_or(""));
                natural(nx, ny).then(x.cmp(&y))
            }
            (Item::Shelf(_), _) => Ordering::Less,
            (_, Item::Shelf(_)) => Ordering::Greater,
            _ => added(a).cmp(&added(b)).then(a.cmp(&b)),
        }
    }

    /// An item's key on `level`; empty if it has none yet.
    fn key_of(&self, level: Option<ShelfId>, item: Item) -> &str {
        let key = match (level, item) {
            (_, Item::Shelf(s)) => self.shelves.get(&s).map(|s| &s.key),
            (None, Item::Book(b)) => self.books.get(&b).map(|b| &b.key),
            (Some(shelf), Item::Book(b)) => self.members.get(&(shelf, b)).map(|m| &m.key),
            (_, Item::Link(l)) => self.links.get(&l).map(|l| &l.key),
        };
        key.map_or("", |k| k.value.as_str())
    }

    fn set_key(&mut self, level: Option<ShelfId>, item: Item, key: String, at: Stamp) {
        let field = Field::new(key, at);
        match (level, item) {
            (_, Item::Shelf(s)) => {
                if let Some(shelf) = self.shelves.get_mut(&s) {
                    shelf.key = field;
                }
            }
            (None, Item::Book(b)) => {
                if let Some(book) = self.books.get_mut(&b) {
                    book.key = field;
                }
            }
            (Some(shelf), Item::Book(b)) => {
                if let Some(m) = self.members.get_mut(&(shelf, b)) {
                    m.key = field;
                }
            }
            (_, Item::Link(l)) => {
                if let Some(link) = self.links.get_mut(&l) {
                    link.key = field;
                }
            }
        }
    }

    /// Moves `items` before `before`, else to the end; false if none moved.
    pub fn arrange(
        &mut self,
        level: Option<ShelfId>,
        items: &[Item],
        before: Option<Item>,
        wall_ms: u64,
    ) -> bool {
        let mut order = self.level(level);
        let moving: Vec<Item> = items
            .iter()
            .copied()
            .filter(|i| order.contains(i))
            .collect();
        // Before itself is where it is already.
        if moving.is_empty() || before.is_some_and(|b| moving.contains(&b)) {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        // Keys first, for a level that still has unkeyed items.
        if order.iter().any(|i| self.key_of(level, *i).is_empty()) {
            let keys = key::spread(None, None, order.len());
            for (item, key) in order.iter().zip(keys) {
                self.set_key(level, *item, key, at);
            }
        }
        order.retain(|i| !moving.contains(i));
        let gap = before
            .and_then(|b| order.iter().position(|i| *i == b))
            .unwrap_or(order.len());
        let prev = gap.checked_sub(1).and_then(|p| order.get(p)).copied();
        let next = order.get(gap).copied();
        let prev = prev.map(|i| self.key_of(level, i).to_owned());
        let next = next.map(|i| self.key_of(level, i).to_owned());
        let keys = key::spread(prev.as_deref(), next.as_deref(), moving.len());
        for (item, key) in moving.into_iter().zip(keys) {
            self.set_key(level, item, key, at);
        }
        if level.is_none() {
            self.home_end = None;
        }
        self.bump();
        true
    }

    /// A key after everything on `level`, for something joining it.
    pub(crate) fn next_key(&mut self, level: Option<ShelfId>) -> String {
        let end = match level {
            // Kept, as the top may hold every book: no scan per import.
            None => match &self.home_end {
                Some(end) => end.clone(),
                None => self.last_key(None),
            },
            Some(_) => self.last_key(level),
        };
        let key = key::after(end.as_deref());
        if level.is_none() {
            self.home_end = Some(Some(key.clone()));
        }
        key
    }

    /// The largest key on a level, removed items' included.
    fn last_key(&self, level: Option<ShelfId>) -> Option<String> {
        let shelves = self.shelves.values().filter(|s| self.parent_of(s) == level);
        let shelves = shelves.map(|s| s.key.value.as_str());
        let links = self.links.values().filter(|l| l.level.value == level);
        let shelves = shelves.chain(links.map(|l| l.key.value.as_str()));
        let last = match level {
            None => shelves
                .chain(self.books.values().map(|b| b.key.value.as_str()))
                .max(),
            Some(shelf) => {
                let range = (shelf, BookId(0))..=(shelf, BookId(u128::MAX));
                let members = self.members.range(range).map(|(_, m)| m.key.value.as_str());
                shelves.chain(members).max()
            }
        };
        last.filter(|k| !k.is_empty()).map(str::to_owned)
    }
}
