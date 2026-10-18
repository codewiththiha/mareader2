//! Hand moves on the page: the copy they may owe, then the landing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mareader_library::{BookId, Copied, Data, Departure, Found, LinkId, Move, Moved, ShelfId};

use super::asks::Question;
use super::books::counted;
use super::{LibraryState, PickAction, Sheet, wall_ms};
use crate::command::{Command, Commands};
use crate::toast::Toast;

/// A gesture waiting on the copy ask, with what finishing it needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyWork {
    /// Books and shelves carried off their folder's ground.
    Move { mv: Move, owed: Departure },
    /// A rung taken apart: its own books come up as copies.
    Apart(ShelfId),
    /// A removal whose rungs carry books read in place.
    Removal {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
        copies: Vec<BookId>,
        forget: bool,
    },
}

impl CopyWork {
    /// The books that become copies, each once.
    pub fn books(&self, model: &mareader_library::Library) -> Vec<BookId> {
        match self {
            Self::Move { owed, .. } => owed.copies(),
            Self::Apart(id) => model.rung_books(*id, false),
            Self::Removal { copies, .. } => copies.clone(),
        }
    }
}

/// The copy ask's answers; Cancel just closes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyAnswer {
    Copy,
    /// Put shelves back, or let the folder make them again.
    Without,
}

impl LibraryState {
    /// Lands a move, asking first if books must leave their folder.
    pub(super) fn carry(&mut self, mv: Move, out: &mut Commands) {
        let owed = self.model.departure(&mv);
        match owed.is_empty() {
            true => self.land_moved(&mv, &[], out),
            false => self.ask_copy(CopyWork::Move { mv, owed }),
        }
    }

    /// Takes a shelf apart; a rung with books read in place asks first.
    pub(super) fn apart(&mut self, id: ShelfId, out: &mut Commands) {
        match self.model.rung_books(id, false).is_empty() {
            true => self.take_apart(id, out),
            false => self.ask_copy(CopyWork::Apart(id)),
        }
    }

    /// The remove sheet's answer: the cascade counted in, copies asked.
    pub(super) fn remove_entries(
        &mut self,
        (books, shelves): (&[BookId], &[ShelfId]),
        cascade: bool,
        forget: bool,
        out: &mut Commands,
    ) {
        self.folders.sheet = None;
        let going = self.model.removing(books, shelves, cascade);
        if going.copies.is_empty() {
            self.remove(&going.books, &going.shelves, forget, out);
            return;
        }
        self.ask_copy(CopyWork::Removal {
            books: going.books,
            shelves: going.shelves,
            copies: going.copies,
            forget,
        });
    }

    fn ask_copy(&mut self, work: CopyWork) {
        self.folders.sheet = Some(Sheet::Copy(Box::new(work)));
    }

    /// The copy ask answered: copies first, or the way without them.
    pub(super) fn answer_copy(&mut self, answer: CopyAnswer, out: &mut Commands) {
        let Some(Sheet::Copy(work)) = self.folders.sheet.take() else {
            return;
        };
        if answer == CopyAnswer::Without {
            self.without(*work, out);
            return;
        }
        let books = work.books(&self.model);
        let files: Vec<Found> = books.iter().filter_map(|b| self.found_of(*b)).collect();
        match files.is_empty() {
            true => self.departed(*work, &HashMap::new(), out),
            false => self.copy_out(work, files, out),
        }
    }

    /// A book as the copier takes it: its file read in place.
    pub(super) fn found_of(&self, id: BookId) -> Option<Found> {
        let book = self.model.book(id)?;
        let home = book.home()?;
        Some(Found {
            path: home.path.clone(),
            format: book.format,
            measure: mareader_library::Measure {
                content: book.content,
                stamp: home.stamp,
            },
            title: None,
            author: None,
        })
    }

    fn without(&mut self, work: CopyWork, out: &mut Commands) {
        match work {
            CopyWork::Move { mv, .. } => {
                let back = self.model.send_home(&mv, wall_ms());
                let note = match back.as_slice() {
                    [] => return,
                    [one] => format!("Put “{}” back in its place.", self.shelf_name(*one)),
                    many => format!("Put {} back in their places.", many.len()),
                };
                out.push(Command::Notify(Toast::info(note)));
            }
            CopyWork::Removal {
                books,
                shelves,
                forget,
                ..
            } => self.remove(&books, &shelves, forget, out),
            CopyWork::Apart(_) => {}
        }
    }

    /// The copies made: each book departs, then the gesture finishes.
    pub(super) fn departed(
        &mut self,
        work: CopyWork,
        copies: &HashMap<PathBuf, Copied>,
        out: &mut Commands,
    ) {
        let wall = wall_ms();
        let books = work.books(&self.model);
        let mut done = Vec::new();
        for &id in &books {
            let path = self.model.book(id).and_then(|b| b.path());
            let copy = path.map(Path::to_path_buf).and_then(|p| copies.get(&p));
            if copy.is_some_and(|c| self.model.depart(id, c, wall)) {
                done.push(id);
            }
        }
        match work {
            CopyWork::Move { mv, .. } => self.land_moved(&mv, &done, out),
            // A copy that failed leaves the rung standing.
            CopyWork::Apart(id) if done.len() == books.len() => self.take_apart(id, out),
            CopyWork::Apart(_) => {}
            CopyWork::Removal {
                books,
                shelves,
                forget,
                ..
            } => self.remove(&books, &shelves, forget, out),
        }
    }

