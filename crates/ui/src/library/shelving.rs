//! Shelves on the page: where the reader is, and what drags and drops do.

use mareader_library::{BookId, Item, Library, Move, ShelfId};

use super::books::counted;
use super::moving::CopyAnswer;
use super::{Action, LibraryState, Scope, wall_ms};
use crate::command::Commands;

/// Shelf work asked for by the page, its crumbs and its sheets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShelfAction {
    /// Opens a shelf, or the library's top level.
    GoTo(Option<ShelfId>),
    /// Makes "New shelf" where the reader is and opens it.
    New,
    /// Names the open shelf in its crumb; Escape cancels.
    StartRename(ShelfId),
    CancelRename,
    /// Takes a shelf apart, asking first if its books leave a folder.
    Apart(ShelfId),
    /// Books and shelves carried to a level, by a drop or a menu.
    Carry(Move),
    /// The copy ask's answer.
    Copy(CopyAnswer),
    /// What's carried rested on a book: both go on a new shelf there.
    Stack {
        onto: BookId,
        carry: Carry,
        level: Option<ShelfId>,
        keep: bool,
    },
    /// Cards moved in a level's own order, before another or to the end.
    Arrange {
        items: Vec<Item>,
        level: Option<ShelfId>,
        before: Option<Item>,
    },
}

/// What every shelf made by hand is called until renamed.
pub const NEW_SHELF: &str = "New shelf";

/// What a drag carries; egui keeps it until the drop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Carry {
    /// A book, from the shelf it was shown on.
    Book {
        id: BookId,
        from: Option<ShelfId>,
    },
    Shelf(ShelfId),
    /// A selection, from the shelf it was shown on.
    Many {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
        from: Option<ShelfId>,
    },
}

impl Carry {
    /// What dropping it on `target` (none: the top level) does.
    pub fn onto(&self, target: Option<ShelfId>, keep: bool) -> Option<Action> {
        let (books, shelves, from) = self.parts();
        // Onto where it came from, or onto one of its own shelves: nothing.
        let home = !matches!(self, Self::Shelf(_)) && from == target;
        if home || target.is_some_and(|t| shelves.contains(&t)) {
            return None;
        }
        let mv = Move {
            books,
            shelves,
            from,
            to: target,
            keep,
            before: None,
        };
        Some(Action::Shelf(ShelfAction::Carry(mv)))
    }

    /// The cards it moves within `level`, if it came from there.
    pub fn items_at(&self, level: Option<ShelfId>, model: &Library) -> Option<Vec<Item>> {
        let here = |s: &ShelfId| model.shelf(*s).is_some_and(|s| model.parent_of(s) == level);
        let items = match self {
            Self::Book { id, from } if *from == level => vec![Item::Book(*id)],
            Self::Shelf(id) if here(id) => vec![Item::Shelf(*id)],
            Self::Many {
                books,
                shelves,
                from,
            } if *from == level && shelves.iter().all(here) => {
                let shelves = shelves.iter().map(|s| Item::Shelf(*s));
                shelves
                    .chain(books.iter().map(|b| Item::Book(*b)))
                    .collect()
            }
            _ => return None,
        };
        Some(items)
    }

    /// Every card it carries, shelves first.
    pub fn picks(&self) -> Vec<Item> {
        let (books, shelves, _) = self.parts();
        let shelves = shelves.into_iter().map(Item::Shelf);
        shelves.chain(books.into_iter().map(Item::Book)).collect()
    }

    /// Whether a book is among what it carries, so it can stack.
    pub fn has_books(&self) -> bool {
        match self {
            Self::Book { .. } => true,
            Self::Shelf(_) => false,
            Self::Many { books, .. } => !books.is_empty(),
        }
    }

    /// Its books, its shelves, and the shelf it came off.
    fn parts(&self) -> (Vec<BookId>, Vec<ShelfId>, Option<ShelfId>) {
        match self {
            &Self::Book { id, from } => (vec![id], Vec::new(), from),
            &Self::Shelf(id) => (Vec::new(), vec![id], None),
            Self::Many {
                books,
                shelves,
                from,
            } => (books.clone(), shelves.clone(), *from),
        }
    }

