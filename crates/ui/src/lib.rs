//! The egui front end (L7). The look it must match: `docs/look.md`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod app;
pub mod icons;
pub mod motion;
pub mod panes;
pub mod platform;
pub mod resize;
pub mod theme;
pub mod titlebar;
pub mod widgets;

pub use app::MareaderApp;
