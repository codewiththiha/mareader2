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
pub mod documents;
pub mod files;
pub mod icons;
pub mod keymap;
pub mod motion;
pub mod pages;
pub mod panes;
pub mod pdf_view;
pub mod platform;
pub mod resize;
pub mod settings_window;
pub mod sidebar;
pub mod theme;
pub mod titlebar;
pub mod toast;
pub mod view_menu;
pub mod widgets;

pub use app::MareaderApp;
pub use documents::OpenRequest;
