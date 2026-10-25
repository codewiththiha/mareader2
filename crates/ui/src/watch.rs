//! Notices open files saved elsewhere, off the UI thread.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, SystemTime};

/// How often open files are looked at.
pub const EVERY: Duration = Duration::from_secs(1);

/// A file's size and modified time: what changes when it is saved.
pub type Stamp = (u64, Option<SystemTime>);

/// What a file was last seen as, and a change not yet settled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    base: Option<Stamp>,
    pending: Option<Stamp>,
}

impl Seen {
    pub fn new(now: Option<Stamp>) -> Self {
        Self {
            base: now,
            pending: None,
        }
    }

    /// True once a change holds for two looks: never a half-written save.
    pub fn step(&mut self, now: Option<Stamp>) -> bool {
        match now {
            // Gone or unreadable for now: keep what is shown.
            None => self.pending = None,
            Some(s) if Some(s) == self.base => self.pending = None,
            Some(s) if Some(s) == self.pending => {
                (self.base, self.pending) = (Some(s), None);
                return true;
            }
            Some(s) => self.pending = Some(s),
        }
        false
    }
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

/// The files to look at, shared with the looking thread.
#[derive(Default)]
struct Shared {
    paths: Mutex<Vec<PathBuf>>,
}

pub struct Watch {
    shared: Arc<Shared>,
    changed: Receiver<PathBuf>,
    paths: Vec<PathBuf>,
}

impl Watch {
    /// Starts looking, calling `wake` on a change; none without threads.
    pub fn new(wake: impl Fn() + Send + 'static) -> Option<Self> {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let shared = Arc::new(Shared::default());
        let (tell, changed) = mpsc::channel();
        let weak = Arc::downgrade(&shared);
        std::thread::Builder::new()
            .name("file watch".into())
            .spawn(move || run(&weak, &tell, &wake))
            .ok()?;
        Some(Self {
            shared,
            changed,
            paths: Vec::new(),
        })
    }

    /// Looks at exactly `paths` from now on.
    pub fn follow(&mut self, mut paths: Vec<PathBuf>) {
        paths.sort();
        paths.dedup();
        if paths != self.paths {
            let mut held = self
                .shared
                .paths
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            held.clone_from(&paths);
            self.paths = paths;
        }
    }

    /// The files that changed since last asked.
    pub fn changed(&self) -> Vec<PathBuf> {
        self.changed.try_iter().collect()
    }
}

/// Looks every `EVERY` until the `Watch` is dropped.
fn run(shared: &Weak<Shared>, tell: &Sender<PathBuf>, wake: &dyn Fn()) {
    let mut seen: HashMap<PathBuf, Seen> = HashMap::new();
    loop {
        std::thread::sleep(EVERY);
        let Some(held) = shared.upgrade() else {
            return;
        };
        let paths = held
            .paths
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        drop(held);
        seen.retain(|p, _| paths.contains(p));
        let mut woke = false;
        for path in paths {
            let now = stamp(&path);
            let Some(file) = seen.get_mut(&path) else {
                seen.insert(path, Seen::new(now));
                continue;
            };
            if file.step(now) {
                if tell.send(path).is_err() {
                    return;
                }
                woke = true;
            }
        }
        if woke {
            wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_counts_once_it_has_held_still() {
        let t = |s| Some((s, None));
        let mut seen = Seen::new(t(10));
        assert!(!seen.step(t(10)), "unchanged");
        assert!(!seen.step(t(20)), "still being written");
        assert!(!seen.step(t(30)), "still being written");
        assert!(seen.step(t(30)), "settled");
        assert!(!seen.step(t(30)), "told once");
        // A file briefly gone, as an editor's save swaps it, is no change.
        assert!(!seen.step(None));
        assert!(!seen.step(t(30)));
        // One that appears counts as a change.
        let mut fresh = Seen::new(None);
        assert!(!fresh.step(t(5)));
        assert!(fresh.step(t(5)));
    }
}
