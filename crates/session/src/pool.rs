//! Workers fed by demand restated each frame; stale work is dropped.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use mareader_pdf::{Paper, Raster, RenderError, TextLayer};
use mareader_reflow::Typography;

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
    },
    /// `doc`, open as `id`, set again in `typo`.
    Retypeset {
        ticket: u64,
        id: DocId,
        doc: Arc<Document>,
        typo: Arc<Typography>,
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
            Self::Opened { .. } | Self::Retypeset { .. } => None,
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

    pub(crate) fn open(&self, ticket: u64, source: Source, typo: Arc<Typography>) {
        self.urgent(Job::Open {
            ticket,
            source,
            typo,
        });
    }

    /// Replaces any re-set of `id` still queued.
    pub(crate) fn retypeset(
        &self,
        ticket: u64,
        id: DocId,
        doc: Arc<Document>,
        typo: Arc<Typography>,
    ) {
        let mut s = self.shared.lock();
        let same = |j: &Job| matches!(j, Job::Retypeset { id: queued, .. } if *queued == id);
        s.queue.retain(|j| !same(j));
        drop(s);
        self.urgent(Job::Retypeset {
            ticket,
            id,
            doc,
            typo,
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
        s.queue.retain(Job::urgent);
        s.done
            .retain(|o| o.want().is_none_or(|w| s.wanted.contains(&w)));
        s.done_bytes = s.done.iter().map(Output::bytes).sum();
        let ready: HashSet<Want> = s.done.iter().filter_map(Output::want).collect();
        let fresh = jobs
            .into_iter()
            .filter(|(k, _)| !s.running.contains(k) && !ready.contains(k));
        s.queue
            .extend(fresh.map(|(want, doc)| Job::Work { want, doc }));
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
        } => Output::Opened {
            ticket,
            result: source.read().and_then(|b| Document::open(b, &typo)),
        },
        Job::Retypeset {
            ticket,
            id,
            doc,
            typo,
        } => Output::Retypeset {
            ticket,
            id,
            result: doc.retypeset(&typo),
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
