//! The egui front end: the only crate that draws (`docs/architecture.md`).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod app;
pub mod appearance_menu;
pub mod command;
pub mod icons;
pub mod motion;
pub mod panes;
pub mod platform;
pub mod resize;
pub mod settings_window;
pub mod sidebar;
pub mod theme;
pub mod titlebar;
pub mod widgets;

pub use app::MareaderApp;
