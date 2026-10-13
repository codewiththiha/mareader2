//! PDF documents through hayro: open, measure and rasterise pages (L1).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod document;
mod outline;
pub mod sample;
mod title;

pub use document::{
    Document, MAX_PAGES, MAX_RASTER_PIXELS, MAX_RASTER_SIDE, OpenError, PageSize, Raster,
    RenderError,
};
pub use outline::{MAX_DEPTH, OutlineEntry, active_entry};
pub use title::{display_name, file_stem, is_usable_title};
