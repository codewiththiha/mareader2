//! The one read of the store an older build wrote, and a browser's only one.
//!
//! A launch with a directory reads the engine's own two files, and the first
//! one after an older build takes the two keys that build left in
//! `eframe::Storage` and moves them there. This is the only module in the crate
//! that names `Storage` (definition of done 7). A browser has no files, so its
//! store stays its store and `apps/mareader-web` keeps writing it.

use std::path::{Path, PathBuf};

use eframe::Storage;
use mareader_engine::save::{self, Loaded};
use mareader_settings::Settings;

/// The window's name, and so the directory its files live in.
pub const APP_ID: &str = "mareader";
/// The key an older build's choices were written under.
pub const SETTINGS_KEY: &str = "settings";
/// Kept apart from settings: it says what was read, not how to read.
pub const LAST_DOCUMENT_KEY: &str = "last_document";

/// Where the engine's files live here; a browser has nowhere.
pub fn state_dir() -> Option<PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        eframe::storage_dir(APP_ID)
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// The store a browser keeps, named here so nothing else has to name it.
pub type Store<'a> = dyn Storage + 'a;

/// What a launch starts with: the engine's files, or the store a browser keeps.
pub fn once(storage: Option<&Store<'_>>, dir: Option<PathBuf>) -> Loaded {
    match dir {
        None => browser(storage),
        Some(dir) => files(storage, &dir),
    }
}

/// A store read as it stands: a browser's every launch, a desktop's first.
pub fn browser(storage: Option<&Store<'_>>) -> Loaded {
    let mut notes = Vec::new();
    let settings = match storage.and_then(|s| s.get_string(SETTINGS_KEY)) {
        None => Settings::default(),
        Some(json) => {
            let loaded = mareader_settings::load(&json);
            notes.extend(loaded.notes);
            loaded.settings
        }
    };
    let held = storage.and_then(|s| s.get_string(LAST_DOCUMENT_KEY));
    let last = match held {
        None => None,
        Some(json) => match serde_json::from_str(&json) {
            Ok(last) => last,
            Err(why) => {
                notes.push(format!("{LAST_DOCUMENT_KEY} could not be read: {why}"));
                None
            }
        },
    };
    Loaded {
        settings,
        last,
        notes,
    }
}

/// The engine's files, with an older build's keys moved in when they are new.
fn files(storage: Option<&Store<'_>>, dir: &Path) -> Loaded {
    let fresh = !dir.join(save::SETTINGS_FILE).exists() && !dir.join(save::SESSION_FILE).exists();
    let held = storage.is_some_and(|s| {
        s.get_string(SETTINGS_KEY).is_some() || s.get_string(LAST_DOCUMENT_KEY).is_some()
    });
    if !fresh || !held {
        return save::load(Some(dir));
    }
    let moved = browser(storage);
    match save::store(dir, &moved.settings, moved.last.as_ref()) {
        Ok(()) => {
            let mut loaded = save::load(Some(dir));
            // A repair made on the way in is said once, at that launch.
            loaded.notes.extend(moved.notes);
            loaded
        }
        Err(why) => {
            let mut kept = moved;
            let note = format!("the stored settings did not move: {why}");
            kept.notes.push(note);
            kept
        }
    }
}

/// A browser's store written: the two keys the engine's files replace.
pub fn keep_browser(app: &crate::MareaderApp, storage: &mut Store<'_>) {
    let settings = mareader_settings::to_json(app.settings());
    storage.set_string(SETTINGS_KEY, settings);
    // Nothing open saves `null`: the next launch starts empty.
    let last = serde_json::to_string(&app.last_document());
    let json = last.unwrap_or_else(|_| "null".to_owned());
    storage.set_string(LAST_DOCUMENT_KEY, json);
}

#[cfg(test)]
mod tests;
