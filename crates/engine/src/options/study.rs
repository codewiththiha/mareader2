//! The study keys that write a setting: the language a meaning arrives in.
//!
//! Everything else here is a front end's: a staged selection, the dictionary's
//! window and page, who explains a word, and the preset built from the settings
//! as they stand.

use super::field;
use crate::command::Command;

/// The study keys; `None` for a key that is not one of them.
pub(super) fn set(key: &str, value: &str) -> Option<Result<Vec<Command>, String>> {
    let parsed = match key {
        "meaning" => meaning(value),
        _ => return None,
    };
    Some(parsed)
}

/// The meaning's language by tag, e.g. `my`.
fn meaning(value: &str) -> Result<Vec<Command>, String> {
    mareader_settings::language_name(value).ok_or("meaning: a tag, e.g. my")?;
    field("ai.tasks.explain.language", Some(value.to_owned()))
}
