//! Shelves: nested collections a reader makes, holding books by id.

use std::collections::BTreeSet;
use std::hash::{BuildHasher, RandomState};

use serde::{Deserialize, Serialize};

use crate::book::Book;
use crate::clock::{Field, Stamp};
use crate::extra::Extra;
use crate::id::{BookId, ShelfId, SourceId};
use crate::library::Library;
use crate::view::natural;

/// The longest shelf name kept, in characters.
pub const NAME_MAX: usize = 120;
/// A chain of parents deeper than this is a loop a merge left.
const DEPTH_MAX: usize = 64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shelf {
    pub id: ShelfId,
    pub name: Field<String>,
    /// None: on the top level.
    pub parent: Field<Option<ShelfId>>,
    /// Where it was before its last move; a move a merge loops goes back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<ShelfId>,
    /// When it was made, in wall milliseconds.
    pub added: u64,
    /// Its place among the shelves and books beside it.
    #[serde(default)]
    pub key: Field<String>,
    /// What made it: the reader, a folder's rung, or a rung that left.
    #[serde(default)]
    pub kind: Field<ShelfKind>,
    /// A rung moved off its seat by hand; walks leave it where it is.
    #[serde(default)]
    pub moved: Field<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<Stamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored: Option<Stamp>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// What made a shelf, which decides the questions it asks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "is")]
pub enum ShelfKind {
    /// Made by the reader; no folder answers for it.
    #[default]
    Own,
    /// Cut from a folder's tree; `key` is the subfolder, `""` the root.
    Rung { folder: SourceId, key: String },
    /// A rung that left its tree: its books are the library's own.
    Departed,
}

impl ShelfKind {
    /// The folder and rung key this shelf stands for, if a rung.
    pub fn rung(&self) -> Option<(SourceId, &str)> {
        match self {
            Self::Rung { folder, key } => Some((*folder, key)),
            Self::Own | Self::Departed => None,
        }
    }
}

impl Shelf {
    /// Not removed, or restored since; removal is a tombstone.
    pub fn alive(&self) -> bool {
        match (self.removed, self.restored) {
            (None, _) => true,
            (Some(removed), Some(restored)) => restored > removed,
            (Some(_), None) => false,
        }
    }
}

/// A book on a shelf: its place in the manual order, and whether it's on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Membership {
    pub shelf: ShelfId,
    pub book: BookId,
    pub key: Field<String>,
    pub alive: Field<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Why a shelf can't take a name or a place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShelfError {
    Blank,
    /// A shelf beside it has the name already.
    Taken(String),
    /// A shelf can't go inside itself or its own shelves.
    Inside,
    Gone,
}

/// What removing a shelf did to what it held.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShelfRemoved {
    /// Books it held; they stay in the library.
    pub books: usize,
    /// Shelves moved up a level.
    pub shelves: usize,
    /// Moved-up shelves renamed to fit beside their new neighbours.
    pub renamed: Vec<String>,
}

impl Library {
    pub fn shelf(&self, id: ShelfId) -> Option<&Shelf> {
        self.shelves.get(&id).filter(|s| s.alive())
    }

    /// Every shelf not removed, in no particular order.
    pub fn shelves(&self) -> impl Iterator<Item = &Shelf> {
        self.shelves.values().filter(|s| s.alive())
    }

    /// Where a shelf sits; a removed or unknown parent is the top.
    pub fn parent_of(&self, shelf: &Shelf) -> Option<ShelfId> {
        shelf.parent.value.filter(|p| self.shelf(*p).is_some())
    }

    /// Where a shelf was last put, even if it or its parent is removed.
    pub fn recorded_parent(&self, id: ShelfId) -> Option<ShelfId> {
        self.shelves.get(&id).and_then(|s| s.parent.value)
    }

    /// The shelves on `parent`'s level, by name.
    pub fn children(&self, parent: Option<ShelfId>) -> Vec<&Shelf> {
        let mut out: Vec<&Shelf> = (self.shelves())
            .filter(|s| self.parent_of(s) == parent && self.rooted(s.id))
            .collect();
        out.sort_by(|a, b| natural(&a.name.value, &b.name.value).then(a.id.cmp(&b.id)));
        out
    }

    /// Every shelf reachable from the top, by name.
    pub fn every_shelf(&self) -> Vec<&Shelf> {
        let mut out: Vec<&Shelf> = self.shelves().filter(|s| self.rooted(s.id)).collect();
        out.sort_by(|a, b| natural(&a.name.value, &b.name.value).then(a.id.cmp(&b.id)));
        out
    }

    /// From the top level down to `id`; empty if it's gone.
    pub fn path_to(&self, id: ShelfId) -> Vec<&Shelf> {
        let mut path = Vec::new();
        let mut at = self.shelf(id);
        while let Some(shelf) = at {
            if path.len() == DEPTH_MAX {
                return Vec::new();
            }
            path.push(shelf);
            at = self.parent_of(shelf).and_then(|p| self.shelf(p));
        }
        path.reverse();
        path
    }

