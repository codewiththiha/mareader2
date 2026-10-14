//! Open documents shared by every view, and their worker pool (L3).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod document;
mod key;
mod pool;
mod typeset;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use document::Document;
pub use key::{DocId, PageKey, Part, Scale, Want};
pub use mareader_pdf::{PageSize, Paper, Raster, RenderError, TextLayer, Tiling};
pub use mareader_reflow::{Anchor, Band, FACES, Face, Kind, Typography, has_family};
pub use typeset::{bundled, typography};

use document::TextFile;
use pool::{Output, Pool};

/// Where a document's bytes come from; workers read paths.
#[derive(Clone)]
pub enum Source {
    Bytes(Arc<Vec<u8>>),
    Path(PathBuf),
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bytes(b) => write!(f, "Bytes({} B)", b.len()),
            Self::Path(p) => write!(f, "Path({})", p.display()),
        }
    }
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bytes(a), Self::Bytes(b)) => Arc::ptr_eq(a, b),
            (Self::Path(a), Self::Path(b)) => a == b,
            _ => false,
        }
    }
}

impl Source {
    fn read(self) -> Result<Arc<Vec<u8>>, OpenError> {
        match self {
            Self::Bytes(b) => Ok(b),
            Self::Path(p) => std::fs::read(p)
                .map(Arc::new)
                .map_err(|e| OpenError::Unreadable(e.kind().to_string())),
        }
    }

