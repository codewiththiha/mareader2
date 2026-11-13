//! A pane's continuous run: the pages ahead, asked as the reader reads.

use std::collections::HashSet;
use std::sync::mpsc::Receiver;

use mareader_library::{AiTask, BookId};
use mareader_settings::{AiSettings, language_name};

use super::read::Read;
use super::view::{Models, page_task};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Going,
    /// The reader went far from the run's pages.
    Away,
    /// Still, far from them: the pill asks whether to carry on.
    Asking,
}

/// A pane's continuous run.
pub struct Follow {
    /// The layout its pages are counted in: a book's own.
    pub layout: u64,
    pub task: AiTask,
    pub book: Option<BookId>,
    pub name: String,
    /// Pages asked: from `from` up to, not with, `next`.
    pub from: usize,
    pub next: usize,
    /// The page count it was asked in; a new layout starts afresh.
    pub pages: usize,
    /// The page read, and since when its result showed.
    pub dwell: Option<(usize, f64)>,
    pub dwell_secs: f64,
    /// The last page whose stay asked more.
    pub fired: Option<usize>,
    /// The page last seen, and since when.
    pub seen: (usize, f64),
    pub state: State,
    /// Runs it started, and pages found without text.
    pub runs: Vec<u64>,
    pub blank: HashSet<usize>,
    pub reading: Vec<Receiver<Read>>,
    /// Pages being read off the frame.
    pub pending: usize,
}

/// The pill's words for a task: what it does, and its verb.
pub fn doing(task: &AiTask) -> (String, &'static str) {
    match task {
        AiTask::Translate { language } => {
            let name = language_name(language).unwrap_or(language);
            (format!("Translating to {name}"), "translating")
        }
        AiTask::Simplify => ("Simplifying pages".to_owned(), "simplifying"),
        AiTask::Summary | AiTask::Points => ("Summarizing pages".to_owned(), "summarizing"),
    }
}

/// Pages asked at once: the setting, or fewer if the model has less room.
pub fn allowance(ai: &dyn Models, settings: &AiSettings, task: &AiTask) -> usize {
    let cap = usize::from(settings.tasks.pages.at_once.max(1));
    let preset = settings.page_preset(page_task(task));
    let room = preset.and_then(|p| ai.capacity(p));
    room.map_or(cap, |r| cap.min(r.max(1) as usize))
}
