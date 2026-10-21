//! A book's narration: its clips decoded, stretched and played in time.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod decode;
#[cfg(not(target_arch = "wasm32"))]
mod device;
mod feed;
mod resample;
mod stretch;

use std::sync::{Arc, Mutex, PoisonError};

/// The bytes of one audio file and the extension its type goes by.
#[derive(Clone)]
pub struct Clip {
    pub data: Arc<Vec<u8>>,
    pub extension: &'static str,
}

impl std::fmt::Debug for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Clip({} bytes, {})", self.data.len(), self.extension)
    }
}

/// Reads a book's audio file by its path in the archive.
pub type Fetch = Arc<dyn Fn(&str) -> Option<Clip> + Send + Sync>;

/// A stretch of one file to play: `from` to `until` seconds, or its end.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub path: String,
    pub from: f64,
    pub until: Option<f64>,
    /// The caller's name for it, reported while it plays.
    pub tag: u64,
}

/// Why nothing plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The system has no sound output, or it wouldn't open.
    NoDevice,
    /// A file is missing or isn't audio this reader decodes.
    Unreadable(String),
}

/// What is heard now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    /// The segment heard and how far into its file, in seconds.
    pub at: Option<(u64, f64)>,
    pub paused: bool,
    /// Every segment has been heard.
    pub done: bool,
    pub fault: Option<Fault>,
}

/// Playback speeds offered; pitch stays put.
pub const RATES: [f32; 7] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5];

/// What the playing thread is asked to do.
#[derive(Debug)]
enum Command {
    /// Segments, and the generation whose samples may be heard.
    Play(Vec<Segment>, u64),
    Rate(f32),
    Stop,
}

/// The device side: samples waiting to be heard, and what they are.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    pub(crate) ring: feed::Ring,
    pub(crate) paused: bool,
    pub(crate) fault: Option<Fault>,
    /// Bumped by each play or stop; older samples are dropped.
    pub(crate) generation: u64,
}

impl Shared {
    /// Fills `out` with what's next, or silence while paused.
    pub(crate) fn take(&mut self, out: &mut [f32]) {
        if self.paused {
            out.fill(0.0);
            return;
        }
        self.ring.take(out);
    }
}

/// Plays segments on a thread of its own; dropping it stops the sound.
pub struct Player {
    shared: Arc<Mutex<Shared>>,
    #[cfg(not(target_arch = "wasm32"))]
    tx: std::sync::mpsc::Sender<Command>,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Player({:?})", self.status())
    }
}

impl Player {
    /// A player for one book; the device opens on the first `play`.
    pub fn new(fetch: Fetch) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let tx = device::spawn(fetch, Arc::clone(&shared));
            Self { shared, tx }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = fetch;
            lock(&shared).fault = Some(Fault::NoDevice);
            Self { shared }
        }
    }

    /// Plays `segments` in turn, dropping whatever was queued.
    pub fn play(&self, segments: Vec<Segment>) {
        let generation = {
            let mut s = lock(&self.shared);
            s.ring.clear();
            s.paused = false;
            s.fault = None;
            s.generation = s.generation.wrapping_add(1);
            s.generation
        };
        self.send(Command::Play(segments, generation));
    }

    pub fn pause(&self) {
        lock(&self.shared).paused = true;
    }

    pub fn resume(&self) {
        lock(&self.shared).paused = false;
    }

    /// Plays on at `rate` times the speed, from what is heard now.
    pub fn set_rate(&self, rate: f32) {
        self.send(Command::Rate(rate));
    }

    pub fn stop(&self) {
        {
            let mut s = lock(&self.shared);
            s.ring.clear();
            s.generation = s.generation.wrapping_add(1);
        }
        self.send(Command::Stop);
    }

    pub fn status(&self) -> Status {
        let s = lock(&self.shared);
        Status {
            at: s.ring.now(),
            paused: s.paused,
            done: s.ring.done(),
            fault: s.fault.clone(),
        }
    }

    fn send(&self, command: Command) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            // A gone thread has nothing left to stop.
            let _ = self.tx.send(command);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = command;
    }
}

/// The shared state, even if a thread panicked holding it.
pub(crate) fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}
