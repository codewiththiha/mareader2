//! Workers fed by demand restated each frame; stale work is dropped.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use mareader_pdf::{Document, OpenError, Raster, RenderError};

use crate::key::PageKey;

pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

pub(crate) enum Job {
    Open { ticket: u64, bytes: Arc<Vec<u8>> },
    Render { key: PageKey, doc: Arc<Document> },
}

pub(crate) enum Output {
    Opened {
        ticket: u64,
        result: Result<Document, OpenError>,
    },
    Rendered {
        key: PageKey,
        result: Result<Raster, RenderError>,
    },
}

impl Output {
    pub(crate) fn bytes(&self) -> usize {
        match self {
            Self::Rendered { result: Ok(r), .. } => r.bytes(),
            _ => 0,
        }
    }
}

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    running: HashSet<PageKey>,
    wanted: HashSet<PageKey>,
    done: VecDeque<Output>,
    quit: bool,
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

    pub(crate) fn open(&self, ticket: u64, bytes: Arc<Vec<u8>>) {
        let mut s = self.shared.lock();
        // Opening outranks drawing: the reader is waiting on it.
        s.queue.push_front(Job::Open { ticket, bytes });
        drop(s);
        self.shared.work.notify_one();
    }

    /// Replaces the render demand; `jobs` come most urgent first.
    pub(crate) fn want(&self, jobs: Vec<(PageKey, Arc<Document>)>) {
        let mut s = self.shared.lock();
        let s = &mut *s;
        s.wanted = jobs.iter().map(|(k, _)| *k).collect();
        s.queue.retain(|j| matches!(j, Job::Open { .. }));
        s.done.retain(|o| match o {
            Output::Rendered { key, .. } => s.wanted.contains(key),
            Output::Opened { .. } => true,
        });
        let ready: HashSet<PageKey> = s
            .done
            .iter()
            .filter_map(|o| match o {
                Output::Rendered { key, .. } => Some(*key),
                Output::Opened { .. } => None,
            })
            .collect();
        let fresh = jobs
            .into_iter()
            .filter(|(k, _)| !s.running.contains(k) && !ready.contains(k));
        s.queue
            .extend(fresh.map(|(key, doc)| Job::Render { key, doc }));
        let more = !s.queue.is_empty();
        if more {
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
            out.extend(s.done.pop_front());
        }
        if !s.done.is_empty() {
            (self.shared.wake)();
        }
        out
    }

    /// Without threads: one job per poll keeps frames short.
    fn run_inline(&self) {
        let job = self.shared.lock().queue.pop_front();
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
                if let Some(job) = s.queue.pop_front() {
                    if let Job::Render { key, .. } = &job {
                        s.running.insert(*key);
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
        Job::Open { ticket, bytes } => Output::Opened {
            ticket,
            result: Document::open(bytes),
        },
        Job::Render { key, doc } => Output::Rendered {
            key,
            result: doc.render(key.page, key.scale.value()),
        },
    };
    let mut s = shared.lock();
    let keep = match &out {
        Output::Rendered { key, .. } => {
            s.running.remove(key);
            s.wanted.contains(key)
        }
        Output::Opened { .. } => true,
    };
    if keep {
        s.done.push_back(out);
        drop(s);
        (shared.wake)();
    }
}
