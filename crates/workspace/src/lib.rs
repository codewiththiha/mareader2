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

mod drop;
mod lift;
mod tree;

pub use drop::DropTarget;
pub use lift::{LiftTarget, resolve as lift_target};

pub use tree::{
    Bounds, Divider, DividerPath, Edge, MAX_PANES, MIN_PANE_PX, MIN_RATIO, PaneId, PaneTree, Side,
    SplitAxis, TreeError,
};
