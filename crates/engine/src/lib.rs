//! The app without a window: what outlives a frame or reaches disk (L5).
//!
//! Nothing here names a toolkit (`docs/architecture.md` § Layers). What it
//! holds is what outlives a frame or reaches disk; a front end draws it. The
//! session and the commands that change it arrive in P2, and until then this
//! crate is a library of rules its front end calls directly.
//!
//! Paths here keep their front-end modules (`texts`, `paper`, `read_aloud`),
//! because a move must not rename what it moves.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

pub mod dict;
pub mod documents;
pub mod export;
pub mod notes_panel;
pub mod page_ai;
pub mod pane;
pub mod pane_texts;
pub mod paper;
pub mod play;
pub mod read_aloud;
pub mod reading_place;
pub mod reveal;
pub mod route;
pub mod sample;
/// Which settings apply to the focused pane's document.
pub mod scope;
pub mod settings;
pub mod stable_hash;
pub mod texts;
pub mod transcript;
pub mod watch;
pub mod word_marks;