    /// Lands what's clean; same-name books join the questions.
    fn land_moved(&mut self, mv: &Move, departed: &[BookId], out: &mut Commands) {
        let on = |b: &BookId, s: Option<ShelfId>| {
            s.is_some_and(|s| self.model.shelves_of(*b).contains(&s))
        };
        let already = mv.books.iter().filter(|b| on(b, mv.to)).count();
        let leaving = (mv.books.iter())
            .filter(|b| !mv.keep && mv.from != mv.to && on(b, mv.from))
            .count();
        let moved = self.model.land_move(mv, departed, wall_ms());
        if mv.books.len() + mv.shelves.len() > 1 {
            self.picks.apply(PickAction::Clear);
        }
        let note = self.moved_note(mv, &moved, (already, leaving));
        self.folders
            .asks
            .extend(moved.asks.into_iter().map(Question::Move));
        out.extend(note.map(|n| Command::Notify(Toast::info(n))));
    }

    fn moved_note(
        &self,
        mv: &Move,
        moved: &Moved,
        (already, leaving): (usize, usize),
    ) -> Option<String> {
        if let Some(&shelf) = moved.stayed.first() {
            let name = self.shelf_name(shelf);
            return Some(format!("“{name}” stayed: its books' copies weren't made."));
        }
        let title = |id: &BookId| self.model.book(*id).map(|b| b.title.value.clone());
        let link = |id: &LinkId| self.model.link(*id).map(|l| l.name.value.clone());
        let counts = (moved.books, moved.shelves, moved.links);
        let what = match (counts, &mv.books[..], &mv.shelves[..], &mv.links[..]) {
            ((0, 0, 0), ..) => return None,
            ((1, 0, 0), [one], ..) => format!("“{}”", title(one).unwrap_or_default()),
            ((0, 1, 0), _, [one], _) => format!("“{}”", self.shelf_name(*one)),
            ((0, 0, 1), .., [one]) => format!("“{}”", link(one).unwrap_or_default()),
            ((b, s, l), ..) => listed(&[
                (b, "book", "books"),
                (s, "shelf", "shelves"),
                (l, "link", "links"),
            ]),
        };
        let there = match mv.to {
            Some(to) => format!("“{}”", self.shelf_name(to)),
            None => "the top of the library".into(),
        };
        let note = match (mv.to, mv.from) {
            (_, _) if moved.shelves + moved.links == 0 && already == moved.books => {
                match moved.books {
                    1 => format!("{what} is already on {there}."),
                    _ => format!("They're already on {there}."),
                }
            }
            (None, Some(from)) if moved.shelves == 0 => {
                format!("Took {what} off “{}”.", self.shelf_name(from))
            }
            // Shelves and links never stay behind: they always move.
            _ if leaving + moved.shelves + moved.links > 0 => format!("Moved {what} to {there}."),
            _ => format!("Added {what} to {there}."),
        };
        Some(note)
    }

    /// Takes a shelf apart now: its shelves move up, a rung's books too.
    pub(super) fn take_apart(&mut self, id: ShelfId, out: &mut Commands) {
        let name = self.shelf_name(id);
        let home = self.model.seat(id).flatten();
        let Some(done) = self.model.remove_shelf(id, wall_ms()) else {
            return;
        };
        self.settle();
        let note = match (done.books, home) {
            (0, _) => format!("Took “{name}” apart."),
            (_, Some(home)) => format!(
                "Took “{name}” apart. Its books are on “{}” now.",
                self.shelf_name(home)
            ),
            (_, None) => format!("Took “{name}” apart. Its books stay in the library."),
        };
        out.push(Command::Notify(Toast::info(note)));
    }

    /// Removes books, then shelves deepest first, and says so.
    pub(super) fn remove(
        &mut self,
        books: &[BookId],
        shelves: &[ShelfId],
        forget: bool,
        out: &mut Commands,
    ) {
        let now = wall_ms();
        let first = books.first().and_then(|id| self.model.book(*id));
        let first = first.map(|b| b.title.value.clone()).unwrap_or_default();
        let shelf = shelves
            .first()
            .map(|s| self.shelf_name(*s))
            .unwrap_or_default();
        let receipt = self.model.receipt(books);
        let data = match forget {
            true => Data::Delete,
            false => Data::Keep,
        };
        let done = self.model.remove_books(books, data, now);
        let gone = (shelves.iter())
            .filter(|id| self.model.remove_shelf(**id, now).is_some())
            .count();
        self.picks.apply(PickAction::Clear);
        self.settle();
        let said = match (done, gone) {
            (0, 0) => return,
            (1, 0) => format!("Removed “{first}”."),
            (n, 0) => format!("Removed {n} books."),
            (0, 1) => format!("Removed “{shelf}”."),
            (0, s) => format!("Removed {s} shelves."),
            (n, s) => format!(
                "Removed {} and {}.",
                counted(n, "book", "books"),
                counted(s, "shelf", "shelves")
            ),
        };
        let note = match receipt.hidden {
            0 => said,
            _ => format!("{said} The folder's Add menu can give them back."),
        };
        out.push(Command::Notify(Toast::info(note)));
    }
}

/// "2 books, 1 shelf and 3 links": the counts that aren't nought.
fn listed(counts: &[(usize, &str, &str)]) -> String {
    let parts: Vec<String> = (counts.iter())
        .filter(|(n, ..)| *n > 0)
        .map(|&(n, one, many)| counted(n, one, many))
        .collect();
    match parts.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}
