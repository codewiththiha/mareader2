//! Loose files screened before they land, and the answers to their asks.

use std::collections::HashMap;

use crate::book::Origin;
use crate::clock::Field;
use crate::conflict::{Arrival, Entry, Level, Placement, collide, next_name};
use crate::folder::Found;
use crate::id::{BookId, FolderId, ShelfId};
use crate::library::{Library, Opened};
use crate::link::LinkTo;
use crate::removal::Data;
use crate::run::{Copied, MergeAsk};

/// Why a loose file asks before it lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// A tree read in place holds a book at the file's address.
    Covered(FolderId),
    /// The library holds the file's content as a book.
    AlreadyHave,
    /// The level holds a row of the file's name.
    Name,
}

/// One loose file's question and the row it meets.
#[derive(Clone, Debug, PartialEq)]
pub struct FileAsk {
    pub arrival: Arrival,
    pub found: Found,
    pub existing: Entry,
    pub why: Why,
}

impl FileAsk {
    /// A held file opens or lands beside; a name may also link.
    pub fn offers(&self) -> &'static [Placement] {
        match self.why {
            Why::Name => Placement::FILE,
            _ => Placement::COVERED,
        }
    }
}

/// A batch screened: what came back, what asks, what to copy in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Screened {
    /// Books a covering folder had removed, back where they were.
    pub restored: Vec<BookId>,
    pub asks: Vec<FileAsk>,
    /// Files to copy into the store, then land with [`Library::land_copies`].
    pub clean: Vec<Found>,
}

/// What an answer did, for the sheet to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Nothing landed: the row there is shown.
    Reveal(Entry),
    Landed(BookId),
    Linked,
    /// Refused: an answer not offered, or a copy that never came.
    Nothing,
}

/// A covering tree's verdict on a loose file, before the logs.
enum Fate {
    Ordinary,
    Restore(FolderId),
    Ask(FolderId, BookId),
}

impl Library {
    /// Screens loose files for `level`: covered, held, then named.
    pub fn screen_files(&mut self, found: Vec<Found>, level: Level, wall_ms: u64) -> Screened {
        let mut out = Screened::default();
        let registry = self.registry();
        for file in found {
            let arrival = Arrival::import(file.path.clone(), level, None);
            let ask = |existing, why| FileAsk {
                arrival: arrival.clone(),
                found: file.clone(),
                existing,
                why,
            };
            match self.covered_fate(&file) {
                Fate::Ask(folder, book) => {
                    out.asks.push(ask(Entry::Book(book), Why::Covered(folder)));
                    continue;
                }
                Fate::Restore(folder) => {
                    out.restored
                        .extend(self.restore_covered(folder, &file, wall_ms));
                    continue;
                }
                Fate::Ordinary => {}
            }
            // A missing row is no answer: the file lands as its copy.
            let held = registry.get(&file.measure.content).filter(|k| !k.missing);
            if let Some(held) = held {
                out.asks.push(ask(Entry::Book(held.id), Why::AlreadyHave));
                continue;
            }
            let rows = self.level_rows(level);
            let rows = rows.iter().map(|(e, n)| (*e, n.as_str()));
            match collide(rows, &arrival) {
                Some(existing) => out.asks.push(ask(existing, Why::Name)),
                None => out.clean.push(file),
            }
        }
        out
    }

    /// A living row at the address asks; a log or a move-out restores.
    fn covered_fate(&self, file: &Found) -> Fate {
        let covering: Vec<_> = (self.folders.values())
            .filter(|f| f.mode().in_place() && file.path.starts_with(&f.root))
            .collect();
        let Some(first) = covering.first().map(|f| f.id) else {
            return Fate::Ordinary;
        };
        let content = file.measure.content;
        let placer = covering.iter().find(|f| f.placed.contains(&content));
        if let Some(book) = self.book_at(&file.path) {
            return Fate::Ask(placer.map_or(first, |f| f.id), book);
        }
        let stoned = covering.iter().find(|f| f.tombstone(content).is_some());
        match stoned.or(placer) {
            Some(folder) => Fate::Restore(folder.id),
            None => Fate::Ordinary,
        }
    }

