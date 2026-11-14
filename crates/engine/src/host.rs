//! The places a machine has, as its front end reads them back.
//!
//! The engine never asks a system where things go: a folder arrives from
//! whoever runs it, and each service's place is a child of one.

use std::path::PathBuf;

/// The places a machine has, and what may run beside them.
///
/// Every path is a directory the host chose; the engine never asks a system
/// where things go, so a browser has an empty machine and a headless client
/// has a temporary one. A place left unnamed stays unattached, and the
/// controller that would have used it keeps its state in memory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Machine {
    /// The library's rows.
    pub library: Option<PathBuf>,
    /// Covers and other files that can be built again.
    pub cache: Option<PathBuf>,
    /// Language packs.
    pub dictionaries: Option<PathBuf>,
    /// The word-level dataset.
    pub vocabulary: Option<PathBuf>,
    /// Downloaded voice models, with the pronunciation pack beside them.
    pub voices: Option<PathBuf>,
    /// The AI service's keys.
    pub ai: Option<PathBuf>,
    /// A helper a debug build may run in the service's place.
    pub ai_helper: Option<PathBuf>,
    /// The voices offered; the catalogue `speech` ships when none is named.
    pub offered: Option<&'static [mareader_speech::Model]>,
}

impl Machine {
    /// A machine with a library in `root` and nothing else: a test's, or a
    /// measurement's.
    pub fn library(root: PathBuf) -> Self {
        Self {
            library: Some(root),
            ..Self::default()
        }
    }

    /// A machine offering `offered` from `dir`, and no other service.
    pub fn voices(dir: PathBuf, offered: &'static [mareader_speech::Model]) -> Self {
        Self {
            voices: Some(dir),
            offered: Some(offered),
            ..Self::default()
        }
    }

    /// The places a desktop launch has, under the folder the window owns.
    ///
    /// `state` holds the engine's own two files and every service's folder;
    /// `cache` is what the platform says may be thrown away.
    pub fn under(
        state: Option<PathBuf>,
        cache: Option<PathBuf>,
        ai_helper: Option<PathBuf>,
    ) -> Self {
        let Some(state) = state else {
            return Self {
                cache,
                ai_helper,
                ..Self::default()
            };
        };
        Self {
            library: Some(state.join("library")),
            cache,
            dictionaries: Some(state.join("dictionaries")),
            vocabulary: Some(state.join("vocabulary")),
            voices: Some(state.join("voice-models")),
            ai: Some(state.join("ai")),
            ai_helper,
            offered: None,
        }
    }
}
