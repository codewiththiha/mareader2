//! Whose ground a shelf or book stands on, and which moves leave it.

use std::collections::BTreeMap;
use std::path::Path;

use crate::conflict::Level;
use crate::folder::{Folder, Mode, parent_key, rel_under};
use crate::id::{ContentId, FolderId, ShelfId};
use crate::shelves::{Shelf, ShelfKind};

/// The shelves by id, removed ones included; lookups skip those.
pub type Shelves = BTreeMap<ShelfId, Shelf>;

/// A chain of parents longer than this is a loop a merge left.
const DEPTH_MAX: usize = 64;

fn alive(shelves: &Shelves, id: ShelfId) -> Option<&Shelf> {
    shelves.get(&id).filter(|s| s.alive())
}

/// The shelves above `id`, nearest first; a loop ends the walk.
pub fn ancestors(shelves: &Shelves, id: ShelfId) -> Vec<ShelfId> {
    let mut out = Vec::new();
    let mut at = alive(shelves, id).and_then(|s| s.parent.value);
    while let Some(up) = at {
        if out.contains(&up) || up == id || out.len() >= DEPTH_MAX {
            break;
        }
        out.push(up);
        at = alive(shelves, up).and_then(|s| s.parent.value);
    }
    out
}

/// The shelves filed directly on a level.
pub fn children(shelves: &Shelves, level: Level) -> Vec<ShelfId> {
    shelves
        .values()
        .filter(|s| s.alive() && s.parent.value == level)
        .map(|s| s.id)
        .collect()
}

/// Everything below the roots, deepest last, never the roots themselves.
pub fn subtree(shelves: &Shelves, roots: &[ShelfId]) -> Vec<ShelfId> {
    let mut out: Vec<ShelfId> = Vec::new();
    let mut next: Vec<ShelfId> = roots.to_vec();
    while let Some(id) = next.pop() {
        for child in children(shelves, Some(id)) {
            if !roots.contains(&child) && !out.contains(&child) {
                out.push(child);
                next.push(child);
            }
        }
    }
    out
}

/// Whether a shelf may be filed on `into`: never itself or below itself.
pub fn can_nest(shelves: &Shelves, id: ShelfId, into: Level) -> bool {
    match into {
        None => true,
        Some(target) => target != id && !ancestors(shelves, target).contains(&id),
    }
}

/// The folder ground a shelf stands on: the rung a watch toggle writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    pub folder: FolderId,
    pub rung: String,
}

/// The tree a shelf stands in: its own rung, or a reader's shelf's nearest.
pub fn tree_of(shelves: &Shelves, folders: &[Folder], id: ShelfId) -> Option<Seat> {
    let shelf = alive(shelves, id)?;
    let seat = match &shelf.kind.value {
        ShelfKind::Rung { folder, key } => Seat {
            folder: *folder,
            rung: key.clone(),
        },
        // A departed shelf stands where the reader put it.
        ShelfKind::Departed => return None,
        ShelfKind::Own => ancestors(shelves, id)
            .into_iter()
            .filter_map(|up| alive(shelves, up))
            .take_while(|up| up.kind.value != ShelfKind::Departed)
            .find_map(|up| {
                up.kind.value.rung().map(|(folder, key)| Seat {
                    folder,
                    rung: key.to_string(),
                })
            })?,
    };
    folders.iter().any(|f| f.id == seat.folder).then_some(seat)
}

/// [`tree_of`], for trees read in place only: where watching is asked.
pub fn seat_of(shelves: &Shelves, folders: &[Folder], id: ShelfId) -> Option<Seat> {
    let seat = tree_of(shelves, folders, id)?;
    let folder = folders.iter().find(|f| f.id == seat.folder)?;
    folder.mode().in_place().then_some(seat)
}

/// How a shelf's books are held; `None` for a shelf no folder owns.
pub fn mode_of(shelves: &Shelves, folders: &[Folder], id: ShelfId) -> Option<Mode> {
    if alive(shelves, id)?.kind.value == ShelfKind::Departed {
        return Some(Mode::Copy);
    }
    let seat = tree_of(shelves, folders, id)?;
    folders
        .iter()
        .find(|f| f.id == seat.folder)
        .map(Folder::mode)
}

