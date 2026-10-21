//! A book's scripts, sandboxed in a JavaScript engine over a small page model.

mod engine;
mod tree;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

pub use engine::Engine;

/// The longest one script call may take before the runtime counts as stuck.
pub const STUCK: Duration = Duration::from_secs(4);
/// The longest the runtime waits when no timer is set.
const IDLE: Duration = Duration::from_secs(3600);
/// Stack for the runtime thread; deep scripts need room.
const STACK: usize = 32 << 20;
/// Timers this close are run now.
const SOON: Duration = Duration::from_millis(2);

/// A chapter with scripts.
#[derive(Clone, Debug, Default)]
pub struct Page {
    /// Index of the chapter, handed back in its changes.
    pub chapter: usize,
    pub xhtml: String,
    /// Its scripts' code, in document order.
    pub scripts: Vec<String>,
    /// Its linked style sheets, for `getComputedStyle`.
    pub styles: Vec<String>,
}

/// What the reader did.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// Clicked the element with this node number.
    Click(u32),
    /// Typed this into the field with this node number.
    Value(u32, String),
    /// Clicked the first element matching this selector, in any chapter.
    Pick(String),
}

/// A text field as the reader edits it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Field {
    pub value: String,
    pub hint: String,
    pub multiline: bool,
    pub secret: bool,
}

/// What a chapter's scripts did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change {
    pub chapter: usize,
    /// The chapter's new markup, if it changed.
    pub page: Option<String>,
    /// Its text fields by node number.
    pub fields: Vec<(u32, Field)>,
    /// Messages for the reader, from `alert`.
    pub alerts: Vec<String>,
    pub errors: Vec<String>,
}

impl Change {
    fn says_something(&self) -> bool {
        self.page.is_some() || !self.alerts.is_empty()
    }
}

/// A book's scripts on their own thread.
pub struct Scripts {
    asks: Sender<(usize, Ask)>,
    changes: Receiver<Change>,
    /// When the running call began, in ms after `born`; 0 when idle.
    busy: Arc<AtomicU64>,
    born: Instant,
    /// Pages whose scripts have run once.
    loaded: Arc<AtomicUsize>,
}

impl Scripts {
    /// Starts `pages`' scripts; `wake` runs after each change is sent.
    pub fn start(pages: Vec<Page>, wake: impl Fn() + Send + 'static) -> Option<Self> {
        let (asks, asked) = mpsc::channel();
        let (send, changes) = mpsc::channel();
        let busy = Arc::new(AtomicU64::new(1));
        let born = Instant::now();
        let loaded = Arc::new(AtomicUsize::new(0));
        let (flag, count) = (Arc::clone(&busy), Arc::clone(&loaded));
        let run = move || serve(pages, (&asked, &send), (&flag, &count), born, &wake);
        let named = thread::Builder::new()
            .name("scripts".into())
            .stack_size(STACK);
        named.spawn(run).ok()?;
        Some(Self {
            asks,
            changes,
            busy,
            born,
            loaded,
        })
    }

    pub fn ask(&self, chapter: usize, ask: Ask) {
        let _ = self.asks.send((chapter, ask));
    }

    /// Changes sent since the last call.
    pub fn changes(&self) -> Vec<Change> {
        self.changes.try_iter().collect()
    }

    /// How it is doing, for a reader of logs.
    pub fn describe(&self) -> String {
        let since = self.busy.load(Ordering::Relaxed);
        let now = millis(self.born);
        let loaded = self.loaded.load(Ordering::Relaxed);
        let call = if since == 0 {
            0
        } else {
            now.saturating_sub(since)
        };
        format!("{loaded} pages loaded, {now} ms old, call at {call} ms")
    }

    /// Whether a call is running now.
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed) != 0
    }

    /// Whether a call has run past [`STUCK`]; the runtime should be dropped.
    pub fn stuck(&self) -> bool {
        let since = self.busy.load(Ordering::Relaxed);
        let now = millis(self.born);
        since != 0 && now.saturating_sub(since) > STUCK.as_millis() as u64
    }
}

fn millis(born: Instant) -> u64 {
    (born.elapsed().as_millis() as u64).max(1)
}

/// The runtime thread: loads every page, then answers asks and timers.
fn serve(
    pages: Vec<Page>,
    (asked, send): (&Receiver<(usize, Ask)>, &Sender<Change>),
    (busy, loaded): (&AtomicU64, &AtomicUsize),
    born: Instant,
    wake: &dyn Fn(),
) {
    let mut engines = Vec::new();
    let tell = |change: Change| {
        let sent = send.send(change).is_ok();
        wake();
        sent
    };
    for page in &pages {
        busy.store(millis(born), Ordering::Relaxed);
        let (engine, change) = Engine::new(page);
        busy.store(0, Ordering::Relaxed);
        loaded.fetch_add(1, Ordering::Relaxed);
        if !tell(change) {
            return;
        }
        engines.push(engine);
    }
    loop {
        let wait = engines.iter_mut().filter_map(Engine::due).min();
        match asked.recv_timeout(wait.unwrap_or(IDLE)) {
            Ok((chapter, ask)) => {
                let all = matches!(ask, Ask::Pick(_));
                let asked = engines.iter_mut().filter(|e| all || e.chapter() == chapter);
                for engine in asked {
                    busy.store(millis(born), Ordering::Relaxed);
                    let change = engine.ask(&ask);
                    busy.store(0, Ordering::Relaxed);
                    if !tell(change) {
                        return;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        for engine in &mut engines {
            if !engine.due().is_some_and(|d| d <= SOON) {
                continue;
            }
            busy.store(millis(born), Ordering::Relaxed);
            let change = engine.tick();
            busy.store(0, Ordering::Relaxed);
            if change.says_something() && !tell(change) {
                return;
            }
        }
    }
}
