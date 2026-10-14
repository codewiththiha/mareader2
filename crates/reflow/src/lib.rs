//! Styled blocks set by parley, cut into pages, drawn by vello_cpu (L1).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod block;
mod flow;
mod fonts;
mod raster;

pub use block::{Block, Emphasis, Role, Span};
pub use flow::{Flow, MAX_PAGES, PAD, PAGE_HEIGHT, PAGE_WIDTH, Typography};
pub use fonts::{Fonts, SANS, SERIF};
pub use raster::{MAX_SIDE, Pixels, Region};
