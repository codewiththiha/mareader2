//! Files the system hands us after launch, posted from any thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Shared by the window and whatever forwards files to it.
#[derive(Clone, Default)]
pub struct Inbox {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    paths: Mutex<Vec<PathBuf>>,
    raise: AtomicBool,
    /// Set once the window exists, so a post can wake it.
    ctx: OnceLock<egui::Context>,
}

/// What the window takes from the inbox each frame.
#[derive(Debug, Default, PartialEq)]
pub struct Mail {
    pub paths: Vec<PathBuf>,
    /// A second launch asked for the window, maybe with no file.
    pub raise: bool,
}

impl Inbox {
    pub fn wake_with(&self, ctx: egui::Context) {
        let _ = self.inner.ctx.set(ctx);
    }

    /// Queues `paths`; `raise` also brings the window forward.
    pub fn post(&self, paths: impl IntoIterator<Item = PathBuf>, raise: bool) {
        let mut queued = self.inner.paths.lock().unwrap_or_else(|e| e.into_inner());
        queued.extend(paths);
        drop(queued);
        if raise {
            self.inner.raise.store(true, Ordering::Relaxed);
        }
        if let Some(ctx) = self.inner.ctx.get() {
            ctx.request_repaint();
        }
    }

    pub fn take(&self) -> Mail {
        let mut queued = self.inner.paths.lock().unwrap_or_else(|e| e.into_inner());
        Mail {
            paths: std::mem::take(&mut *queued),
            raise: self.inner.raise.swap(false, Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mail_is_taken_once() {
        let inbox = Inbox::default();
        inbox.post([PathBuf::from("a.pdf")], false);
        inbox.clone().post([PathBuf::from("b.pdf")], true);
        let mail = inbox.take();
        assert_eq!(mail.paths, [PathBuf::from("a.pdf"), "b.pdf".into()]);
        assert!(mail.raise);
        assert_eq!(inbox.take(), Mail::default());
    }
}
