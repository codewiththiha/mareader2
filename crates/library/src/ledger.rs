//! What a walk of a folder does, file by file, and what it gives back.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::folder::{Folder, Tombstone};
use crate::id::{BookId, ContentId, FolderId};

/// What the ledger knows of a book the library holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    pub id: BookId,
    pub path: PathBuf,
    pub missing: bool,
    /// For the library's own copy: the file it was copied from.
    pub source: Option<PathBuf>,
}

/// Content to the book holding it; built fresh, so it never goes stale.
pub type Registry = HashMap<ContentId, Known>;

/// One found file's decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// A book the library does not hold.
    Add,
    /// A held book whose file moved here; a copy's source moves instead.
    Relink(BookId),
    /// The common case: a rescan of an unchanged folder is all skips.
    Skip,
}

/// A rescan's decision: removals, moves and unwatched rungs stay quiet.
pub fn decide(folder: &Folder, registry: &Registry, path: &Path, content: ContentId) -> Action {
    // A tombstone wins over everything, a never-placed file included.
    if folder.is_ignored(content) {
        return Action::Skip;
    }
    match registry.get(&content) {
        // A placed row that went stays gone.
        None if folder.placed.contains(&content) => Action::Skip,
        None => match folder.rung_of(path) {
            Some(rung) if folder.tracks_rung(&rung) => Action::Add,
            _ => Action::Skip,
        },
        Some(known) => known_action(folder, known, path, content),
    }
}

/// An import the reader asked for: the same, with tombstones lifted.
pub fn decide_import(
    folder: &Folder,
    registry: &Registry,
    path: &Path,
    content: ContentId,
) -> Action {
    match registry.get(&content) {
        None => Action::Add,
        Some(known) => match known_action(folder, known, path, content) {
            // Content held elsewhere is a question, not a quiet skip.
            Action::Skip if known.path != path => Action::Add,
            other => other,
        },
    }
}

/// A held content skips at its own address and follows a move.
fn known_action(folder: &Folder, known: &Known, path: &Path, content: ContentId) -> Action {
    if known.path == path || known.source.as_deref() == Some(path) {
        return Action::Skip;
    }
    if folder.placed.contains(&content) || known.missing {
        Action::Relink(known.id)
    } else {
        Action::Skip
    }
}

/// Record a removal in every folder that placed the book.
pub fn tombstone(folders: &mut [Folder], stone: &Tombstone) {
    for folder in folders.iter_mut() {
        if folder.placed.contains(&stone.content) && !folder.is_ignored(stone.content) {
            folder.ignored.push(stone.clone());
        }
    }
}

/// Drop tombstones of books that came back; move-out logs stay.
pub fn prune(folder: &mut Folder, registry: &Registry) {
    folder
        .ignored
        .retain(|stone| stone.moved || !registry.contains_key(&stone.content));
}

/// A book this folder could give the reader back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recovered {
    /// Removed by the reader and not in the library.
    Removed(Tombstone),
    /// Still in the library, but off every shelf of this folder.
    Moved {
        book: BookId,
        /// The name the shelf shows.
        title: String,
        path: PathBuf,
        /// The first shelf it is on, if any.
        home: Option<String>,
    },
}

/// A held book, as the folder's Add menu needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub id: BookId,
    pub title: String,
    pub missing: bool,
    /// Its shelves in order: the name, and the folder whose rung it is.
    pub shelves: Vec<(String, Option<FolderId>)>,
}

/// What a folder's Add menu offers back: removals first, then moves.
pub fn recoverables(folder: &Folder, held: &HashMap<ContentId, Held>) -> Vec<Recovered> {
    let removed = folder
        .ignored
        .iter()
        .filter(|stone| !stone.moved && !held.contains_key(&stone.content))
        .map(|stone| Recovered::Removed(stone.clone()));
    let moved = folder.last_seen.iter().filter_map(|(content, path)| {
        let book = held.get(content)?;
        // A missing card already offers to find the file again.
        let on_ours = book.shelves.iter().any(|(_, f)| *f == Some(folder.id));
        (!book.missing && !on_ours).then(|| Recovered::Moved {
            book: book.id,
            title: book.title.clone(),
            path: path.clone(),
            home: book.shelves.first().map(|(name, _)| name.clone()),
        })
    });
    removed.chain(moved).collect()
}
