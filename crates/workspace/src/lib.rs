//! The reader workspace as data: up to four panes in a split tree.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod tree;

pub use tree::{
    Bounds, Divider, DividerPath, Edge, MAX_PANES, MIN_PANE_PX, MIN_RATIO, PaneId, PaneTree, Side,
    SplitAxis, TreeError,
};
