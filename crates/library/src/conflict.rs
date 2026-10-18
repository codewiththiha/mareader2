//! Name collisions on a level: does it already hold that name?

use std::path::PathBuf;

use crate::id::{BookId, LinkId, ShelfId};
use crate::words::{free_name, same_name, stem};

/// A level: a shelf's members, or `None` for the library's top level.
pub type Level = Option<ShelfId>;

/// One row on a level: a book, or a link pointing somewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Entry {
    Book(BookId),
    Link(LinkId),
}

/// What is arriving and where: one value for every route in.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrival {
    /// The name being placed, as the level would show it.
    pub name: String,
    /// The row being moved or filed; `None` for a file.
    pub moving: Option<Entry>,
    /// The file being imported; `None` for a move.
    pub file: Option<PathBuf>,
    pub level: Level,
    /// The level a move leaves, if it leaves one.
    pub from: Option<Level>,
    /// The slot a drop pointed at; `None` appends.
    pub index: Option<usize>,
}

impl Arrival {
    /// A file arriving under its own stem.
    pub fn import(file: PathBuf, level: Level, index: Option<usize>) -> Self {
        Self {
            name: stem(&file),
            moving: None,
            file: Some(file),
            level,
            from: None,
            index,
        }
    }

    /// A row being moved or filed onto a level.
    pub fn moved(
        entry: Entry,
        name: impl Into<String>,
        level: Level,
        index: Option<usize>,
    ) -> Self {
        Self {
            name: name.into(),
            moving: Some(entry),
            file: None,
            level,
            from: None,
            index,
        }
    }

    /// Name the level this arrival leaves, for the survivor's sake.
    pub fn leaving(mut self, from: Level) -> Self {
        self.from = Some(from);
        self
    }

    pub fn is_import(&self) -> bool {
        self.file.is_some()
    }
}

/// The row on a level whose name the arrival carries; links never collide.
pub fn collide<'a>(
    members: impl IntoIterator<Item = (Entry, &'a str)>,
    at: &Arrival,
) -> Option<Entry> {
    members.into_iter().find_map(|(entry, name)| {
        let skip = matches!(entry, Entry::Link(_)) || Some(entry) == at.moving;
        (!skip && same_name(name, &at.name)).then_some(entry)
    })
}

/// The shelf on a level whose name an arriving shelf carries.
pub fn collide_shelf<'a>(
    children: impl IntoIterator<Item = (ShelfId, &'a str)>,
    name: &str,
) -> Option<ShelfId> {
    children
        .into_iter()
        .find_map(|(id, held)| same_name(held, name).then_some(id))
}

/// The next free name on a level, counted against everything on it.
pub fn next_name<'a>(names: impl IntoIterator<Item = &'a str> + Clone, name: &str) -> String {
    free_name(name, names)
}

/// What the reader decided about a thing the level already holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Placement {
    /// Place nothing: go to the thing that is there and light it.
    Open,
    /// Land beside it under the next free name.
    KeepBoth,
    /// Fold the arrival into it: the further place wins.
    Merge,
    /// The thing there goes and the arrival takes its slot.
    Replace,
    /// A pointer at the thing there instead of a second one.
    LinkOnly,
}

impl Placement {
    /// A file arriving: nothing to fold and nothing to displace.
    pub const FILE: &'static [Self] = &[Self::Open, Self::KeepBoth, Self::LinkOnly];
    /// A row moved onto a row: the reader holds the arrival.
    pub const MOVE: &'static [Self] = &[Self::Merge, Self::Replace, Self::KeepBoth];
    /// A read-in-place row onto the library's own copy: link, not replace.
    pub const MOVE_KEEPING_BOTH: &'static [Self] = &[Self::Merge, Self::LinkOnly, Self::KeepBoth];
    /// A file the library already holds, or a tree reads in place.
    pub const COVERED: &'static [Self] = &[Self::KeepBoth, Self::Open];
    /// One file of a folder merging into a shelf.
    pub const FOLDER_MERGE: &'static [Self] = &[Self::Merge, Self::Replace, Self::KeepBoth];
    /// A copying folder whose name the level holds.
    pub const SHELF_STORED: &'static [Self] = &[Self::Open, Self::Replace, Self::KeepBoth];
    /// A read-in-place folder whose name the level holds.
    pub const SHELF_IN_PLACE: &'static [Self] = &[Self::LinkOnly, Self::Merge];
}
