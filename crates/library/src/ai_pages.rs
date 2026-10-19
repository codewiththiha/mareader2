//! Pages the AI summarized, translated or simplified, kept with the book.

use std::hash::{BuildHasher, RandomState};

use serde::{Deserialize, Serialize};

use crate::clock::{Field, Stamp};
use crate::extra::Extra;
use crate::id::{AiPageId, BookId};
use crate::library::Library;

/// What was asked of the page.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum AiTask {
    Summary,
    /// A summary as key points.
    Points,
    Translate {
        language: String,
    },
    Simplify,
}

/// The part of the book a result covers.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageSpan {
    /// A PDF's page.
    Page { page: usize },
    /// A book's text from `[block, byte]` up to `[block, byte]`.
    Blocks { from: [usize; 2], to: [usize; 2] },
}

/// One page's result. Only `book` and `alive` change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiPage {
    pub id: AiPageId,
    pub book: Field<BookId>,
    pub task: AiTask,
    pub span: PageSpan,
    /// A hash of the text sent, to tell when the page has changed.
    pub source: String,
    /// The result as the app reads it, kept as written.
    pub result: String,
    /// The provider and model, as the bubble's menu names them.
    pub by: String,
    /// When it was made, in wall milliseconds.
    pub added: u64,
    pub alive: Field<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A result to keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewAiPage {
    pub task: AiTask,
    pub span: PageSpan,
    pub source: String,
    pub result: String,
    pub by: String,
}

impl Library {
    /// `book`'s living results, oldest first.
    pub fn ai_pages(&self, book: BookId) -> Vec<&AiPage> {
        let mut out: Vec<&AiPage> = self
            .ai_pages
            .values()
            .filter(|p| p.alive.value && p.book.value == book)
            .collect();
        out.sort_by_key(|p| (p.added, p.id));
        out
    }

    pub fn ai_page(&self, id: AiPageId) -> Option<&AiPage> {
        self.ai_pages.get(&id).filter(|p| p.alive.value)
    }

    /// Keeps a result; it replaces one for the same task and span.
    pub fn add_ai_page(&mut self, book: BookId, new: NewAiPage, wall_ms: u64) -> Option<AiPageId> {
        self.book(book)?;
        let same = |p: &&AiPage| p.task == new.task && p.span == new.span;
        let old: Vec<AiPageId> = self
            .ai_pages(book)
            .into_iter()
            .filter(same)
            .map(|p| p.id)
            .collect();
        let at = self.clock.tick(wall_ms);
        for id in old {
            if let Some(page) = self.ai_pages.get_mut(&id) {
                page.alive = Field::new(false, at);
            }
        }
        let id = self.new_ai_page_id(at);
        let page = AiPage {
            id,
            book: Field::new(book, at),
            task: new.task,
            span: new.span,
            source: new.source,
            result: new.result,
            by: new.by,
            added: wall_ms,
            alive: Field::new(true, at),
            extra: Extra::default(),
        };
        self.ai_pages.insert(id, page);
        Some(id)
    }

    /// Removes or brings back results; how many changed.
    pub fn set_ai_pages_alive(&mut self, ids: &[AiPageId], alive: bool, wall_ms: u64) -> usize {
        let flips = |id: &&AiPageId| {
            let page = self.ai_pages.get(id);
            page.is_some_and(|p| p.alive.value != alive && self.book(p.book.value).is_some())
        };
        let ids: Vec<AiPageId> = ids.iter().filter(flips).copied().collect();
        if ids.is_empty() {
            return 0;
        }
        let at = self.clock.tick(wall_ms);
        for id in &ids {
            if let Some(page) = self.ai_pages.get_mut(id) {
                page.alive = Field::new(alive, at);
            }
        }
        ids.len()
    }

    /// `gone`'s results go to `keeper`, as a merge folding them decides.
    pub(crate) fn move_ai_pages(&mut self, gone: BookId, keeper: BookId, at: Stamp) {
        for page in self.ai_pages.values_mut_where(|p| p.book.value == gone) {
            page.book = Field::new(keeper, at);
        }
    }

    fn new_ai_page_id(&self, at: Stamp) -> AiPageId {
        let state = RandomState::new();
        let seed = (at.ms, at.counter, self.device().0);
        let half = |salt: u8| u128::from(state.hash_one((seed, salt)));
        let mut id = AiPageId(half(3) << 64 | half(4));
        while self.ai_pages.contains_key(&id) {
            id = AiPageId(id.0.wrapping_add(1));
        }
        id
    }
}

#[cfg(test)]
#[path = "ai_pages_tests.rs"]
mod tests;
