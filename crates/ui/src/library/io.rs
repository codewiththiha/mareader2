//! The library's disk work on one thread, or inline for tests and shots.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use egui::Context;
use mareader_library::disk::{self, Folder, FoundAgain, Known, Limits, Loaded, Sought};
use mareader_library::{BookId, ContentId, DeviceId, Format, Found, Measure, Scan, Source};

pub enum Job {
    Load(Folder, DeviceId),
    Measure(Probe),
    Write(Vec<(PathBuf, String)>),
    Rescan(Box<Rescan>),
    /// Measures a file picked to stand in for a lost book.
    Relink(Relink),
    /// Looks in a folder for a lost book's file.
    Seek {
        book: BookId,
        folder: PathBuf,
        sought: Sought,
    },
}

/// Known files re-checked, if any, then the folders walked.
pub struct Rescan {
    /// None: a walk of named folders leaves known files alone.
    pub locations: Option<Vec<PathBuf>>,
    pub folders: Vec<Source>,
    /// Content ids by path, to skip re-hashing unchanged files.
    pub known: Known,
    /// Contents the library has met: no need to read their names.
    pub contents: HashSet<ContentId>,
    /// The library's own folders, never walked.
    pub avoid: Vec<PathBuf>,
    pub now_ms: u64,
}

#[derive(Clone, Debug)]
pub struct Relink {
    pub book: BookId,
    pub path: PathBuf,
    pub force: bool,
}

/// A file a pane opened, waiting to be measured.
#[derive(Clone, Debug)]
pub struct Probe {
    pub path: PathBuf,
    pub format: Format,
    pub title: String,
    pub author: Option<String>,
}

pub enum News {
    Loaded(Box<Loaded>),
    Measured(Probe, Measure),
    WriteFailed(String),
    Rescanned(Box<Rescanned>),
    /// The picked file's format and measure; none if unreadable.
    Relinked(Relink, Option<(Format, Measure)>),
    Sought {
        book: BookId,
        folder: PathBuf,
        found: FoundAgain,
    },
}

/// Which known files are gone, if checked, and each folder's walk.
pub struct Rescanned {
    pub gone: Option<HashSet<PathBuf>>,
    pub scans: Vec<Scan>,
    /// Folders with files still being written; look again soon.
    pub settling: Vec<mareader_library::SourceId>,
    /// Folders that hit the walk's limits.
    pub truncated: Vec<mareader_library::SourceId>,
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
        Job::Rescan(job) => Some(News::Rescanned(Box::new(rescan(*job)))),
        Job::Relink(relink) => {
            let format = Format::of_path(&relink.path);
            let measured = format.and_then(|f| Some((f, disk::measure(&relink.path, f).ok()?)));
            Some(News::Relinked(relink, measured))
        }
        Job::Seek {
            book,
            folder,
            sought,
        } => {
            let found = disk::find_again(&folder, &sought, Limits::new(super::wall_ms()));
            Some(News::Sought {
                book,
                folder,
                found,
            })
        }
    }
}

fn rescan(job: Rescan) -> Rescanned {
    let gone = (job.locations).map(|l| l.into_iter().filter(|p| !p.is_file()).collect());
    let mut contents = job.contents;
    let mut out = Rescanned {
        gone,
        scans: Vec::new(),
        settling: Vec::new(),
        truncated: Vec::new(),
    };
    let limits = Limits::new(job.now_ms);
    for source in job.folders {
        let keep = |format, size| source.counts(format, size);
        let walked = disk::walk(&source.root, keep, limits, &job.avoid);
        if walked.settling > 0 {
            out.settling.push(source.id);
        }
        if walked.truncated {
            out.truncated.push(source.id);
        }
        let measured = disk::measure_all(walked.files, &job.known);
        let mut found = Vec::with_capacity(measured.len());
        for (path, format, measure) in measured {
            // Names are read once per content, for books new to the library.
            let fresh = contents.insert(measure.content);
            let (title, author) = match fresh {
                true => mareader_session::describe(&path),
                false => (None, None),
            };
            found.push(Found {
                path,
                format,
                measure,
                title,
                author,
            });
        }
        out.scans.push(Scan {
            source: source.id,
            found,
            offline: walked.offline,
        });
    }
    out
}
