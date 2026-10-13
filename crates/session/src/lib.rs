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

mod key;
mod pool;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use key::{DocId, PageKey, Scale};
pub use mareader_pdf::{Document, OpenError, PageSize, Raster, RenderError};

use pool::{Output, Pool};

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
}

/// One open document and what to call it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub doc: Arc<Document>,
    pub name: String,
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
    opening: BTreeMap<u64, Option<String>>,
    next: u64,
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
            next: 1,
        }
    }

    /// Workers for this machine: two at most, leaving the UI a core.
    pub fn default_workers() -> usize {
        std::thread::available_parallelism().map_or(1, |n| n.get().clamp(2, 3) - 1)
    }

    /// Starts opening `bytes`; `file_name` names it if the title won't.
    pub fn open(&mut self, bytes: Arc<Vec<u8>>, file_name: Option<String>) -> Ticket {
        let ticket = self.bump();
        self.opening.insert(ticket, file_name);
        self.pool.open(ticket, bytes);
        Ticket(ticket)
    }

    pub fn close(&mut self, id: DocId) {
        self.docs.remove(&id);
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

    /// Replaces the render demand, most urgent first.
    ///
    /// Keys of closed documents are ignored.
    pub fn want(&mut self, keys: impl IntoIterator<Item = PageKey>) {
        let jobs = keys
            .into_iter()
            .filter_map(|k| self.docs.get(&k.doc).map(|e| (k, Arc::clone(&e.doc))))
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
                let file_name = self.opening.remove(&ticket)?;
                let result = result.map(|doc| {
                    let name = mareader_pdf::display_name(doc.raw_title(), file_name.as_deref())
                        .unwrap_or_else(|| "Untitled".to_string());
                    let id = DocId(self.bump());
                    let doc = Arc::new(doc);
                    self.docs.insert(id, Entry { doc, name });
                    id
                });
                Some(Event::Opened {
                    ticket: Ticket(ticket),
                    result,
                })
            }
            // A page of a document closed meanwhile is dropped.
            Output::Rendered { key, result } => self
                .docs
                .contains_key(&key.doc)
                .then_some(Event::Rendered { key, result }),
        }
    }

    fn bump(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

#[cfg(test)]
mod tests;
