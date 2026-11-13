//! Picked pages asked in groups, as many at once as the model allows.

use std::collections::VecDeque;
use std::sync::mpsc::Receiver;

use mareader_library::{AiPageId, AiTask, BookId, PageSpan};
use mareader_session::Document;
use mareader_settings::{AiSettings, PageTask, Provider};

use super::extract::Source;
use super::state::Around;
use super::view::{Models, group_span, page_task};

/// A group's text, if any, the group, and the span it covers.
pub type Read = (Option<Source>, Group, PageSpan);

/// Times a group the model was too busy for is asked again.
pub const TRIES: u8 = 3;

/// Own pages `from` to `to`, both in, asked as one.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub task: AiTask,
    pub from: usize,
    pub to: usize,
    pub redo_of: Option<AiPageId>,
    pub tries: u8,
}

/// A pane's groups: waiting, being read, being written.
pub struct Batch {
    /// The layout the pages are counted in: a book's own.
    pub layout: u64,
    pub book: Option<BookId>,
    pub name: String,
    pub waiting: VecDeque<Group>,
    pub reading: Vec<(Receiver<Option<Source>>, Group, PageSpan)>,
    pub runs: Vec<(u64, Group)>,
    pub total: usize,
    pub finished: usize,
    pub blank: usize,
}

impl Batch {
    pub fn waiting_any(&self) -> bool {
        !self.waiting.is_empty()
    }

    pub fn flying(&self) -> usize {
        self.runs.len() + self.reading.len()
    }

    pub fn has(&self, g: &Group) -> bool {
        let same = |o: &Group| o.task == g.task && o.from == g.from && o.to == g.to;
        self.waiting.iter().any(same)
            || self.runs.iter().any(|(_, o)| same(o))
            || self.reading.iter().any(|(_, o, _)| same(o))
    }

    /// The task now being done, for the pill.
    pub fn task(&self) -> Option<&AiTask> {
        let flying = self.runs.iter().map(|(_, g)| g);
        let reading = self.reading.iter().map(|(_, g, _)| g);
        let mut all = flying.chain(reading).chain(self.waiting.iter());
        all.next().map(|g| &g.task)
    }
}

/// Own pages in order, cut into runs of neighbours of at most `size`.
pub fn groups(pages: &[usize], size: usize) -> Vec<(usize, usize)> {
    let mut pages = pages.to_vec();
    pages.sort_unstable();
    pages.dedup();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for page in pages {
        match out.last_mut() {
            Some((from, to)) if *to + 1 == page && page - *from < size.max(1) => *to = page,
            _ => out.push((page, page)),
        }
    }
    out
}

/// Pages per request: the setting, fewer for a model with little context.
pub fn size_for(settings: &AiSettings, task: &AiTask) -> usize {
    let size = usize::from(settings.tasks.pages.per_request.max(1));
    let preset = settings.page_preset(page_task(task));
    // Apple's model reads about 4,000 tokens in all, answer included.
    match preset.map(|p| p.provider) {
        Some(Provider::Apple) if page_task(task) == PageTask::Summarize => size.min(2),
        Some(Provider::Apple) => 1,
        _ => size,
    }
}

/// What a group covers in the book: its span, or none without text.
pub fn span_of(document: &Document, (from, to): (usize, usize)) -> Option<PageSpan> {
    match document {
        Document::Epub { .. } => {
            let [from, to] = group_span(document, (from, to))?;
            Some(PageSpan::Blocks { from, to })
        }
        Document::Pdf(_) if from == to => Some(PageSpan::Page { page: from }),
        Document::Pdf(_) => Some(PageSpan::Pages { from, to }),
    }
}

/// Groups written at once: the model's room, else its own setting.
pub fn room_for(a: &Around<'_>, task: &AiTask) -> usize {
    let Some(preset) = a.settings.page_preset(page_task(task)) else {
        return 1;
    };
    let room = a.ai.capacity(preset).unwrap_or(u32::from(preset.at_once));
    room.max(1) as usize
}