    /// Whether it was carried off a shelf, so Alt can add instead.
    pub fn off_shelf(&self) -> bool {
        match self {
            Self::Book { from, .. } | Self::Many { from, .. } => from.is_some(),
            Self::Shelf(_) => false,
        }
    }

    /// The words under the pointer while it's carried.
    pub fn label(&self, model: &Library) -> String {
        match self {
            Self::Book { id, .. } => model.book(*id).map(|b| b.title.value.clone()),
            Self::Shelf(id) => model.shelf(*id).map(|s| s.name.value.clone()),
            Self::Many { books, shelves, .. } => Some(match (books.len(), shelves.len()) {
                (b, 0) => counted(b, "book", "books"),
                (0, s) => counted(s, "shelf", "shelves"),
                (b, s) => format!(
                    "{}, {}",
                    counted(b, "book", "books"),
                    counted(s, "shelf", "shelves")
                ),
            }),
        }
        .unwrap_or_default()
    }
}

/// One step of the breadcrumb; none is the library's top level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crumb {
    pub id: Option<ShelfId>,
    pub name: String,
}

impl LibraryState {
    /// The shelf open on the page; none is the top level.
    pub fn at(&self) -> Option<ShelfId> {
        self.at
    }

    /// The shelf whose name is being edited in the breadcrumb.
    pub fn renaming(&self) -> Option<ShelfId> {
        self.renaming
    }

    /// From the top level to the open shelf; empty at the top.
    pub fn trail(&self) -> Vec<Crumb> {
        let Some(at) = self.at else {
            return Vec::new();
        };
        let path = self.model.path_to(at).into_iter().map(|s| Crumb {
            id: Some(s.id),
            name: s.name.value.clone(),
        });
        let top = Crumb {
            id: None,
            name: "Library".into(),
        };
        std::iter::once(top).chain(path).collect()
    }

    /// A shelf removed, here or by a sync, gives way to its nearest parent.
    pub fn settle(&mut self) {
        self.settle_picks();
        let Some(at) = self.at else {
            return;
        };
        let open = |id: ShelfId| !self.model.path_to(id).is_empty();
        let mut up = Some(at);
        // Bounded: removed shelves can still loop through their parents.
        for _ in 0..64 {
            match up {
                Some(id) if !open(id) => up = self.model.recorded_parent(id),
                _ => break,
            }
        }
        self.at = up.filter(|&id| open(id));
        self.renaming = self.renaming.filter(|&id| self.model.shelf(id).is_some());
    }

    /// Whether `to` is inside the shelf open now.
    fn goes_down(&self, to: Option<ShelfId>) -> bool {
        let Some(to) = to.filter(|&to| Some(to) != self.at) else {
            return false;
        };
        let path = self.model.path_to(to);
        self.at.is_none_or(|at| path.iter().any(|s| s.id == at))
    }

    /// Picks of removed books and shelves fall away.
    fn settle_picks(&mut self) {
        if self.picks.is_empty() {
            return;
        }
        let model = &self.model;
        self.picks.retain(|p| match p {
            super::Pick::Book(id) => model.book(id).is_some(),
            super::Pick::Shelf(id) => model.shelf(id).is_some(),
        });
    }

    /// Opens the shelf at a path of names from the top; false if none.
    pub fn open_shelf(&mut self, path: &str) -> bool {
        let mut at = None;
        for name in path.split('/').filter(|n| !n.is_empty()) {
            let shelves = self.model.children(at).into_iter();
            let mut named = shelves.filter(|s| s.name.value == name);
            match named.next() {
                Some(shelf) => at = Some(shelf.id),
                None => return false,
            }
        }
        self.at = at;
        true
    }

