//! Workers fed by demand restated each frame; stale work is dropped.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use mareader_epub::{Inserts, OpenBook, OpenStep, OpenTarget};
use mareader_pdf::{Paper, Raster, RenderError, TextLayer};
use mareader_reflow::Typography;

use crate::document::TextFile;
use crate::key::{DocId, PageKey, Part, Want};
use crate::{Document, OpenError, Source};

/// Finished results wait here; past it, workers pause until a poll.
const DONE_ITEMS: usize = 32;
const DONE_BYTES: usize = 32 << 20;

pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

pub(crate) enum Job {
    Open {
        ticket: u64,
        source: Source,
        typo: Arc<Typography>,
        /// Set when the file is text to convert first.
        text: Option<TextFile>,
        target: Option<OpenTarget>,
    },
    ContinueOpen {
        ticket: u64,
        state: Box<OpenBook>,
    },
    /// `doc`, open as `id`, set again in `typo`, maybe with `inserts`.
    Retypeset {
        ticket: u64,
        id: DocId,
        doc: Arc<Document>,
        typo: Arc<Typography>,
        inserts: Option<Arc<Inserts>>,
    },
    Work {
        want: Want,
        doc: Arc<Document>,
    },
}

impl Job {
    /// Opens and re-sets jump the queue and its backlog limit.
    fn urgent(&self) -> bool {
        matches!(self, Self::Open { .. } | Self::Retypeset { .. })
    }
}

pub(crate) enum Output {
    Opened {
        ticket: u64,
        result: Result<Document, OpenError>,
    },
    OpenPreview {
        ticket: u64,
        doc: Document,
        state: Box<OpenBook>,
    },
    OpenProgress {
        ticket: u64,
        state: Box<OpenBook>,
    },
    OpenComplete {
        ticket: u64,
        doc: Document,
    },
    OpenFailed {
        ticket: u64,
        error: OpenError,
    },
    /// None when the book wouldn't set; the old one stays.
    Retypeset {
        ticket: u64,
        id: DocId,
        result: Option<Document>,
    },
    Rendered {
        key: PageKey,
        result: Result<Raster, RenderError>,
    },
    Read {
        doc: DocId,
        page: usize,
        result: Result<TextLayer, RenderError>,
    },
    Paper {
        doc: DocId,
        page: usize,
        result: Result<Paper, RenderError>,
    },
}

impl Output {
    pub(crate) fn bytes(&self) -> usize {
        match self {
            Self::Rendered { result: Ok(r), .. } => r.bytes(),
            Self::Read { result: Ok(t), .. } => t.bytes(),
            _ => 0,
        }
    }

    /// The demand this answers; opens answer none.
    fn want(&self) -> Option<Want> {
        match self {
            Self::Rendered { key, .. } => Some(Want::Raster(*key)),
            Self::Read { doc, page, .. } => Some(Want::Text {
                doc: *doc,
                page: *page,
            }),
            Self::Paper { doc, page, .. } => Some(Want::Paper {
                doc: *doc,
                page: *page,
            }),
            Self::Opened { .. }
            | Self::OpenPreview { .. }
            | Self::OpenProgress { .. }
            | Self::OpenComplete { .. }
            | Self::OpenFailed { .. }
            | Self::Retypeset { .. } => None,
        }
    }
}

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    running: HashSet<Want>,
    wanted: HashSet<Want>,
    done: VecDeque<Output>,
    done_bytes: usize,
    quit: bool,
}

impl State {
    fn full(&self) -> bool {
        self.done.len() + self.running.len() >= DONE_ITEMS || self.done_bytes >= DONE_BYTES
    }

    /// The next job a worker may take: opens always, the rest if room.
    fn next(&mut self) -> Option<Job> {
        let urgent = self.queue.front().is_some_and(Job::urgent);
        if urgent || !self.full() {
            self.queue.pop_front()
        } else {
            None
        }
    }
}