    /// The folder's own book back in place, on the shelf it left.
    fn restore_covered(&mut self, id: FolderId, file: &Found, wall_ms: u64) -> Option<BookId> {
        let mut folder = self.folders.get(&id)?.clone();
        let stone = folder.tombstone(file.measure.content).and_then(|s| s.shelf);
        let (rung, root) = folder.rungs_for(&file.path);
        let book = self.mint_found(&mut folder, file, &HashMap::new(), false, wall_ms)?;
        let standing = |s: &Option<ShelfId>| s.filter(|s| self.shelf(*s).is_some());
        let target = standing(&stone).or(standing(&rung)).or(standing(&root));
        self.folders.insert(id, folder);
        if let Some(shelf) = target {
            self.file(book, shelf, wall_ms);
        }
        self.index();
        self.bump();
        Some(book)
    }

    /// Lands copies made of screened files; a removal's log is spent.
    pub fn land_copies(
        &mut self,
        copies: &[(Found, Copied)],
        level: Level,
        wall_ms: u64,
    ) -> Vec<BookId> {
        let landed: Vec<BookId> = (copies.iter())
            .map(|(file, copy)| {
                let title = self.spend_stone(file);
                self.land_copy(file, copy, title, level, wall_ms)
            })
            .collect();
        self.index();
        self.bump();
        landed
    }

    /// Lifts the log any folder keeps for the file; its name, if kept.
    fn spend_stone(&mut self, file: &Found) -> Option<String> {
        let content = file.measure.content;
        let owner = (self.folders.values()).find(|f| f.tombstone(content).is_some());
        let mut folder = owner?.clone();
        let stone = folder.lift(content);
        self.folders.insert(folder.id, folder);
        stone.map(|s| s.title).filter(|t| !t.is_empty())
    }

    /// One copy made a row: its own, unless its content is new here.
    fn land_copy(
        &mut self,
        file: &Found,
        copy: &Copied,
        title: Option<String>,
        level: Level,
        wall_ms: u64,
    ) -> BookId {
        let opened = Opened {
            path: copy.path.clone(),
            measure: copy.measure,
            format: file.format,
            title: title.unwrap_or_else(|| crate::display_title(file.title.as_deref(), &file.path)),
            author: file.author.clone(),
        };
        let held = self.registry().contains_key(&copy.measure.content);
        let at = self.clock.tick(wall_ms);
        let id = match held {
            true => self.add_independent(&opened, wall_ms),
            false => self.add(&opened, wall_ms, at, false),
        };
        let from = Some(file.path.clone());
        self.place_from(id, &copy.path, copy.measure.stamp, Origin::Store, from);
        if let Some(shelf) = level {
            self.file(id, shelf, wall_ms);
        }
        id
    }

    /// Writes a loose file's answer; "keep both" passes the copy made.
    pub fn answer_file(
        &mut self,
        ask: &FileAsk,
        answer: Placement,
        copy: Option<&Copied>,
        wall_ms: u64,
    ) -> Answer {
        if !ask.offers().contains(&answer) {
            return Answer::Nothing;
        }
        let level = ask.arrival.level;
        match (answer, ask.existing) {
            (Placement::Open, existing) => Answer::Reveal(existing),
            (Placement::LinkOnly, Entry::Book(book)) => {
                let linked = self.link_to(LinkTo::Book(book), level, wall_ms);
                linked.map_or(Answer::Nothing, |_| Answer::Linked)
            }
            (Placement::KeepBoth, _) => {
                let Some(copy) = copy else {
                    return Answer::Nothing;
                };
                let name = self.free_on(level, &ask.arrival.name);
                let book = self.land_copy(&ask.found, copy, Some(name), level, wall_ms);
                self.index();
                self.bump();
                Answer::Landed(book)
            }
            _ => Answer::Nothing,
        }
    }

    /// The next free name on a level, rows and links counted.
    pub(crate) fn free_on(&self, level: Level, name: &str) -> String {
        let rows = self.level_rows(level);
        next_name(rows.iter().map(|(_, n)| n.as_str()), name)
    }

