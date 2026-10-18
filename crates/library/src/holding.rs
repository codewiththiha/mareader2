//! Where a shelf's books live, as its badge says it.

use crate::book::{Book, Origin};
use crate::folder::Mode;
use crate::id::ShelfId;
use crate::library::Library;
use crate::link::LinkTo;

/// A shelf's badge: its books on disk, copied in, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holding {
    OnDisk,
    Copied,
    Mixed,
}

impl Holding {
    /// How a folder's empty shelf still reads: as its folder does.
    pub fn of(mode: Mode) -> Self {
        match mode.copies() {
            true => Self::Copied,
            false => Self::OnDisk,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OnDisk => "On disk",
            Self::Copied => "Copied",
            Self::Mixed => "Mixed",
        }
    }

    /// The sentence hovering the badge shows.
    pub fn tip(self) -> &'static str {
        match self {
            Self::OnDisk => {
                "These books stay in their folder on disk; the library only remembers where they are."
            }
            Self::Copied => {
                "Every book here is a copy the library keeps — the folder on disk can go."
            }
            Self::Mixed => "Some books are on disk and some are copies the library keeps.",
        }
    }
}

impl Library {
    /// A shelf's badge: its own books, else all below, else its folder.
    pub fn holding(&self, id: ShelfId) -> Option<Holding> {
        let own = self.tally(&[id]);
        let below = || self.tally(&self.subtree(&[id]));
        let mode = || {
            let folders = self.folder_list();
            crate::ground::mode_of(&self.shelves, &folders, id).map(Holding::of)
        };
        own.or_else(below).or_else(mode)
    }

    /// What books on these shelves are; a book's link counts as its book.
    fn tally(&self, shelves: &[ShelfId]) -> Option<Holding> {
        let (mut disk, mut copied) = (false, false);
        for &shelf in shelves {
            let linked = (self.links())
                .filter(|l| l.level.value == Some(shelf))
                .filter_map(|l| match l.target {
                    LinkTo::Book(b) => self.book(b),
                    LinkTo::Shelf(_) => None,
                });
            for book in self.on(shelf).map(|(_, b)| b).chain(linked) {
                match stored(book) {
                    true => copied = true,
                    false => disk = true,
                }
            }
            if disk && copied {
                return Some(Holding::Mixed);
            }
        }
        match (disk, copied) {
            (true, _) => Some(Holding::OnDisk),
            (_, true) => Some(Holding::Copied),
            _ => None,
        }
    }
}

/// Whether the library keeps the book's file itself.
pub(crate) fn stored(book: &Book) -> bool {
    book.home().is_some_and(|l| l.origin == Origin::Store)
}
