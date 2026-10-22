//! What set books hold right now, counted for the memory audit.
use std::sync::atomic::{AtomicUsize, Ordering};

/// Flows alive: a book set once, a re-set, a result's card.
pub(crate) static FLOWS: AtomicUsize = AtomicUsize::new(0);
/// Estimated bytes of laid blocks every flow keeps.
pub(crate) static LAID: AtomicUsize = AtomicUsize::new(0);
/// Bytes of decoded pictures every book keeps.
pub(crate) static DECODED: AtomicUsize = AtomicUsize::new(0);

/// The counts at one moment, across every book open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Held {
    pub flows: usize,
    pub laid: usize,
    pub decoded: usize,
}

/// What every set book holds now.
pub fn held() -> Held {
    Held {
        flows: FLOWS.load(Ordering::Relaxed),
        laid: LAID.load(Ordering::Relaxed),
        decoded: DECODED.load(Ordering::Relaxed),
    }
}

pub(crate) fn add(counter: &AtomicUsize, n: usize) {
    counter.fetch_add(n, Ordering::Relaxed);
}

pub(crate) fn take(counter: &AtomicUsize, n: usize) {
    counter.fetch_sub(n, Ordering::Relaxed);
}
