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
pub mod disk;
mod extra;
mod id;
mod key;
mod library;
mod marks;
mod merge;
mod query;
mod records;
mod removal;
mod shelves;
mod sources;
#[cfg_attr(target_arch = "wasm32", path = "store_none.rs")]
pub mod store;
mod tracked;
mod view;

pub use absorb::Absorbed;
pub use arrange::Item;
pub use book::{Book, FileStamp, Format, Location, Origin, Position, State, display_title};
pub use clock::{Clock, Field, Stamp};
pub use extra::Extra;
pub use id::{BookId, ContentId, DeviceId, MarkId, ShelfId, SourceId};
pub use library::{Dirty, Library, Measure, Opened};
pub use marks::{Gloss, Mark, MarkKind, NewMark, Pen, Spot};
pub use merge::Merge;
pub use query::{Hit, Query, SUGGEST_LIMIT, Suggestion, Target};
pub use records::{Changes, Kind, Row};
pub use removal::{Imported, Receipt, Removed};
pub use shelves::{Membership, NAME_MAX, Shelf, ShelfError, ShelfRemoved};
pub use sources::{
    AddError, Added, Found, RelinkError, Scan, ScanReport, Shelving, Source, SourceEdit, counts,
};
pub use view::{COLUMNS_MAX, COLUMNS_MIN, CoverFit, Home, Layout, SortKey, View, natural};

#[cfg(test)]
mod absorb_tests;
#[cfg(test)]
mod arrange_tests;
#[cfg(test)]
mod marks_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod removal_tests;
#[cfg(test)]
mod shelves_tests;
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod tests;
