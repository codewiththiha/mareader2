//! The app without a window: what outlives a frame or reaches disk (L5).
//!
//! Nothing here names a toolkit (`docs/architecture.md` § Layers). A front end
//! holds one session and reaches it only through commands. Both arrive in P2;
//! no item crosses crates until then, so no shim can outlive this phase.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]
