//! Shelves on the page: where the reader is, and what drags and drops do.

use mareader_library::{BookId, Item, Library, ShelfError, ShelfId};

use super::books::counted;
use super::{Action, LibraryState, Scope, Sheet, wall_ms};
use crate::command::{Command, Commands};
use crate::toast::Toast;

/// Shelf work asked for by the page, its crumbs and its sheets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShelfAction {
    /// Opens a shelf, or the library's top level.
    GoTo(Option<ShelfId>),
    /// Makes one where the reader is, opens it and starts naming it.
    New,
    StartRename(ShelfId),
    Rename(ShelfId, String),
    CancelRename,
    /// Asks before `Remove`.
    AskRemove(ShelfId),
    Remove(ShelfId),
    /// A book dropped on a shelf, or off `from` when `to` is none.
    File {
        book: BookId,
        to: Option<ShelfId>,
        from: Option<ShelfId>,
        /// Added beside `from` rather than moved from it.
        keep: bool,
    },
    Move {
        shelf: ShelfId,
        to: Option<ShelfId>,
    },
    /// A selection dropped: its books filed as `File`, shelves moved.
    FileMany {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
        to: Option<ShelfId>,
        from: Option<ShelfId>,
        keep: bool,
    },
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
        let action = match self {
            Self::Book { from, .. } if *from == target => return None,
            &Self::Book { id, from } => ShelfAction::File {
                book: id,
                to: target,
                from,
                keep,
            },
            Self::Shelf(shelf) if Some(*shelf) == target => return None,
            &Self::Shelf(shelf) => ShelfAction::Move { shelf, to: target },
            // Onto where it came from, or one of its own shelves: nothing.
            Self::Many { from, .. } if *from == target => return None,
            Self::Many { shelves, .. } if target.is_some_and(|t| shelves.contains(&t)) => {
                return None;
            }
            Self::Many {
                books,
                shelves,
                from,
            } => ShelfAction::FileMany {
                books: books.clone(),
                shelves: shelves.clone(),
                to: target,
                from: *from,
                keep,
            },
        };
        Some(Action::Shelf(action))
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
        let notify = |out: &mut Commands, toast| out.push(Command::Notify(toast));
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
                let name = self.model.fresh_shelf_name(self.at);
                if let Ok(id) = self.model.create_shelf(&name, self.at, now) {
                    self.search.clear();
                    self.at = Some(id);
                    self.scope = Scope::All;
                    self.renaming = Some(id);
                }
            }
            ShelfAction::StartRename(id) => {
                self.renaming = self.model.shelf(id).map(|s| s.id);
            }
            ShelfAction::CancelRename => self.renaming = None,
            ShelfAction::Rename(id, name) => {
                self.renaming = None;
                // An empty name keeps the old one.
                match self.model.rename_shelf(id, &name, now) {
                    Ok(()) | Err(ShelfError::Blank | ShelfError::Gone) => {}
                    Err(e) => notify(out, Toast::error(why(&e))),
                }
            }
            ShelfAction::AskRemove(id) => {
                self.renaming = None;
                self.folders.sheet = Some(Sheet::RemoveShelf(id));
            }
            ShelfAction::Remove(id) => {
                self.folders.sheet = None;
                let name = self.shelf_name(id);
                let parent = self.model.shelf(id).and_then(|s| self.model.parent_of(s));
                let Some(done) = self.model.remove_shelf(id, now) else {
                    return;
                };
                if self.at == Some(id) {
                    self.at = parent;
                }
                let mut note = format!("Removed “{name}”.");
                if done.books > 0 {
                    note.push_str(" Its books stay in the library.");
                }
                if let Some(first) = done.renamed.first() {
                    let more = done.renamed.len().saturating_sub(1);
                    note.push_str(&match more {
                        0 => format!(" “{first}” was renamed to fit beside its neighbours."),
                        n => format!(" “{first}” and {n} more were renamed to fit."),
                    });
                }
                notify(out, Toast::info(note));
            }
            ShelfAction::File {
                book,
                to,
                from,
                keep,
            } => self.file(book, to, from, keep, out),
            ShelfAction::Move { shelf, to } => {
                if let Err(e) = self.model.move_shelf(shelf, to, now) {
                    notify(out, Toast::error(why(&e)));
                }
            }
            ShelfAction::FileMany {
                books,
                shelves,
                to,
                from,
                keep,
            } => self.file_many(&books, &shelves, (to, from), keep, out),
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

    /// Makes a shelf where `onto` sits on `level`, holding it and `carry`.
    fn stack(
        &mut self,
        onto: BookId,
        carry: &Carry,
        level: Option<ShelfId>,
        keep: bool,
        out: &mut Commands,
    ) {
        let now = wall_ms();
        let (books, shelves, from) = carry.parts();
        let books: Vec<BookId> = (books.into_iter()).filter(|&b| b != onto).collect();
        if books.is_empty() || self.model.book(onto).is_none() {
            return;
        }
        let name = self.stack_name(onto, &books, level);
        let Ok(id) = self.model.create_shelf(&name, level, now) else {
            return;
        };
        self.picks.apply(super::PickAction::Clear);
        let (shelf, book) = (Item::Shelf(id), Item::Book(onto));
        self.model.arrange(level, &[shelf], Some(book), now);
        self.model.file(onto, id, now);
        if let Some(level) = level {
            self.model.unfile(onto, level, now);
        }
        let leave = from.filter(|_| !keep);
        for &b in &books {
            self.model.file(b, id, now);
            if let Some(from) = leave {
                self.model.unfile(b, from, now);
            }
        }
        let moved = shelves
            .iter()
            .filter(|&&s| self.model.move_shelf(s, Some(id), now).is_ok());
        let shelved = moved.count();
        let mut what = counted(books.len() + 1, "book", "books");
        if shelved > 0 {
            what = format!("{what} and {}", counted(shelved, "shelf", "shelves"));
        }
        let note = format!("Made “{name}” of {what}.");
        out.push(Command::Notify(Toast::info(note)));
    }

    /// The books' one shared author, if free there; else "New shelf".
    fn stack_name(&self, onto: BookId, books: &[BookId], level: Option<ShelfId>) -> String {
        let author = |b: &BookId| self.model.book(*b).and_then(|b| b.author.value.clone());
        let first = author(&onto).filter(|a| !a.trim().is_empty());
        let shared = first.filter(|a| books.iter().all(|b| author(b).as_ref() == Some(a)));
        let taken = |name: &str| {
            let here = self.model.children(level).into_iter();
            here.into_iter()
                .any(|s| s.name.value.eq_ignore_ascii_case(name))
        };
        match shared {
            Some(name) if !taken(&name) => name,
            _ => self.model.fresh_shelf_name(level),
        }
    }

    /// Files a dropped selection: books as `file` does, shelves moved.
    fn file_many(
        &mut self,
        books: &[BookId],
        shelves: &[ShelfId],
        (to, from): (Option<ShelfId>, Option<ShelfId>),
        keep: bool,
        out: &mut Commands,
    ) {
        let now = wall_ms();
        self.picks.apply(super::PickAction::Clear);
        let (mut filed, mut moved, mut off) = (0, 0, 0);
        for &book in books {
            let added = to.is_some_and(|t| self.model.file(book, t, now));
            let leave = from
                .filter(|_| !keep || to.is_none())
                .filter(|&f| Some(f) != to);
            let left = leave.is_some_and(|f| self.model.unfile(book, f, now));
            match (added, left, to) {
                (_, true, None) => off += 1,
                (_, true, Some(_)) => moved += 1,
                (true, false, _) => filed += 1,
                (false, false, _) => {}
            }
        }
        let mut refused = None;
        let mut shelved = 0;
        for &shelf in shelves {
            match self.model.move_shelf(shelf, to, now) {
                Ok(()) => shelved += 1,
                Err(e) => refused = Some(why(&e)),
            }
        }
        let name = |s: Option<ShelfId>| s.map_or("the library".into(), |s| self.shelf_name(s));
        let books_done = counted(filed + moved + off, "book", "books");
        let shelves_done = counted(shelved, "shelf", "shelves");
        let what = match (filed + moved + off, shelved) {
            (0, 0) => None,
            (_, 0) => Some(books_done),
            (0, _) => Some(shelves_done),
            _ => Some(format!("{books_done} and {shelves_done}")),
        };
        let note = match (what, to) {
            (None, _) => refused.unwrap_or_else(|| format!("They're already on “{}”.", name(to))),
            (Some(what), None) if off > 0 => format!("Took {what} off “{}”.", name(from)),
            (Some(what), _) if moved > 0 || shelved > 0 => {
                format!("Moved {what} to “{}”.", name(to))
            }
            (Some(what), _) => format!("Added {what} to “{}”.", name(to)),
        };
        out.push(Command::Notify(Toast::info(note)));
    }

    /// Files, moves or unfiles a dropped book, and says what happened.
    fn file(
        &mut self,
        book: BookId,
        to: Option<ShelfId>,
        from: Option<ShelfId>,
        keep: bool,
        out: &mut Commands,
    ) {
        let now = wall_ms();
        let title = self.model.book(book).map(|b| b.title.value.clone());
        let title = title.unwrap_or_default();
        let note = match (to, from) {
            (None, Some(from)) => {
                self.model.unfile(book, from, now);
                format!("Took “{title}” off “{}”.", self.shelf_name(from))
            }
            (None, None) => return,
            (Some(to), from) => {
                let filed = self.model.file(book, to, now);
                let off = from.filter(|_| !keep).filter(|&f| f != to);
                let moved = off.is_some_and(|f| self.model.unfile(book, f, now));
                let shelf = self.shelf_name(to);
                match (filed, moved) {
                    (_, true) => format!("Moved “{title}” to “{shelf}”."),
                    (true, false) => format!("Added “{title}” to “{shelf}”."),
                    (false, false) => format!("“{title}” is already on “{shelf}”."),
                }
            }
        };
        out.push(Command::Notify(Toast::info(note)));
    }

    fn shelf_name(&self, id: ShelfId) -> String {
        let shelf = self.model.shelf(id).map(|s| s.name.value.clone());
        shelf.unwrap_or_default()
    }
}

fn why(error: &ShelfError) -> String {
    match error {
        ShelfError::Taken(name) => format!("There's already a shelf called “{name}” there."),
        ShelfError::Inside => "A shelf can't go inside itself.".into(),
        ShelfError::Blank => "A shelf needs a name.".into(),
        ShelfError::Gone => "That shelf was removed.".into(),
    }
}
