//! Shelves on the page: where the reader is, and what drags and drops do.

use mareader_library::{Book, BookId, Home, Library, ShelfError, ShelfId, SortKey, View};

use super::folders::in_scope;
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
    /// A book moved in a shelf's own order, before another or to the end.
    Reorder {
        book: BookId,
        shelf: ShelfId,
        before: Option<BookId>,
    },
}

/// What a drag carries; egui keeps it until the drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carry {
    /// A book, from the shelf it was shown on.
    Book {
        id: BookId,
        from: Option<ShelfId>,
    },
    Shelf(ShelfId),
}

impl Carry {
    /// What dropping it on `target` (none: the top level) does.
    pub fn onto(self, target: Option<ShelfId>, keep: bool) -> Option<Action> {
        let action = match self {
            Self::Book { from, .. } if from == target => return None,
            Self::Book { id, from } => ShelfAction::File {
                book: id,
                to: target,
                from,
                keep,
            },
            Self::Shelf(shelf) if Some(shelf) == target => return None,
            Self::Shelf(shelf) => ShelfAction::Move { shelf, to: target },
        };
        Some(Action::Shelf(action))
    }

    /// The words under the pointer while it's carried.
    pub fn label(self, model: &Library) -> String {
        match self {
            Self::Book { id, .. } => model.book(id).map(|b| b.title.value.clone()),
            Self::Shelf(id) => model.shelf(id).map(|s| s.name.value.clone()),
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

/// The page's books in the order shown, for where the reader is.
pub fn books_shown<'a>(
    model: &'a Library,
    at: Option<ShelfId>,
    scope: Scope,
    view: &View,
) -> Vec<&'a Book> {
    if let Some(shelf) = at {
        let own = model.shelf_books(shelf);
        return match (view.sort, view.ascending) {
            (SortKey::Manual, true) => own,
            (SortKey::Manual, false) => own.into_iter().rev().collect(),
            _ => view.sort(own.into_iter()),
        };
    }
    if scope != Scope::All {
        return view.sort(model.books().filter(|b| in_scope(b, scope)));
    }
    match view.home {
        Home::Everything => view.sort(model.books()),
        Home::Unshelved => {
            let shelved = model.shelved();
            view.sort(model.books().filter(|b| !shelved.contains(&b.id)))
        }
    }
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
                self.at = at.filter(|&id| self.model.shelf(id).is_some());
                self.scope = Scope::All;
                self.renaming = None;
            }
            ShelfAction::New => {
                let name = self.model.fresh_shelf_name(self.at);
                if let Ok(id) = self.model.create_shelf(&name, self.at, now) {
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
            ShelfAction::Reorder {
                book,
                shelf,
                before,
            } => {
                self.model.place_before(book, shelf, before, now);
            }
        }
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
