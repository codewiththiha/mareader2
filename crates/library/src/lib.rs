//! The library as data (L1): books, positions, pure rules; see
//! `docs/library.md`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod absorb;
mod arrange;
mod book;
mod clock;
pub mod conflict;
pub mod disk;
mod extra;
mod folder;
pub mod ground;
mod id;
mod key;
pub mod ledger;
mod library;
mod link;
mod marks;
mod merge;
mod query;
mod records;
mod removal;
pub mod run;
mod shelves;
mod sources;
#[cfg_attr(target_arch = "wasm32", path = "store_none.rs")]
pub mod store;
mod tracked;
mod tree;
mod view;
mod words;

pub use absorb::Absorbed;
pub use arrange::Item;
pub use book::{Book, FileStamp, Format, Location, Origin, Position, State, display_title};
pub use clock::{Clock, Field, Stamp};
pub use conflict::{Arrival, Entry, Level, Placement};
pub use extra::Extra;
pub use folder::{
    Folder, MIN_SIZE_MAX, MIN_SIZE_STEP, Mode, Options, Rungs, Tombstone, dir_of_rung, parent_key,
};
pub use id::{BookId, ContentId, DeviceId, LinkId, MarkId, ShelfId, SourceId};
pub use ledger::{Held, Recovered};
pub use library::{Dirty, Library, Measure, Opened};
pub use link::{Link, LinkTo};
pub use marks::{Gloss, Mark, MarkKind, NewMark, Pen, Spot, Style};
pub use merge::Merge;
pub use query::{Hit, Query, SUGGEST_LIMIT, Suggestion, Target};
pub use records::{Changes, Kind, Row};
pub use removal::{Data, Imported, Receipt, Removed};
pub use run::{Asked, Copied, Landed, MergeAsk, Plan, Planned};
pub use shelves::{Membership, NAME_MAX, Shelf, ShelfError, ShelfKind, ShelfRemoved};
pub use sources::{
    AddError, Added, Found, RelinkError, Scan, ScanReport, Shelving, Source, SourceEdit, counts,
};
pub use tree::Duplicated;
pub use view::{COLUMNS_MAX, COLUMNS_MIN, CoverFit, Home, Layout, SortKey, View, natural};
pub use words::{free_name, human_age, human_size, plural, same_name};

#[cfg(test)]
mod absorb_tests;
#[cfg(test)]
mod arrange_tests;
#[cfg(test)]
mod conflict_tests;
#[cfg(test)]
mod folder_tests;
#[cfg(test)]
mod ground_tests;
#[cfg(test)]
mod ledger_tests;
#[cfg(test)]
mod marks_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod removal_tests;
#[cfg(test)]
mod run_tests;
#[cfg(test)]
mod shelves_tests;
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod words_tests;
