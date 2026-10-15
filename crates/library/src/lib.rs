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
mod library;
mod view;

pub use book::{Book, FileStamp, Format, Location, Origin, Position, State};
pub use clock::{Clock, Field, Stamp};
pub use id::{BookId, ContentId, DeviceId};
pub use library::{Dirty, Library, Measure, Opened};
pub use view::{COLUMNS_MAX, COLUMNS_MIN, CoverFit, Layout, SortKey, View, natural};

#[cfg(test)]
mod tests;
