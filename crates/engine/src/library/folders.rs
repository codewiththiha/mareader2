//! What a folder walk keeps between frames: the walk on its way, the work
//! waiting on the worker, and the folders no walk enters.
//!
//! The walking itself runs on the library's worker. This is the state that
//! answers it, so a walk's news lands on a model that remembers the walk.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;

use mareader_library::picks::Pick;
use mareader_library::{
    Asked, Book, ContentId, FolderId, Found, Options, Origin, Placement, Plan, Planned, ShelfId,
};

use super::{CopyWork, Question, RunId, State, Tag};

/// Seconds between rescans when the window keeps regaining focus.
pub const RESCAN_GAP: f64 = 2.0;
/// Seconds before looking again at files still being written.
pub const SETTLE_WAIT: f64 = 3.5;

/// A walk on its way: what it walks, as what, and its card.
#[derive(Clone, Debug)]
pub struct Going {
    pub root: PathBuf,
    pub opts: Options,
    pub plan: Plan,
    pub asked: Asked,
    pub run: Option<RunId>,
    /// Files the walk found, for the run's last word.
    pub found: usize,
}

/// Work waiting on the worker, under the tag its news carries.
#[derive(Debug)]
pub enum Pending {
    /// A folder walking; a rescan names the folder it walks.
    Walk(Going, Option<FolderId>),
    /// A planned walk's copies, landed once made.
    Land(Box<Planned>, Going),
    /// Loose files measured, then screened onto the level.
    Files(Option<ShelfId>, Option<RunId>),
    /// Screened files copied, then landed onto the level.
    Loose(Box<Screening>),
    /// An answer's copy, then the answer written.
    Keep(Box<Question>, Placement),
    /// Books read in place copied, then the gesture that owed them.
    Depart(Box<CopyWork>),
    /// A removed book's file measured, then copied if its folder copies.
    Restore(FolderId, ContentId),
    Restored(FolderId, ContentId, Box<Found>),
    /// The picks' books copied, then the duplicates made.
    Duplicate(Vec<Pick>),
}

/// Loose files measured, on their way onto a level.
#[derive(Debug)]
pub struct Screening {
    pub to: Option<ShelfId>,
    pub run: Option<RunId>,
    pub files: Vec<Found>,
    /// Questions the screen queued.
    pub asked: usize,
}

/// When folders are walked, what they said, and what waits on the worker.
#[derive(Debug, Default)]
pub struct Folders {
    pub next: Tag,
    pub pending: HashMap<Tag, Pending>,
    /// Watched folders walking quietly now.
    pub walking: BTreeSet<FolderId>,
    /// A rescan asked for while one ran.
    pub again: bool,
    /// When the last rescan left, in seconds since the app started.
    pub last: Option<f64>,
    pub settle_at: Option<f64>,
    pub focused: bool,
    pub offline: BTreeSet<FolderId>,
    /// Folders dropped while a sheet was up, asked about in turn.
    pub later: VecDeque<PathBuf>,
    /// The import sheet's last answers, the next one's start.
    pub opts: Option<Options>,
}

impl State {
    /// The library's own folders, which no walk enters.
    pub fn avoid(&self) -> Vec<PathBuf> {
        let own = self.folder.as_ref().map(|f| f.root().to_path_buf());
        own.into_iter().chain(self.cache.clone()).collect()
    }
}

/// The folder an absent book's last address belongs to, if offline.
pub fn offline(book: &Book, offline: &BTreeSet<FolderId>) -> bool {
    book.missing()
        && book.locations.iter().any(|l| match l.origin {
            Origin::Folder(id) => offline.contains(&id),
            Origin::Opened | Origin::Store => false,
        })
}
