//! The machine a desktop launch runs on.
//!
//! It names the two folders this platform says to use, lends the window's
//! clipboard, and hands the file manager and the system's own opener to
//! `system.rs`, which runs them off this thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::Context;
use mareader_engine::host::{Host, Shared};

/// The platform's folders, and the window that draws into them.
pub struct Desk {
    ctx: Context,
    state: Option<PathBuf>,
    cache: Option<PathBuf>,
}

impl Desk {
    /// The machine for a window drawn in `ctx`.
    pub fn new(ctx: Context) -> Shared {
        Arc::new(Self {
            ctx,
            state: mareader_ui_egui::import::state_dir(),
            cache: cache_dir(),
        })
    }
}

impl Host for Desk {
    fn state_dir(&self) -> Option<PathBuf> {
        self.state.clone()
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        self.cache.clone()
    }

    fn set_clipboard(&self, text: String) {
        self.ctx.copy_text(text);
    }

    fn reveal(&self, path: &Path) -> bool {
        crate::system::reveal(path)
    }

    fn open_external(&self, path: &Path) -> bool {
        crate::system::open(path)
    }
}

/// Where rebuildable files such as covers go, as each platform expects.
fn cache_dir() -> Option<PathBuf> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "windows") {
        return var("LOCALAPPDATA").map(|d| d.join("mareader").join("cache"));
    }
    if cfg!(target_os = "macos") {
        return var("HOME").map(|h| h.join("Library/Caches/mareader"));
    }
    let base = var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache")));
    base.map(|d| d.join("mareader"))
}
