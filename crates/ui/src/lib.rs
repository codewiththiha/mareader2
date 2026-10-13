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
pub mod gpu;
pub mod icons;
pub mod keymap;
pub mod layout;
pub mod motion;
pub mod outline_panel;
pub mod page_bar;
pub mod pages;
pub mod panes;
pub mod pdf_view;
pub mod platform;
pub mod resize;
pub mod settings_window;
pub mod sidebar;
pub mod theme;
pub mod thumbnails;
pub mod titlebar;
pub mod toast;
pub mod view_menu;
pub mod widgets;
pub mod zoom;

pub use app::MareaderApp;
pub use documents::OpenRequest;

#[cfg(test)]
mod testing {
    use mareader_session::{DocId, Session};

    /// A real `DocId` comes only from opening; a tiny sample will do.
    pub fn doc() -> Result<DocId, &'static str> {
        let mut s = Session::new(0, || {});
        let bytes = std::sync::Arc::new(mareader_pdf::sample::pdf(1, "t"));
        s.open(mareader_session::Source::Bytes(bytes), None);
        let budget = mareader_session::Budget {
            items: 1,
            bytes: usize::MAX,
        };
        s.poll(budget)
            .into_iter()
            .find_map(|e| match e {
                mareader_session::Event::Opened { result: Ok(id), .. } => Some(id),
                _ => None,
            })
            .ok_or("the sample didn't open")
    }
}
