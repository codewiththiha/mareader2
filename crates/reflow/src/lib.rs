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
mod hyphen;
mod label;
mod page_text;
mod picture;
mod raster;

pub use block::{
    Align, Block, Boxed, CHAR_BYTES, Cell, Content, Edge, Emphasis, Frame, GridRow, LAYOUT_BYTES,
    Length, Link, Look, Role, Row, Span, Table, Text, VAlign, clip,
};
pub use flow::{
    Anchor, Band, COLUMN_SCALE, Design, Flow, MAX_LAID, MAX_PAGES, PAD, PAGE_HEIGHT, PAGE_WIDTH,
    PageLink, Typography,
};
pub use fonts::{BookFace, FACES, Face, Faces, Family, Fonts, Kind, MONO, SANS, SERIF, has_family};
pub use label::{Fallback, Lines, fallbacks, label, label_lines, measure_lines};
pub use page_text::{PageChar, PageLine};
pub use picture::{MAX_PICTURE_PIXELS, MAX_PICTURE_SIDE, Picture, PictureError};
pub use raster::{MAX_SIDE, Pixels, Region};