    /// Reaches the top: not caught in a loop of parents.
    fn rooted(&self, id: ShelfId) -> bool {
        !self.path_to(id).is_empty()
    }

    /// The books on a shelf, in its manual order.
    pub fn shelf_books(&self, id: ShelfId) -> Vec<&Book> {
        let mut on: Vec<(&str, &Book)> = self.on(id).collect();
        on.sort_by(|a, b| a.0.cmp(b.0).then(a.1.id.cmp(&b.1.id)));
        on.into_iter().map(|(_, book)| book).collect()
    }

    /// Each living book on `id` with its key, unordered.
    fn on(&self, id: ShelfId) -> impl Iterator<Item = (&str, &Book)> {
        let range = (id, BookId(0))..=(id, BookId(u128::MAX));
        let members = self.members.range(range).filter(|(_, m)| m.alive.value);
        members.filter_map(|(_, m)| Some((m.key.value.as_str(), self.book(m.book)?)))
    }

    /// The shelves a book is on.
    pub fn shelves_of(&self, book: BookId) -> BTreeSet<ShelfId> {
        let on = self
            .members
            .values()
            .filter(|m| m.book == book && m.alive.value);
        on.map(|m| m.shelf)
            .filter(|&s| self.shelf(s).is_some())
            .collect()
    }

    /// Books on some shelf; one test per book, cheaper than `shelves_of`.
    pub fn shelved(&self) -> BTreeSet<BookId> {
        let on = self.members.values().filter(|m| m.alive.value);
        on.filter(|m| self.shelf(m.shelf).is_some())
            .map(|m| m.book)
            .collect()
    }

    /// Books and shelves directly on `id`.
    pub fn counts(&self, id: ShelfId) -> (usize, usize) {
        (self.on(id).count(), self.children(Some(id)).len())
    }

    /// "New shelf", else the first free "New shelf 2", "New shelf 3"…
    pub fn fresh_shelf_name(&self, parent: Option<ShelfId>) -> String {
        let free = |name: &str| self.free(name, parent, None);
        let numbered = (2..).map(|n| format!("New shelf {n}"));
        let mut names = std::iter::once("New shelf".to_owned()).chain(numbered);
        names.find(|n| free(n)).unwrap_or_default()
    }

    /// No shelf beside `parent`'s others but `own` has `name`.
    pub(crate) fn free(&self, name: &str, parent: Option<ShelfId>, own: Option<ShelfId>) -> bool {
        let lower = name.to_lowercase();
        let others = self.children(parent).into_iter();
        !others
            .filter(|s| Some(s.id) != own)
            .any(|s| s.name.value.to_lowercase() == lower)
    }

    pub fn create_shelf(
        &mut self,
        name: &str,
        parent: Option<ShelfId>,
        wall_ms: u64,
    ) -> Result<ShelfId, ShelfError> {
        let name = clean(name).ok_or(ShelfError::Blank)?;
        if parent.is_some_and(|p| self.shelf(p).is_none()) {
            return Err(ShelfError::Gone);
        }
        if !self.free(&name, parent, None) {
            return Err(ShelfError::Taken(name));
        }
        let at = self.clock.tick(wall_ms);
        let id = self.new_shelf_id(at);
        let key = self.next_key(parent);
        let shelf = Shelf {
            id,
            name: Field::new(name, at),
            parent: Field::new(parent, at),
            from: None,
            added: wall_ms,
            key: Field::new(key, at),
            kind: Field::default(),
            moved: Field::default(),
            removed: None,
            restored: None,
            extra: Extra::default(),
        };
        self.shelves.insert(id, shelf);
        self.bump();
        Ok(id)
    }

    fn new_shelf_id(&self, at: Stamp) -> ShelfId {
        let state = RandomState::new();
        let seed = (at.ms, at.counter, self.device().0);
        let half = |salt: u8| u128::from(state.hash_one((seed, salt)));
        let mut id = ShelfId(half(1) << 64 | half(2));
        while self.shelves.contains_key(&id) {
            id = ShelfId(id.0.wrapping_add(1));
        }
        id
    }

    pub fn rename_shelf(
        &mut self,
        id: ShelfId,
        name: &str,
        wall_ms: u64,
    ) -> Result<(), ShelfError> {
        let name = clean(name).ok_or(ShelfError::Blank)?;
        let shelf = self.shelf(id).ok_or(ShelfError::Gone)?;
        if shelf.name.value == name {
            return Ok(());
        }
        let parent = self.parent_of(shelf);
        if !self.free(&name, parent, Some(id)) {
            return Err(ShelfError::Taken(name));
        }
        let at = self.clock.tick(wall_ms);
        if let Some(shelf) = self.shelves.get_mut(&id) {
            shelf.name = Field::new(name, at);
        }
        self.bump();
        Ok(())
    }

