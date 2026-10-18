//! Books and their files: relinking, where a path came from, what's lost.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use crate::book::{Book, Format, Origin};
use crate::id::{BookId, FolderId};
use crate::library::{Library, Measure};

/// Why a file can't stand in for a book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelinkError {
    Removed,
    /// The file is already this other book.
    OtherBook(String),
    /// Different content in another format.
    Format {
        file: Format,
        book: Format,
    },
    /// Different content: another edition, or an edit. Asked first.
    Differs,
}

impl Library {
    /// Every record, removed ones included.
    pub fn all_books(&self) -> impl Iterator<Item = &Book> {
        self.books.values()
    }

    /// Books with no file left to open.
    pub fn missing(&self) -> usize {
        self.books().filter(|b| b.missing()).count()
    }

    /// Read in place under a folder's root, else opened by hand.
    pub(crate) fn origin_of(&self, path: &Path) -> Origin {
        let holds = |f: &&crate::folder::Folder| f.mode().in_place() && path.starts_with(&f.root);
        let deepest = self.folders.values().filter(holds);
        let deepest = deepest.max_by_key(|f| f.root.components().count());
        deepest.map_or(Origin::Opened, |f| Origin::Folder(f.id))
    }

    /// A fresh folder id: hashed from where and when, never reused.
    pub(crate) fn new_folder_id(&self, root: &Path, wall_ms: u64) -> FolderId {
        let mut hasher = DefaultHasher::new();
        (self.device(), root, wall_ms).hash(&mut hasher);
        let mut id = FolderId(hasher.finish());
        while self.folders.contains_key(&id) {
            id = FolderId(id.0.wrapping_add(1));
        }
        id
    }

    /// Points a book at `path` after measuring it; `force` takes an edit.
    pub fn relink(
        &mut self,
        id: BookId,
        path: &Path,
        format: Format,
        measure: Measure,
        force: bool,
    ) -> Result<(), RelinkError> {
        // A twin's files are its original's.
        let id = self.book(id).ok_or(RelinkError::Removed)?.original();
        let book = self.book(id).ok_or(RelinkError::Removed)?;
        let same = book.content == measure.content;
        let holds =
            |b: &Book| b.content == measure.content || b.locations.iter().any(|l| l.path == path);
        let other = |b: &&Book| b.original() != id && holds(b);
        if let Some(other) = self.books().find(other) {
            return Err(RelinkError::OtherBook(other.title.value.clone()));
        }
        if !same && format != book.format {
            let book = book.format;
            return Err(RelinkError::Format { file: format, book });
        }
        if !same && !force {
            return Err(RelinkError::Differs);
        }
        let origin = self.origin_of(path);
        if let Some(book) = self.books.get_mut(&id) {
            book.content = measure.content;
            book.format = format;
            book.locations.retain(|l| l.present);
        }
        self.place(id, path, measure.stamp, origin);
        self.mirror(id);
        self.bump();
        Ok(())
    }
}
