//! The headless knobs, parsed: one key, one command.
//!
//! `set_option`'s keys began as a private string channel into the front end
//! (finding F7): a shot, an example or a query string named a dial and the
//! window's own code turned it into state. What most of those keys set is the
//! engine's — a setting by its dotted path, as `Command::SetField` — so the
//! parsing lives here, where a front end, a command line and a test all meet
//! the same answer. A front end keeps the keys that read what is set so far,
//! and the ones that name its toolkit.
//!
//! `None` is the answer for a key this module does not own, which is how a
//! front end's own keys stay reachable: it asks here first and falls through.

mod appearance;
mod library;
mod panes;
mod study;

use crate::command::Command;

/// The commands a key asks for, in order; `None` when it is a front end's own.
///
/// A batch, because one key can ask for two writes: `sort=title:desc` sets the
/// key and the way it runs, and the settings hold them apart.
pub fn parse(key: &str, value: &str) -> Option<Result<Vec<Command>, String>> {
    appearance::set(key, value)
        .or_else(|| library::set(key, value))
        .or_else(|| panes::set(key, value))
        .or_else(|| study::set(key, value))
}

/// A setting written by its dotted path, as the session's `put` takes it.
pub(super) fn field<T: serde::Serialize>(path: &str, value: T) -> Result<Vec<Command>, String> {
    let value = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    Ok(vec![Command::SetField {
        path: path.into(),
        value,
    }])
}

/// `true`, and only `true`, is on.
pub(super) fn flag(value: &str) -> bool {
    value == "true"
}

/// A number, with the key's own hint when the value is not one.
pub(super) fn number<T: std::str::FromStr>(value: &str, hint: &str) -> Result<T, String> {
    value.parse::<T>().map_err(|_| hint.to_owned())
}

/// One of `choices` by name, ignoring case; `hint` is the refusal.
pub(super) fn named<T: Copy>(value: &str, choices: &[(&str, T)], hint: &str) -> Result<T, String> {
    let found = choices
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(value))
        .map(|(_, some)| *some);
    found.ok_or_else(|| hint.to_owned())
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