/// Whether the tree watches the rung this shelf stands on.
pub fn shelf_tracked(shelves: &Shelves, folders: &[Folder], id: ShelfId) -> bool {
    seat_of(shelves, folders, id).is_some_and(|seat| {
        folders
            .iter()
            .find(|f| f.id == seat.folder)
            .is_some_and(|f| f.tracks_rung(&seat.rung))
    })
}

/// Whether a book read in place, moving to `to`, must become a copy.
pub fn converts_on_move(folders: &[Folder], content: ContentId, path: &Path, to: Level) -> bool {
    let placing: Vec<&Folder> = folders
        .iter()
        .filter(|f| f.mode().in_place() && f.placed.contains(&content))
        .collect();
    if placing.is_empty() {
        return false;
    }
    match to {
        None => true,
        Some(to) => !placing.iter().any(|f| f.rungs_for(path).0 == Some(to)),
    }
}

/// Whether moving a rung onto `to` takes it off its tree's seat.
pub fn departs_on_move(shelves: &Shelves, folders: &[Folder], id: ShelfId, to: Level) -> bool {
    let Some(shelf) = alive(shelves, id) else {
        return false;
    };
    let Some((folder, key)) = shelf.kind.value.rung() else {
        return false;
    };
    let Some(folder) = folders
        .iter()
        .find(|f| f.id == folder && f.mode().in_place())
    else {
        return false;
    };
    // A reorder on the current seat is the folder's own business.
    if shelf.parent.value == to {
        return false;
    }
    let seat = parent_key(key).and_then(|rung| folder.shelf_map.get(rung).copied());
    seat != to
}

/// Split shelf moves into those that land and those that ask first.
pub fn departing_moves(
    shelves: &Shelves,
    folders: &[Folder],
    ids: &[ShelfId],
    to: Level,
) -> (Vec<ShelfId>, Vec<ShelfId>) {
    let (departing, clean): (Vec<ShelfId>, Vec<ShelfId>) = ids
        .iter()
        .partition(|id| departs_on_move(shelves, folders, **id, to));
    // A departing shelf inside another departing one rides with it.
    let riders: Vec<ShelfId> = departing
        .iter()
        .filter(|id| {
            ancestors(shelves, **id)
                .iter()
                .any(|up| departing.contains(up))
        })
        .copied()
        .collect();
    let departing = departing
        .into_iter()
        .filter(|id| !riders.contains(id))
        .collect();
    (clean, departing)
}

/// The folder, rung and standing shelf that answer for a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub folder: FolderId,
    pub rel: String,
    pub shelf: ShelfId,
}

fn standing(shelves: &Shelves, folder: &Folder, rel: &str) -> Option<ShelfId> {
    folder
        .shelf_map
        .get(rel)
        .copied()
        .filter(|id| alive(shelves, *id).is_some())
}

/// The standing shelf a tree read in place holds for `dir`; a root wins.
pub fn covering(shelves: &Shelves, folders: &[Folder], dir: &Path) -> Option<Coverage> {
    let mut rung = None;
    for folder in folders.iter().filter(|f| f.mode().in_place()) {
        let Some(rel) = rel_under(dir, &folder.root) else {
            continue;
        };
        let Some(shelf) = standing(shelves, folder, &rel) else {
            continue;
        };
        let coverage = Coverage {
            folder: folder.id,
            rel,
            shelf,
        };
        if coverage.rel.is_empty() {
            return Some(coverage);
        }
        rung.get_or_insert(coverage);
    }
    rung
}

/// The deepest standing tree `dir` belongs to without a rung of its own.
pub fn family(shelves: &Shelves, folders: &[Folder], dir: &Path) -> Option<(FolderId, String)> {
    folders
        .iter()
        .filter(|f| f.mode().in_place() && standing(shelves, f, "").is_some())
        .filter_map(|f| {
            rel_under(dir, &f.root)
                .filter(|rel| !rel.is_empty())
                .map(|rel| (f, rel))
        })
        .max_by_key(|(_, rel)| rel.len())
        .and_then(|(f, rel)| standing(shelves, f, &rel).is_none().then_some((f.id, rel)))
}
