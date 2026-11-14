//! The shelf's keys: what a folder does, and how the books are laid out.
//!
//! What stays in a front end reads the books as they stand — `select` and
//! `remove` count from the most recently read, `shelf` walks the shelves it can
//! see — or fills a fixture, as `books`, `shelves`, `copies`, `runs` and
//! `organize` do.

use std::path::PathBuf;

use mareader_library::{CoverFit, Home, Layout, Options, SortKey};

use super::field;
use crate::command::Command;
use crate::library::{Action, SearchAction};

/// The shelf's keys; `None` for a key that is not one of them.
pub(super) fn set(key: &str, value: &str) -> Option<Result<Vec<Command>, String>> {
    let parsed = match key {
        "import" => ask_folder(value),
        "folder" => read_folder(value),
        "search" => Ok(vec![Command::Library(Action::Search(SearchAction::Text(
            value.to_owned(),
        )))]),
        "layout" => layout(value),
        "columns" => field("library.columns", value.parse::<usize>().ok()),
        "cover" => cover(value),
        "sort" => sort(value),
        "home" => home(value),
        _ => return None,
    };
    Some(parsed)
}

/// The import sheet for a folder, as Add ▸ Folder… asks.
fn ask_folder(value: &str) -> Result<Vec<Command>, String> {
    let ask = Action::AskFolder(PathBuf::from(value));
    Ok(vec![Command::Library(ask)])
}

/// A folder read in place: watched, every file counted, so its books show.
fn read_folder(value: &str) -> Result<Vec<Command>, String> {
    let opts = Options {
        watch: true,
        min_size: 0,
        ..Options::default()
    };
    let import = Action::ImportFolder {
        root: PathBuf::from(value),
        opts,
    };
    Ok(vec![Command::Library(import)])
}

fn layout(value: &str) -> Result<Vec<Command>, String> {
    let some = match value {
        "list" => Layout::List,
        _ => Layout::Grid,
    };
    field("library.layout", some)
}

fn cover(value: &str) -> Result<Vec<Command>, String> {
    let some = match value {
        "crop" => CoverFit::Crop,
        _ => CoverFit::Fit,
    };
    field("library.cover", some)
}

fn home(value: &str) -> Result<Vec<Command>, String> {
    let some = match value {
        "unshelved" => Home::Unshelved,
        _ => Home::Everything,
    };
    field("library.home", some)
}

/// The key and the way it runs, which the settings hold apart.
fn sort(value: &str) -> Result<Vec<Command>, String> {
    let hint = "sort: arranged, last_read, title, author or added";
    let (key, way) = value.split_once(':').unwrap_or((value, ""));
    let flat = key.replace('_', "");
    let key = SortKey::ALL
        .into_iter()
        .find(|k| format!("{k:?}").eq_ignore_ascii_case(&flat))
        .ok_or(hint)?;
    let some = match way {
        "asc" => true,
        "desc" => false,
        _ => key.natural_ascending(),
    };
    let mut commands = field("library.sort", key)?;
    commands.extend(field("library.ascending", some)?);
    Ok(commands)
}
