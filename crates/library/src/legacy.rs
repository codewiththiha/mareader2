//! Folders saved before ledgers: read once, kept as folders from then on.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;

use crate::book::{Book, Format, Origin};
use crate::clock::Field;
use crate::folder::{Folder, MIN_SIZE_MAX, Options, Tombstone};
use crate::id::{BookId, FolderId, ShelfId};
use crate::library::Library;
use crate::shelves::ShelfKind;

/// A folder row as Mareader 0.0.1 wrote it.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct OldFolder {
    id: FolderId,
    root: PathBuf,
    formats: BTreeSet<Format>,
    /// `formats` were the ones left out.
    #[serde(default)]
    exclude: bool,
    #[serde(default)]
    min_size: u64,
    #[serde(default)]
    watch: bool,
    #[serde(default)]
    added: u64,
    /// Books removed from it: tombstones now.
    #[serde(default)]
    hidden: BTreeSet<BookId>,
    #[serde(default)]
    shelving: Shelving,
    /// The shelf named after the folder, if it had one.
    #[serde(default)]
    shelf: Option<ShelfId>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Shelving {
    #[default]
    None,
    One,
    Each,
}

/// Turns old rows into folders; the next save writes and retires them.
pub(crate) fn adopt(library: &mut Library, sources: Vec<OldFolder>) {
    for source in sources {
        library.retired.insert(source.id);
        if !library.folders.contains_key(&source.id) {
            let folder = convert(library, &source);
            library.folders.insert(folder.id, folder);
        }
    }
}

fn convert(library: &mut Library, source: &OldFolder) -> Folder {
    let formats: BTreeSet<Format> = match source.exclude {
        true => (Format::ALL.into_iter())
            .filter(|f| !source.formats.contains(f))
            .collect(),
        false => source.formats.clone(),
    };
    let opts = Options {
        formats: match formats.is_empty() {
            true => Format::ALL.into_iter().collect(),
            false => formats,
        },
        include: true,
        min_size: source.min_size.min(MIN_SIZE_MAX),
        in_place: true,
        watch: source.watch,
        groups: source.shelving == Shelving::Each,
    };
    let mut folder = Folder::new(source.id, source.root.clone(), opts, source.added);
    let here = |b: &&Book| (b.locations.iter()).any(|l| l.origin == Origin::Folder(source.id));
    for book in library.books.values().filter(here) {
        folder.placed.insert(book.content);
        let path = book.home().map(|l| l.path.clone()).unwrap_or_default();
        match book.alive() {
            true => folder.last_seen.push((book.content, path)),
            false if source.hidden.contains(&book.id) => {
                folder.ignored.push(stone(book, path));
            }
            false => {}
        }
    }
    if let Some(top) = source.shelf.filter(|s| library.shelf(*s).is_some()) {
        let mut rungs = vec![(top, String::new())];
        while let Some((shelf, key)) = rungs.pop() {
            if source.shelving == Shelving::Each {
                for child in library.children(Some(shelf)) {
                    let name = &child.name.value;
                    let below = match key.is_empty() {
                        true => name.clone(),
                        false => format!("{key}/{name}"),
                    };
                    rungs.push((child.id, below));
                }
            }
            folder.shelf_map.insert(key, shelf);
        }
        let at = library.clock.tick(source.added);
        for (key, shelf) in &folder.shelf_map {
            let kind = ShelfKind::Rung {
                folder: source.id,
                key: key.clone(),
            };
            if let Some(shelf) = library.shelves.get_mut(shelf) {
                shelf.kind = Field::new(kind, at);
            }
        }
    }
    folder
}

/// A hidden book as the folder's tombstone, restorable from its menu.
fn stone(book: &Book, last_path: PathBuf) -> Tombstone {
    Tombstone {
        content: book.content,
        book: book.id,
        title: book.title.value.clone(),
        format: book.format,
        last_path,
        shelf: None,
        removed_ms: book.removed.map_or(0, |at| at.ms),
        moved: false,
        returned: None,
        size: book.home().map_or(0, |l| l.stamp.size),
    }
}
