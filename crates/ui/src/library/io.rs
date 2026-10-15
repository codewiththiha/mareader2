//! The library's disk work on one thread, or inline for tests and shots.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use egui::Context;
use mareader_library::disk::{self, Folder, Loaded};
use mareader_library::{DeviceId, Format, Measure};

pub enum Job {
    Load(Folder, DeviceId),
    Measure(Probe),
    Write(Vec<(PathBuf, String)>),
}

/// A file a pane opened, waiting to be measured.
#[derive(Clone, Debug)]
pub struct Probe {
    pub path: PathBuf,
    pub format: Format,
    pub title: String,
}

pub enum News {
    Loaded(Box<Loaded>),
    Measured(Probe, Measure),
    WriteFailed(String),
}

pub enum Io {
    Inline(VecDeque<News>),
    Thread {
        jobs: Option<Sender<Job>>,
        news: Receiver<News>,
        /// To wake the frame when news arrives.
        ctx: Arc<Mutex<Option<Context>>>,
        worker: Option<JoinHandle<()>>,
    },
}

impl Default for Io {
    fn default() -> Self {
        Self::Inline(VecDeque::new())
    }
}

impl Io {
    /// A worker thread; inline where one can't start.
    pub fn thread() -> Self {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let (post, news) = mpsc::channel();
        let ctx: Arc<Mutex<Option<Context>>> = Arc::default();
        let wake = Arc::clone(&ctx);
        let spawned = std::thread::Builder::new()
            .name("library".into())
            .spawn(move || {
                for job in inbox {
                    let Some(n) = run(job) else { continue };
                    if post.send(n).is_err() {
                        return;
                    }
                    let ctx = wake.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Some(ctx) = ctx.as_ref() {
                        ctx.request_repaint();
                    }
                }
            });
        match spawned {
            Ok(worker) => Self::Thread {
                jobs: Some(jobs),
                news,
                ctx,
                worker: Some(worker),
            },
            Err(_) => Self::default(),
        }
    }

    pub fn send(&mut self, job: Job) {
        match self {
            Self::Inline(news) => news.extend(run(job)),
            Self::Thread { jobs, .. } => {
                // A dead worker means no disk: the library stays in memory.
                let _ = jobs.as_ref().map(|j| j.send(job));
            }
        }
    }

    /// What finished since the last call.
    pub fn poll(&mut self, ctx: &Context) -> Vec<News> {
        match self {
            Self::Inline(news) => news.drain(..).collect(),
            Self::Thread {
                news, ctx: wake, ..
            } => {
                let mut slot = wake.lock().unwrap_or_else(PoisonError::into_inner);
                slot.get_or_insert_with(|| ctx.clone());
                news.try_iter().collect()
            }
        }
    }
}

impl Drop for Io {
    /// Finishes the queued writes before the app exits.
    fn drop(&mut self) {
        if let Self::Thread { jobs, worker, .. } = self {
            jobs.take();
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

fn run(job: Job) -> Option<News> {
    match job {
        Job::Load(folder, device) => Some(News::Loaded(Box::new(folder.load(device)))),
        // A file gone before it was measured is simply not recorded.
        Job::Measure(probe) => disk::measure(&probe.path, probe.format)
            .ok()
            .map(|m| News::Measured(probe, m)),
        Job::Write(files) => {
            // Each file is tried; one failure doesn't hold back the rest.
            let mut failed = None;
            for (path, text) in &files {
                if let Err(e) = disk::write_atomic(path, text.as_bytes()) {
                    failed.get_or_insert(e);
                }
            }
            failed.map(|e| News::WriteFailed(format!("Couldn't save the library: {e}")))
        }
    }
}
