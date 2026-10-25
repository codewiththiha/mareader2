//! The library's disk work on one thread, or inline for tests and shots.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use egui::Context;

use super::runs::{Beat, Phase, RunId};
use mareader_library::disk::{self, Folder, FoundAgain, Known, Limits, Loaded, Sought};
use mareader_library::store::Store;
use mareader_library::{
    BookId, Changes, ContentId, Copied, DeviceId, Format, Found, Measure, Options,
};

pub enum Job {
    Load(Folder, DeviceId),
    Measure(Box<Probe>),
    Write(Box<Changes>),
    /// Walks a folder and measures what counts in it.
    Walk(Box<Walk>),
    /// Measures files picked or dropped one by one.
    Files(Box<Files>),
    /// Copies measured files into the library's own folder.
    Copy(Box<CopyIn>),
    /// Which of these files are gone.
    Check(Vec<PathBuf>),
    /// Measures a file picked to stand in for a lost book.
    Relink(Relink),
    /// Looks in a folder for a lost book's file.
    Seek {
        book: BookId,
        folder: PathBuf,
        sought: Sought,
    },
    /// Deletes store copies of removed books.
    Delete {
        store: PathBuf,
        paths: Vec<PathBuf>,
    },
}

/// Ties a job's news to the work waiting on it.
pub type Tag = u64;

/// A folder to walk, counting files as `opts` says.
pub struct Walk {
    pub tag: Tag,
    pub run: Option<RunId>,
    pub root: PathBuf,
    pub opts: Options,
    /// Content ids by path, to skip re-hashing unchanged files.
    pub known: Known,
    /// Contents the library has met: no need to read their names.
    pub contents: HashSet<ContentId>,
    /// The library's own folders, never walked.
    pub avoid: Vec<PathBuf>,
    pub now_ms: u64,
}

/// What a walk found, measured.
pub struct Walked {
    pub tag: Tag,
    pub found: Vec<Found>,
    /// The folder couldn't be read: an unplugged drive, a deleted folder.
    pub offline: bool,
    /// Files still being written; worth another look soon.
    pub settling: bool,
    /// Stopped at a limit: only part of it was read.
    pub truncated: bool,
}

pub struct Files {
    pub tag: Tag,
    pub run: Option<RunId>,
    pub paths: Vec<PathBuf>,
}

/// Loose files measured, and those that aren't books.
pub struct Measured {
    pub tag: Tag,
    pub found: Vec<Found>,
    pub skipped: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
}

pub struct CopyIn {
    pub tag: Tag,
    pub run: Option<RunId>,
    pub store: PathBuf,
    pub files: Vec<Found>,
}

/// Copies made, by the path they came from.
pub struct Copies {
    pub tag: Tag,
    pub copies: HashMap<PathBuf, Copied>,
    pub failed: Vec<(PathBuf, String)>,
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
    /// Opened as this book, which may be a twin.
    pub book: Option<BookId>,
}

pub enum News {
    Loaded(Box<Loaded>),
    Measured(Box<Probe>, Measure),
    WriteFailed(String),
    Walked(Box<Walked>),
    Files(Box<Measured>),
    Copied(Box<Copies>),
    /// The checked files that are gone.
    Checked(HashSet<PathBuf>),
    /// The picked file's format and measure; none if unreadable.
    Relinked(Relink, Option<(Format, Measure)>),
    Sought {
        book: BookId,
        folder: PathBuf,
        found: FoundAgain,
    },
    Beat(Beat),
}

pub enum Io {
    Inline {
        news: VecDeque<News>,
        store: Option<Store>,
    },
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
        Self::Inline {
            news: VecDeque::new(),
            store: None,
        }
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
                // The app has gone: no one to tell, so stop.
                let closed = std::cell::Cell::new(false);
                let mut tell = |n: News| {
                    closed.set(closed.get() || post.send(n).is_err());
                    let ctx = wake.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Some(ctx) = ctx.as_ref() {
                        ctx.request_repaint();
                    }
                };
                // Opened by the load, then written by every save.
                let mut store = None;
                for job in inbox {
                    run(job, &mut store, &mut tell);
                    if closed.get() {
                        return;
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
            Self::Inline { news, store } => run(job, store, &mut |n| news.push_back(n)),
            Self::Thread { jobs, .. } => {
                // A dead worker means no disk: the library stays in memory.
                let _ = jobs.as_ref().map(|j| j.send(job));
            }
        }
    }