struct Shared {
    state: Mutex<State>,
    work: Condvar,
    wake: Wake,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // Documents catch their own panics, so poison is only noise.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Memory tracks the viewport: unwanted jobs and results are dropped.
pub(crate) struct Pool {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

impl Pool {
    /// Zero workers runs jobs inline from `poll`, for threadless targets.
    pub(crate) fn new(workers: usize, wake: Wake) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            work: Condvar::new(),
            wake,
        });
        let workers = (0..workers)
            .filter_map(|i| {
                let shared = Arc::clone(&shared);
                std::thread::Builder::new()
                    .name(format!("mareader-render-{i}"))
                    .spawn(move || work(&shared))
                    .ok()
            })
            .collect();
        Self { shared, workers }
    }

    pub(crate) fn open(
        &self,
        ticket: u64,
        source: Source,
        typo: Arc<Typography>,
        text: Option<TextFile>,
        target: Option<OpenTarget>,
    ) {
        self.urgent(Job::Open {
            ticket,
            source,
            typo,
            text,
            target,
        });
    }

    pub(crate) fn continue_open(&self, ticket: u64, state: Box<OpenBook>) {
        self.shared
            .lock()
            .queue
            .push_back(Job::ContinueOpen { ticket, state });
        if self.workers.is_empty() {
            (self.shared.wake)();
        } else {
            self.shared.work.notify_one();
        }
    }

    pub(crate) fn cancel_open(&self, ticket: u64) {
        self.shared.lock().queue.retain(
            |job| !matches!(job, Job::ContinueOpen { ticket: queued, .. } if *queued == ticket),
        );
    }

    /// Replaces the `stale` re-set if still queued.
    pub(crate) fn retypeset(
        &self,
        ticket: u64,
        (id, doc): (DocId, Arc<Document>),
        typo: Arc<Typography>,
        inserts: Option<Arc<Inserts>>,
        stale: Option<u64>,
    ) {
        let mut s = self.shared.lock();
        let same = |j: &Job| matches!(j, Job::Retypeset { ticket: t, .. } if Some(*t) == stale);
        s.queue.retain(|j| !same(j));
        drop(s);
        self.urgent(Job::Retypeset {
            ticket,
            id,
            doc,
            typo,
            inserts,
        });
    }

    /// Opening and re-setting outrank drawing: the reader is waiting.
    fn urgent(&self, job: Job) {
        self.shared.lock().queue.push_front(job);
        if self.workers.is_empty() {
            (self.shared.wake)();
        } else {
            self.shared.work.notify_one();
        }
    }

    /// Replaces the demand; `jobs` come most urgent first.
    pub(crate) fn want(&self, jobs: Vec<(Want, Arc<Document>)>) {
        let mut s = self.shared.lock();
        let s = &mut *s;
        s.wanted = jobs.iter().map(|(k, _)| *k).collect();
        let mut priority = VecDeque::new();
        let mut background = VecDeque::new();
        for job in s.queue.drain(..) {
            if job.urgent() {
                priority.push_back(job);
            } else if matches!(&job, Job::ContinueOpen { .. }) {
                background.push_back(job);
            }
        }
        s.queue = priority;
        s.done
            .retain(|o| o.want().is_none_or(|w| s.wanted.contains(&w)));
        s.done_bytes = s.done.iter().map(Output::bytes).sum();
        let ready: HashSet<Want> = s.done.iter().filter_map(Output::want).collect();
        let fresh = jobs
            .into_iter()
            .filter(|(k, _)| !s.running.contains(k) && !ready.contains(k));
        s.queue
            .extend(fresh.map(|(want, doc)| Job::Work { want, doc }));
        s.queue.extend(background);
        let more = !s.queue.is_empty();
        if more && self.workers.is_empty() {
            // Inline work runs from the next poll: ask for a frame.
            (self.shared.wake)();
        } else if more {
            self.shared.work.notify_all();
        }
    }

    /// Takes finished work: at least one item, then up to the budget.
    pub(crate) fn poll(&self, max_items: usize, max_bytes: usize) -> Vec<Output> {
        if self.workers.is_empty() {
            self.run_inline();
        }
        let mut s = self.shared.lock();
        let mut out = Vec::new();
        let mut bytes = 0;
        while let Some(next) = s.done.front() {
            let size = next.bytes();
            let full = out.len() >= max_items || bytes + size > max_bytes;
            if !out.is_empty() && full {
                break;
            }
            bytes += size;
            s.done_bytes -= size;
            out.extend(s.done.pop_front());
        }
        if !s.done.is_empty() {
            (self.shared.wake)();
        }
        drop(s);
        // Room again: workers paused on a full queue may continue.
        self.shared.work.notify_all();
        out
    }

    /// Without threads: one job per poll keeps frames short.
    fn run_inline(&self) {
        let job = self.shared.lock().next();
        if let Some(job) = job {
            finish(&self.shared, job);
        }
        if !self.shared.lock().queue.is_empty() {
            (self.shared.wake)();
        }
    }

    /// Jobs not yet finished, queued or running, for tests and meters.
    pub(crate) fn pending(&self) -> usize {
        let s = self.shared.lock();
        s.queue.len() + s.running.len()
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.lock().quit = true;
        self.shared.work.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn work(shared: &Shared) {
    loop {
        let job = {
            let mut s = shared.lock();
            loop {
                if s.quit {
                    return;
                }
                if let Some(job) = s.next() {
                    if let Job::Work { want, .. } = &job {
                        s.running.insert(*want);
                    }
                    break job;
                }
                s = shared.work.wait(s).unwrap_or_else(PoisonError::into_inner);
            }
        };
        finish(shared, job);
    }
}

fn finish(shared: &Shared, job: Job) {
    let out = match job {
        Job::Open {
            ticket,
            source,
            typo,
            text,
            target,
        } => match open_document(source, &typo, text.as_ref(), target) {
            Ok(Opened::Complete(doc)) => Output::Opened {
                ticket,
                result: Ok(doc),
            },
            Ok(Opened::Preview(doc, state)) => Output::OpenPreview { ticket, doc, state },
            Ok(Opened::Progress(state)) => Output::OpenProgress { ticket, state },
            Err(error) => Output::Opened {
                ticket,
                result: Err(error),
            },
        },
        Job::ContinueOpen { ticket, mut state } => {
            let target = state.target().cloned();
            match state.step() {
                Ok(OpenStep::Continue) => Output::OpenProgress { ticket, state },
                Ok(OpenStep::Preview(book)) => Output::OpenPreview {
                    ticket,
                    doc: Document::from_preview(book, target.as_ref()),
                    state,
                },
                Ok(OpenStep::Complete(book)) => Output::OpenComplete {
                    ticket,
                    doc: Document::from_book(book),
                },
                Err(error) => Output::OpenFailed {
                    ticket,
                    error: OpenError::Epub(error),
                },
            }
        },
        Job::Retypeset {
            ticket,
            id,
            doc,
            typo,
            inserts,
        } => Output::Retypeset {
            ticket,
            id,
            result: doc.reset(&typo, inserts.as_ref()),
        },
        Job::Work {
            want: Want::Raster(key),
            doc,
        } => Output::Rendered {
            key,
            result: match key.part {
                Part::Whole => doc.render(key.page, key.scale.value()),
                Part::Tile { col, row } => {
                    let (col, row) = (u32::from(col), u32::from(row));
                    doc.render_tile(key.page, key.scale.value(), col, row)
                }
            },
        },
        Job::Work {
            want: Want::Text { doc: id, page },
            doc,
        } => Output::Read {
            doc: id,
            page,
            result: doc.text_layer(page),
        },
        Job::Work {
            want: Want::Paper { doc: id, page },
            doc,
        } => Output::Paper {
            doc: id,
            page,
            result: doc.paper(page),
        },
    };
    let mut s = shared.lock();
    let keep = match out.want() {
        Some(want) => {
            s.running.remove(&want);
            s.wanted.contains(&want)
        }
        None => true,
    };
    if keep {
        s.done_bytes += out.bytes();
        s.done.push_back(out);
        drop(s);
        (shared.wake)();
    }
}

enum Opened {
    Complete(Document),
    Preview(Document, Box<OpenBook>),
    Progress(Box<OpenBook>),
}

fn open_document(
    source: Source,
    typo: &Typography,
    text: Option<&TextFile>,
    target: Option<OpenTarget>,
) -> Result<Opened, OpenError> {
    let bytes = source.read()?;
    if let Some(file) = text {
        return Document::convert(&bytes, file, typo).map(Opened::Complete);
    }
    if !mareader_epub::sniff(&bytes) {
        return Document::open(bytes, typo).map(Opened::Complete);
    }
    let mut state = OpenBook::new(bytes, typo, target)?;
    if !state.is_progressive() {
        loop {
            match state.step()? {
                OpenStep::Complete(book) => {
                    return Ok(Opened::Complete(Document::from_book(book)));
                }
                OpenStep::Continue | OpenStep::Preview(_) => {}
            }
        }
    }
    let target = state.target().cloned();
    match state.step()? {
        OpenStep::Continue => Ok(Opened::Progress(Box::new(state))),
        OpenStep::Preview(book) => Ok(Opened::Preview(
            Document::from_preview(book, target.as_ref()),
            Box::new(state),
        )),
        OpenStep::Complete(book) => Ok(Opened::Complete(Document::from_book(book))),
    }
}