    /// The path's text, for naming the document.
    fn file_name(&self) -> Option<String> {
        match self {
            Self::Bytes(_) => None,
            Self::Path(p) => Some(p.to_string_lossy().into_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    #[error("the file couldn't be read ({0})")]
    Unreadable(String),
    #[error(transparent)]
    Pdf(#[from] mareader_pdf::OpenError),
    #[error(transparent)]
    Epub(#[from] mareader_epub::OpenError),
}

/// Names one `open` call, to match its `Event::Opened`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ticket(u64);

#[derive(Debug)]
pub enum Event {
    Opened {
        ticket: Ticket,
        result: Result<DocId, OpenError>,
    },
    Rendered {
        key: PageKey,
        result: Result<Raster, RenderError>,
    },
    /// A page's text and links, shared: views and copies read it.
    Text {
        doc: DocId,
        page: usize,
        result: Result<Arc<TextLayer>, RenderError>,
    },
    /// A page's paper colour, read from a small drawing of it.
    Paper {
        doc: DocId,
        page: usize,
        result: Result<Paper, RenderError>,
    },
    /// `old`, set again in new type, is now `new`; `previous` was `old`.
    Retypeset {
        old: DocId,
        new: DocId,
        previous: Arc<Document>,
    },
}

/// One open document and what to call it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub doc: Arc<Document>,
    pub name: String,
    /// The file it came from; none for bytes, such as a browser's drop.
    pub path: Option<PathBuf>,
}

/// How much finished work one `poll` hands over.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub items: usize,
    pub bytes: usize,
}

pub struct Session {
    pool: Pool,
    docs: BTreeMap<DocId, Entry>,
    /// Each open in flight: its fallback name, file and type.
    opening: BTreeMap<u64, Opening>,
    /// What books are set in now.
    typography: Arc<Typography>,
    /// The newest re-set asked of each document; older ones are dropped.
    typesetting: BTreeMap<DocId, u64>,
    next: u64,
}

struct Opening {
    name: Option<String>,
    path: Option<PathBuf>,
    typo: Arc<Typography>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Session({} documents)", self.docs.len())
    }
}

impl Session {
    /// `wake` runs on a worker when results are ready: request a frame.
    pub fn new(workers: usize, wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            pool: Pool::new(workers, Arc::new(wake)),
            docs: BTreeMap::new(),
            opening: BTreeMap::new(),
            typography: Arc::default(),
            typesetting: BTreeMap::new(),
            next: 1,
        }
    }

    /// Workers for this machine: two at most, leaving the UI a core.
    pub fn default_workers() -> usize {
        std::thread::available_parallelism().map_or(1, |n| n.get().clamp(2, 3) - 1)
    }

    /// Starts opening; `file_name` names it if its title won't.
    pub fn open(&mut self, source: Source, file_name: Option<String>) -> Ticket {
        let ticket = self.bump();
        let path = match &source {
            Source::Path(p) => Some(p.clone()),
            Source::Bytes(_) => None,
        };
        let name = file_name.or_else(|| source.file_name());
        // Markdown and plain text are told by name; the rest by content.
        let text = name.as_deref().and_then(|n| {
            Some(TextFile {
                format: mareader_epub::Format::of_name(n)?,
                name: n.to_owned(),
                // A dropped file arrives as bytes named by its full path.
                folder: path
                    .clone()
                    .or_else(|| Some(PathBuf::from(n)).filter(|p| p.is_absolute()))
                    .and_then(|p| p.parent().map(Path::to_path_buf)),
            })
        });
        let typo = Arc::clone(&self.typography);
        let opening = Opening {
            name,
            path,
            typo: Arc::clone(&typo),
        };
        self.opening.insert(ticket, opening);
        self.pool.open(ticket, source, typo, text);
        Ticket(ticket)
    }

    pub fn close(&mut self, id: DocId) {
        self.docs.remove(&id);
        self.typesetting.remove(&id);
    }

    /// Re-sets books when text or Column Width change; each sends `Retypeset`.
    pub fn set_text(&mut self, text: &mareader_settings::TextSettings, column_pct: f32) {
        let typo = typography(text, column_pct);
        if *self.typography == typo {
            return;
        }
        self.typography = Arc::new(typo);
        let books: Vec<DocId> = self
            .docs
            .iter()
            .filter(|(_, e)| e.doc.is_reflowable())
            .map(|(id, _)| *id)
            .collect();
        for id in books {
            self.retypeset(id);
        }
    }

    fn retypeset(&mut self, id: DocId) {
        let ticket = self.bump();
        let Some(entry) = self.docs.get(&id) else {
            return;
        };
        let doc = Arc::clone(&entry.doc);
        self.typesetting.insert(id, ticket);
        self.pool
            .retypeset(ticket, id, doc, Arc::clone(&self.typography));
    }

    pub fn get(&self, id: DocId) -> Option<&Entry> {
        self.docs.get(&id)
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Replaces the demand, most urgent first.
    ///
    /// Work for closed documents is ignored.
    pub fn want(&mut self, wants: impl IntoIterator<Item = impl Into<Want>>) {
        let jobs = wants
            .into_iter()
            .map(Into::into)
            .filter_map(|w: Want| self.docs.get(&w.doc()).map(|e| (w, Arc::clone(&e.doc))))
            .collect();
        self.pool.want(jobs);
    }

    /// Finished work, within `budget` (always at least one item).
    pub fn poll(&mut self, budget: Budget) -> Vec<Event> {
        let done = self.pool.poll(budget.items, budget.bytes);
        done.into_iter().filter_map(|o| self.accept(o)).collect()
    }

    /// Jobs queued or running.
    pub fn pending(&self) -> usize {
        self.pool.pending()
    }

    fn accept(&mut self, out: Output) -> Option<Event> {
        match out {
            Output::Opened { ticket, result } => {
                let Opening { name, path, typo } = self.opening.remove(&ticket)?;
                let result = result.map(|doc| {
                    let name = mareader_pdf::display_name(doc.raw_title(), name.as_deref())
                        .unwrap_or_else(|| "Untitled".to_string());
                    let id = DocId(self.bump());
                    // The type changed while it opened: set it again.
                    let stale = doc.is_reflowable() && !Arc::ptr_eq(&typo, &self.typography);
                    let doc = Arc::new(doc);
                    self.docs.insert(id, Entry { doc, name, path });
                    if stale {
                        self.retypeset(id);
                    }
                    id
                });
                Some(Event::Opened {
                    ticket: Ticket(ticket),
                    result,
                })
            }
            Output::Retypeset { ticket, id, result } => {
                if self.typesetting.get(&id) != Some(&ticket) {
                    return None;
                }
                self.typesetting.remove(&id);
                // A book that won't set again stays as it was.
                let doc = result?;
                let old = self.docs.remove(&id)?;
                let new = DocId(self.bump());
                let entry = Entry {
                    doc: Arc::new(doc),
                    name: old.name,
                    path: old.path,
                };
                self.docs.insert(new, entry);
                Some(Event::Retypeset {
                    old: id,
                    new,
                    previous: old.doc,
                })
            }
            // A page of a document closed meanwhile is dropped.
            Output::Rendered { key, result } => self
                .docs
                .contains_key(&key.doc)
                .then_some(Event::Rendered { key, result }),
            Output::Read { doc, page, result } => {
                self.docs.contains_key(&doc).then(|| Event::Text {
                    doc,
                    page,
                    result: result.map(Arc::new),
                })
            }
            Output::Paper { doc, page, result } => self
                .docs
                .contains_key(&doc)
                .then_some(Event::Paper { doc, page, result }),
        }
    }

    fn bump(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

#[cfg(test)]
mod tests;
