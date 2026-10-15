//! A book, where its file is, and where its reader is in it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::clock::{Field, Stamp};
use crate::id::{BookId, ContentId};

/// The file's own format; text and Markdown open converted to EPUB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Pdf,
    Epub,
    Text,
    Markdown,
}

impl Format {
    /// Text is hashed whole: a same-length edit can miss both ends.
    pub fn hashed_whole(self) -> bool {
        matches!(self, Self::Text | Self::Markdown)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::Epub => "EPUB",
            Self::Text => "TXT",
            Self::Markdown => "MD",
        }
    }
}

/// Where a reader was: the page shown, and a book's place in its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub page: usize,
    /// The page count then, for "page 12 of 300" and the progress.
    pub pages: usize,
    /// A book's block and byte offset, which outlast a change of type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<(usize, usize)>,
}

impl Position {
    /// How far through, 0 to 1; none before the count is known.
    pub fn fraction(&self) -> Option<f32> {
        let read = self.page.saturating_add(1) as f32;
        (self.pages > 0).then(|| (read / self.pages as f32).min(1.0))
    }
}

/// What changes as a book is read; kept in its own small file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub position: Field<Position>,
}

/// Size and modification time, to skip re-hashing an unchanged file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStamp {
    pub size: u64,
    pub mtime_ms: u64,
}

/// How the library came to know a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Opened in the reader and read in place; never moved or deleted.
    Opened,
}

/// One place a book's file is; local to this machine, never synced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub path: PathBuf,
    pub origin: Origin,
    pub stamp: FileStamp,
    /// Whether the file was there when last looked for; not saved.
    #[serde(skip, default = "present")]
    pub present: bool,
}

fn present() -> bool {
    true
}

/// One book: a content, its names, and where its file may be found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub id: BookId,
    pub content: ContentId,
    pub format: Format,
    pub title: Field<String>,
    pub author: Field<Option<String>>,
    /// When it joined the library, in wall milliseconds.
    pub added: u64,
    /// When it was last opened, in wall milliseconds.
    pub opened: Field<u64>,
    /// Newest first; the first present one is opened.
    pub locations: Vec<Location>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<Stamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored: Option<Stamp>,
}

impl Book {
    /// Not removed, or restored since; removal is a tombstone.
    pub fn alive(&self) -> bool {
        match (self.removed, self.restored) {
            (None, _) => true,
            (Some(removed), Some(restored)) => restored > removed,
            (Some(_), None) => false,
        }
    }

    /// The file to open: the newest location still there.
    pub fn path(&self) -> Option<&Path> {
        let here = self.locations.iter().find(|l| l.present);
        here.map(|l| l.path.as_path())
    }

    /// No location of it is there any more.
    pub fn missing(&self) -> bool {
        self.path().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_counts_the_page_shown_as_read() {
        let at = |page, pages| {
            Position {
                page,
                pages,
                place: None,
            }
            .fraction()
        };
        assert_eq!(at(0, 0), None);
        assert_eq!(at(0, 4), Some(0.25));
        assert_eq!(at(3, 4), Some(1.0));
        // A count that shrank since never reads past the end.
        assert_eq!(at(9, 4), Some(1.0));
    }
}
