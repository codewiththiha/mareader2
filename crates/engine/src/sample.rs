//! Documents made in memory, so a test can read one without a file.
//!
//! A `DocId` comes only from opening, and opening is asynchronous; a test that
//! needs a handle should not wait on a worker to get one. The sample itself is
//! drawn by `mareader-pdf`, so nothing here makes a pixel.

use std::sync::Arc;

use mareader_pdf::sample::pdf;
use mareader_session::{Budget, DocId, Event, Session, Source};

/// A `DocId` for a one-page sample that opened here, synchronously.
///
/// The only failure is a sample that would not open: a broken build, not a
/// condition a caller handles.
pub fn doc() -> Result<DocId, &'static str> {
    let mut s = Session::new(0, || {});
    let bytes = Arc::new(pdf(1, "t"));
    s.open(Source::Bytes(bytes), None, Default::default());
    let budget = Budget {
        items: 1,
        bytes: usize::MAX,
    };
    s.poll(budget)
        .into_iter()
        .find_map(|e| match e {
            Event::Opened { result: Ok(id), .. } => Some(id),
            _ => None,
        })
        .ok_or("the sample didn't open")
}
