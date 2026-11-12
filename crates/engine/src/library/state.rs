//! What the library remembers between frames: its model, where the page
//! stands in it, and the sheet over it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{BuildHasher, RandomState};
use std::path::PathBuf;

use mareader_library::{BookId, DeviceId, Library, ShelfId, disk::Folder};
use mareader_workspace::PaneId;

use crate::documents::OpenRequest;

use super::{Picks, Question, Scope, Sheet};

/// Whether the library's files can be written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Its files are being read; opens wait for their positions.
    Loading,
    #[default]
    Ready,
    /// A newer app's files, or none readable: kept in memory only.
    ReadOnly,
}

/// The library's own state: the part that outlives a frame.
pub struct State {
    pub model: Library,
    /// None: in memory only, as in tests and shots.
    pub folder: Option<Folder>,
    /// Questions waiting for the reader, oldest first.
    pub asks: VecDeque<Question>,
    pub status: Status,
    /// Disk work runs on the frame rather than on a worker.
    pub inline: bool,
    /// The file each pane showed when last looked at, and as which book.
    pub seen: BTreeMap<PaneId, (PathBuf, Option<BookId>)>,
    /// Opens held until the library has loaded.
    pub waiting: Vec<OpenRequest>,
    /// When the last write left, in seconds since the app started.
    pub wrote: f64,
    /// The library page is up in place of the panes.
    pub showing: bool,
    pub scope: Scope,
    /// The shelf open on the page; none is the top level.
    pub at: Option<ShelfId>,
    /// The shelf whose name the breadcrumb is editing in place.
    pub renaming: Option<ShelfId>,
    /// A shelf the rail's tree just made, for it to name.
    pub made: Option<ShelfId>,
    pub picks: Picks,
    /// Shelf rows the list shows unfolded; going elsewhere folds them.
    pub unfolded: BTreeSet<ShelfId>,
    /// The shelf under files dragged in from outside, if any.
    pub file_target: Option<ShelfId>,
    /// The sheet over the page; none while the page is clear of one.
    pub sheet: Option<Sheet>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            model: Library::new(new_device()),
            folder: None,
            status: Status::Ready,
            inline: false,
            seen: BTreeMap::new(),
            waiting: Vec::new(),
            wrote: f64::NEG_INFINITY,
            showing: false,
            scope: Scope::All,
            at: None,
            renaming: None,
            made: None,
            picks: Picks::default(),
            unfolded: BTreeSet::new(),
            file_target: None,
            sheet: None,
            asks: VecDeque::new(),
        }
    }
}

/// This run's own device, so two apps never share a library in step.
fn new_device() -> DeviceId {
    DeviceId(RandomState::new().hash_one(wall_ms()))
}

/// Milliseconds since 1970; the browser preview has no clock here.
pub fn wall_ms() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        since.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}