    /// Writes a folder merge's answer; a copying folder passes its copy.
    pub fn answer_merge(
        &mut self,
        ask: &MergeAsk,
        answer: Placement,
        copy: Option<&Copied>,
        wall_ms: u64,
    ) -> Answer {
        if !Placement::FOLDER_MERGE.contains(&answer) {
            return Answer::Nothing;
        }
        let Entry::Book(existing) = ask.existing else {
            return Answer::Nothing;
        };
        let file = &ask.found;
        let same_file = self.book(existing).and_then(|b| b.path()) == Some(file.path.as_path());
        let in_place = self.folder(ask.folder).is_some_and(|f| f.mode().in_place());
        // Read in place, the same file is one book: two would be twins.
        let answer = match (answer, in_place && same_file) {
            (Placement::KeepBoth, true) => Placement::Merge,
            (answer, _) => answer,
        };
        let level = ask.arrival.level;
        let answered = match answer {
            Placement::KeepBoth => {
                let name = self.free_on(level, &ask.arrival.name);
                self.land_answer(ask, Some(name), copy, wall_ms)
            }
            Placement::Replace => {
                let slot = self.slot_of(existing, level);
                self.remove_books(&[existing], Data::Delete, wall_ms);
                let landed = self.land_answer(ask, None, copy, wall_ms);
                if let (Answer::Landed(book), Some(slot)) = (&landed, slot) {
                    self.set_slot(*book, level, slot, wall_ms);
                }
                landed
            }
            _ => {
                if same_file {
                    self.relink_found(existing, file);
                }
                Answer::Landed(existing)
            }
        };
        self.settle(ask.folder, file);
        self.index();
        self.bump();
        answered
    }

    /// The answer's file landed on its level: in place, or its copy.
    fn land_answer(
        &mut self,
        ask: &MergeAsk,
        name: Option<String>,
        copy: Option<&Copied>,
        wall_ms: u64,
    ) -> Answer {
        let Some(mut folder) = self.folders.get(&ask.folder).cloned() else {
            return Answer::Nothing;
        };
        let level = ask.arrival.level;
        let book = match folder.mode().in_place() {
            true => self.mint_found(
                &mut folder,
                &ask.found,
                &HashMap::new(),
                name.is_some(),
                wall_ms,
            ),
            false => copy.map(|c| self.land_copy(&ask.found, c, name.clone(), level, wall_ms)),
        };
        self.folders.insert(ask.folder, folder);
        let Some(book) = book else {
            return Answer::Nothing;
        };
        if let Some(name) = name {
            self.rename_book(book, &name, wall_ms);
        }
        if let Some(shelf) = level {
            self.file(book, shelf, wall_ms);
        }
        Answer::Landed(book)
    }

    /// The folder placed the file: its log is spent.
    fn settle(&mut self, id: FolderId, file: &Found) {
        let Some(mut folder) = self.folders.get(&id).cloned() else {
            return;
        };
        folder.placed.insert(file.measure.content);
        folder.lift(file.measure.content);
        let seen = (file.measure.content, file.path.clone());
        if !folder.last_seen.contains(&seen) {
            folder.last_seen.push(seen);
        }
        self.folders.insert(id, folder);
    }

    /// Where a row sits on its level, for a replacement to take.
    pub(crate) fn slot_of(&self, book: BookId, level: Level) -> Option<String> {
        match level {
            Some(shelf) => (self.members.get(&(shelf, book))).map(|m| m.key.value.clone()),
            None => self.book(book).map(|b| b.key.value.clone()),
        }
    }

    pub(crate) fn set_slot(&mut self, book: BookId, level: Level, slot: String, wall_ms: u64) {
        let at = self.clock.tick(wall_ms);
        match level {
            Some(shelf) => {
                if let Some(member) = self.members.get_mut(&(shelf, book)) {
                    member.key = Field::new(slot, at);
                }
            }
            None => {
                if let Some(row) = self.books.get_mut(&book) {
                    row.key = Field::new(slot, at);
                }
            }
        }
    }
}