    /// Files a shelf inside `parent`, or on the top level.
    pub fn move_shelf(
        &mut self,
        id: ShelfId,
        parent: Option<ShelfId>,
        wall_ms: u64,
    ) -> Result<(), ShelfError> {
        let shelf = self.shelf(id).ok_or(ShelfError::Gone)?;
        if self.parent_of(shelf) == parent {
            return Ok(());
        }
        if let Some(p) = parent {
            let path = self.path_to(p);
            if path.is_empty() {
                return Err(ShelfError::Gone);
            }
            if path.iter().any(|s| s.id == id) {
                return Err(ShelfError::Inside);
            }
        }
        let name = shelf.name.value.clone();
        if !self.free(&name, parent, Some(id)) {
            return Err(ShelfError::Taken(name));
        }
        let at = self.clock.tick(wall_ms);
        // It joins its new level at the end.
        let key = self.next_key(parent);
        if let Some(shelf) = self.shelves.get_mut(&id) {
            shelf.from = shelf.parent.value;
            shelf.parent = Field::new(parent, at);
            shelf.key = Field::new(key, at);
        }
        self.bump();
        Ok(())
    }

    /// Takes a shelf apart: its books stay, its shelves move up a level.
    pub fn remove_shelf(&mut self, id: ShelfId, wall_ms: u64) -> Option<ShelfRemoved> {
        let shelf = self.shelf(id)?;
        let parent = self.parent_of(shelf);
        let inner: Vec<(ShelfId, String)> = (self.children(Some(id)).into_iter())
            .map(|s| (s.id, s.name.value.clone()))
            .collect();
        let at = self.clock.tick(wall_ms);
        let mut done = ShelfRemoved {
            books: self.on(id).count(),
            shelves: inner.len(),
            renamed: Vec::new(),
        };
        if let Some(shelf) = self.shelves.get_mut(&id) {
            shelf.removed = Some(at);
        }
        for (child, name) in inner {
            // Beside the parent's own shelves, a clash gets a number.
            let mut fit = name.clone();
            let mut n = 2;
            while !self.free(&fit, parent, Some(child)) {
                fit = format!("{name} {n}");
                n += 1;
            }
            if let Some(shelf) = self.shelves.get_mut(&child) {
                shelf.from = shelf.parent.value;
                shelf.parent = Field::new(parent, at);
                if fit != name {
                    shelf.name = Field::new(fit, at);
                    done.renamed.push(name);
                }
            }
        }
        let range = (id, BookId(0))..=(id, BookId(u128::MAX));
        for (_, m) in self.members.range_mut(range) {
            m.alive = Field::new(false, at);
        }
        self.bump();
        Some(done)
    }

    /// Puts a book at the end of a shelf; false if it's already there.
    pub fn file(&mut self, book: BookId, shelf: ShelfId, wall_ms: u64) -> bool {
        let on = self
            .members
            .get(&(shelf, book))
            .is_some_and(|m| m.alive.value);
        if on || self.book(book).is_none() || self.shelf(shelf).is_none() {
            return false;
        }
        let key = self.next_key(Some(shelf));
        self.set_member(book, shelf, key, wall_ms);
        true
    }

    /// Takes a book off a shelf; it stays in the library.
    pub fn unfile(&mut self, book: BookId, shelf: ShelfId, wall_ms: u64) -> bool {
        let on = self
            .members
            .get(&(shelf, book))
            .is_some_and(|m| m.alive.value);
        if !on {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        if let Some(m) = self.members.get_mut(&(shelf, book)) {
            m.alive = Field::new(false, at);
        }
        self.bump();
        true
    }

    fn set_member(&mut self, book: BookId, shelf: ShelfId, key: String, wall_ms: u64) {
        let at = self.clock.tick(wall_ms);
        // Back on a shelf: what a newer build wrote on it stays.
        let old = self.members.get(&(shelf, book));
        let extra = old.map(|m| m.extra.clone()).unwrap_or_default();
        let member = Membership {
            shelf,
            book,
            key: Field::new(key, at),
            alive: Field::new(true, at),
            extra,
        };
        self.members.insert((shelf, book), member);
        self.bump();
    }
}

/// One line, trimmed and capped; none if nothing is left.
pub(crate) fn clean(name: &str) -> Option<String> {
    let spaced = name.replace(char::is_control, " ");
    let words: Vec<&str> = spaced.split_whitespace().collect();
    let joined: String = words.join(" ").chars().take(NAME_MAX).collect();
    let joined = joined.trim_end().to_owned();
    (!joined.is_empty()).then_some(joined)
}