    /// What finished since the last call.
    pub fn poll(&mut self, ctx: &Context) -> Vec<News> {
        match self {
            Self::Inline { news, .. } => news.drain(..).collect(),
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

/// Does `job`, telling `post` how it goes and how it ended.
fn run(job: Job, store: &mut Option<Store>, post: &mut dyn FnMut(News)) {
    let news = match job {
        Job::Load(folder, device) => {
            let mut loaded = folder.load(device);
            *store = loaded.store.take();
            Some(News::Loaded(Box::new(loaded)))
        }
        // A file gone before it was measured is simply not recorded.
        Job::Measure(probe) => disk::measure(&probe.path, probe.format)
            .ok()
            .map(|m| News::Measured(probe, m)),
        Job::Write(changes) => {
            let saved = match store.as_mut() {
                Some(store) => store.apply(&changes),
                None => Err("the library file isn't open".into()),
            };
            saved
                .err()
                .map(|e| News::WriteFailed(format!("Couldn't save the library: {e}")))
        }
        Job::Walk(job) => Some(News::Walked(Box::new(walk(*job, post)))),
        Job::Files(job) => Some(News::Files(Box::new(files(*job, post)))),
        Job::Copy(job) => Some(News::Copied(Box::new(copy(*job, post)))),
        Job::Check(paths) => {
            let gone = paths.into_iter().filter(|p| !p.is_file());
            Some(News::Checked(gone.collect()))
        }
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
        Job::Delete { store, paths } => {
            disk::delete_copies(&store, &paths);
            None
        }
    };
    if let Some(news) = news {
        post(news);
    }
}

/// A beat for `run`, if there is one and it moves the ring.
fn beat(
    post: &mut dyn FnMut(News),
    run: Option<RunId>,
    phase: Phase,
    at: (usize, usize),
    name: &Path,
) {
    let (done, total) = at;
    let Some(run) = run.filter(|_| Beat::worth(done, total)) else {
        return;
    };
    let name = name.file_name().map(|n| n.to_string_lossy().into_owned());
    post(News::Beat(Beat {
        run,
        phase,
        done,
        total,
        name: name.unwrap_or_default(),
    }));
}

fn walk(job: Walk, post: &mut dyn FnMut(News)) -> Walked {
    let limits = Limits::new(job.now_ms);
    let opts = &job.opts;
    beat(post, job.run, Phase::Scanning, (0, 0), &job.root);
    let walked = disk::walk(
        &job.root,
        |f, size| opts.admits(f, size),
        limits,
        &job.avoid,
    );
    let total = walked.files.len();
    let each = |done, path: &Path| beat(post, job.run, Phase::Reading, (done, total), path);
    let measured = disk::measure_all(walked.files, &job.known, each);
    let mut contents = job.contents;
    let found = measured.into_iter().map(|(path, format, measure)| {
        // Names are read once per content, for books new to the library.
        let fresh = contents.insert(measure.content);
        found(path, format, measure, fresh)
    });
    Walked {
        tag: job.tag,
        found: found.collect(),
        offline: walked.offline,
        settling: walked.settling > 0,
        truncated: walked.truncated,
    }
}

fn files(job: Files, post: &mut dyn FnMut(News)) -> Measured {
    let mut out = Measured {
        tag: job.tag,
        found: Vec::new(),
        skipped: Vec::new(),
        failed: Vec::new(),
    };
    let mut seen = HashSet::new();
    let total = job.paths.len();
    for (done, path) in job.paths.into_iter().enumerate() {
        beat(post, job.run, Phase::Reading, (done, total), &path);
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let Some(format) = Format::of_path(&path).filter(|_| path.is_file()) else {
            out.skipped.push(path);
            continue;
        };
        match disk::measure(&path, format) {
            // The same file dropped twice in one go is taken once.
            Ok(m) if !seen.insert(m.content) => {}
            Ok(m) => out.found.push(found(path, format, m, true)),
            Err(e) => out.failed.push((path, e.to_string())),
        }
    }
    out
}

fn copy(job: CopyIn, post: &mut dyn FnMut(News)) -> Copies {
    let mut out = Copies {
        tag: job.tag,
        copies: HashMap::new(),
        failed: Vec::new(),
    };
    let total = job.files.len();
    for (done, file) in job.files.into_iter().enumerate() {
        beat(post, job.run, Phase::Copying, (done, total), &file.path);
        let content = file.measure.content;
        match disk::copy_in(&job.store, &file.path, content) {
            Ok((path, stamp)) => {
                // Its pictures come along, or the copy would show alt text.
                if file.format == Format::Markdown {
                    let pictures = mareader_session::pictures(&file.path);
                    disk::copy_beside(&path, &file.path, &pictures);
                }
                let measure = Measure { stamp, content };
                out.copies.insert(file.path, Copied { path, measure });
            }
            Err(e) => out.failed.push((file.path, e.to_string())),
        }
    }
    out
}

/// A measured file; its own title and author are read if `describe`.
fn found(path: PathBuf, format: Format, measure: Measure, describe: bool) -> Found {
    let (title, author) = match describe {
        true => mareader_session::describe(&path),
        false => (None, None),
    };
    Found {
        path,
        format,
        measure,
        title,
        author,
    }
}
