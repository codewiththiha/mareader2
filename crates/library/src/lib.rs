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

mod book;
mod clock;
pub mod disk;
mod id;
mod key;
mod library;
mod query;
mod removal;
mod shelves;
mod sources;
mod view;

pub use book::{Book, FileStamp, Format, Location, Origin, Position, State, display_title};
pub use clock::{Clock, Field, Stamp};
pub use id::{BookId, ContentId, DeviceId, ShelfId, SourceId};
pub use library::{Dirty, Library, Measure, Opened};
pub use query::{Hit, Query, SUGGEST_LIMIT, Suggestion, Target};
pub use removal::{Imported, Receipt, Removed};
pub use shelves::{Membership, NAME_MAX, Shelf, ShelfError, ShelfRemoved};
pub use sources::{AddError, Added, Found, RelinkError, Scan, ScanReport, Source, SourceEdit};
pub use view::{COLUMNS_MAX, COLUMNS_MIN, CoverFit, Home, Layout, SortKey, View, natural};

#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod removal_tests;
#[cfg(test)]
mod shelves_tests;
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod tests;