    /// Made-up shelves for shots, over the seeded books.
    pub fn seed_shelves(&mut self) {
        let now = wall_ms();
        let books: Vec<BookId> = self.model.books().map(|b| b.id).collect();
        let mut make = |name: &str, parent| self.model.create_shelf(name, parent, now).ok();
        let fiction = make("Fiction", None);
        let essays = make("Essays", None);
        let classics = make("Classics", fiction);
        make("To read", None);
        let plan = [
            (fiction, [0, 1, 4, 6].as_slice()),
            (classics, &[0, 1]),
            (essays, &[5, 8]),
        ];
        for (shelf, picks) in plan {
            let picked = picks.iter().filter_map(|&n| books.get(n));
            for (shelf, &book) in picked.filter_map(|b| Some((shelf?, b))) {
                self.model.file(book, shelf, now);
            }
        }
    }

    pub(super) fn shelf_act(&mut self, action: ShelfAction, out: &mut Commands) {
        let now = wall_ms();
        match action {
            ShelfAction::GoTo(at) => {
                let at = at.filter(|&id| self.model.shelf(id).is_some());
                // Into a shelf the search found: that search is done.
                if self.goes_down(at) {
                    self.search.clear();
                }
                self.at = at;
                self.scope = Scope::All;
                self.renaming = None;
                self.picks.apply(super::PickAction::Clear);
            }
            ShelfAction::New => {
                if let Ok(id) = self.model.create_shelf(NEW_SHELF, self.at, now) {
                    self.search.clear();
                    self.at = Some(id);
                    self.scope = Scope::All;
                }
            }
            ShelfAction::StartRename(id) => {
                self.renaming = self.model.shelf(id).map(|s| s.id);
            }
            ShelfAction::CancelRename => self.renaming = None,
            ShelfAction::Apart(id) => self.apart(id, out),
            ShelfAction::Carry(mv) => self.carry(mv, out),
            ShelfAction::Copy(answer) => self.answer_copy(answer, out),
            ShelfAction::Stack {
                onto,
                carry,
                level,
                keep,
            } => self.stack(onto, &carry, level, keep, out),
            ShelfAction::Arrange {
                items,
                level,
                before,
            } => {
                self.model.arrange(level, &items, before, now);
                if items.len() > 1 {
                    self.picks.apply(super::PickAction::Clear);
                }
            }
        }
    }

    /// Makes a shelf where `onto` sits on `level`, and carries both onto it.
    fn stack(
        &mut self,
        onto: BookId,
        carry: &Carry,
        level: Option<ShelfId>,
        keep: bool,
        out: &mut Commands,
    ) {
        let now = wall_ms();
        let (books, shelves, _) = carry.parts();
        let books: Vec<BookId> = (books.into_iter()).filter(|&b| b != onto).collect();
        if books.is_empty() || self.model.book(onto).is_none() {
            return;
        }
        let name = self.stack_name(onto, &books);
        let Ok(id) = self.model.create_shelf(&name, level, now) else {
            return;
        };
        self.picks.apply(super::PickAction::Clear);
        let (shelf, book) = (Item::Shelf(id), Item::Book(onto));
        self.model.arrange(level, &[shelf], Some(book), now);
        let mv = Move {
            books: std::iter::once(onto).chain(books).collect(),
            shelves,
            from: level,
            to: Some(id),
            keep,
            before: None,
        };
        self.carry(mv, out);
    }

    /// The books' one shared author, else "New shelf".
    fn stack_name(&self, onto: BookId, books: &[BookId]) -> String {
        let author = |b: &BookId| self.model.book(*b).and_then(|b| b.author.value.clone());
        let first = author(&onto).filter(|a| !a.trim().is_empty());
        let shared = first.filter(|a| books.iter().all(|b| author(b).as_ref() == Some(a)));
        shared.unwrap_or_else(|| NEW_SHELF.to_owned())
    }

    pub(super) fn shelf_name(&self, id: ShelfId) -> String {
        let shelf = self.model.shelf(id).map(|s| s.name.value.clone());
        shelf.unwrap_or_default()
    }
}
