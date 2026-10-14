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
mod picture;
mod raster;

pub use block::{Align, Block, Content, Emphasis, Frame, Role, Row, Span, Table, Text};
pub use flow::{Flow, MAX_PAGES, PAD, PAGE_HEIGHT, PAGE_WIDTH, Typography};
pub use fonts::{Fonts, MONO, SANS, SERIF};
pub use picture::{MAX_PICTURE_PIXELS, MAX_PICTURE_SIDE, Picture, PictureError};
pub use raster::{MAX_SIDE, Pixels, Region};
