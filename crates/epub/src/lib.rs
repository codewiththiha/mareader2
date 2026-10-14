//! EPUB books through rbook: chapters read into reflow blocks (L2).
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
mod css;
mod guard;
mod href;
pub mod sample;
mod xhtml;

pub use book::{Book, Destination, Link, MAX_MARKUP, MAX_PICTURE_BYTES, MAX_TOC, TocEntry};
pub use css::MAX_SHEET;
pub use guard::{MAX_ENTRIES, MAX_ENTRY, MAX_TOTAL};
pub use xhtml::{MAX_DEPTH, MAX_LINKS, MAX_PLACES};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    #[error("the file isn't a readable EPUB")]
    Invalid,
    #[error("the book has no text to show")]
    Empty,
    #[error("the book is too large to open safely")]
    TooLarge,
}

/// Whether `bytes` look like a zip, as every EPUB is.
pub fn sniff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}
