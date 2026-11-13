//! The word card's answer: asked of the explainer, received, and kept.

use super::{Card, Explained, Key, Study};
use egui::{Context, Rect};
use mareader_cefr::Role;
use mareader_engine::session::Waker;
use mareader_engine::study::asking::{self, Got, Said};
use mareader_library::{Library, MarkId, NewMark};
use mareader_study::{Explainer, WordInfo};
use mareader_workspace::PaneId;
use std::sync::Arc;
use std::sync::mpsc::TryRecvError;

impl Study {
    /// True while someone answers for the AI Dictionary.
    pub fn explains(&self) -> bool {
        !self.explainer.by().is_empty()
    }

    pub fn set_explainer(&mut self, explainer: Arc<dyn Explainer>) {
        // Saved answers stay; Ask again reaches the new AI.
        self.explainer = explainer;
        self.failed.clear();
    }

    /// The mark whose card or note is open in `pane`.
    pub fn card_mark(&self, pane: PaneId) -> Option<MarkId> {
        if let Some(n) = self.note.as_ref().filter(|n| n.pane == pane) {
            return Some(n.mark.id);
        }
        if let Some(c) = self.card.as_ref().filter(|c| c.pane == pane) {
            return match c.key {
                Key::Mark(id) => Some(id),
                Key::Loose(_) => None,
            };
        }
        self.menu.filter(|m| m.pane == pane).map(|m| m.mark)
    }

    pub fn card_open(&self) -> bool {
        self.card.is_some()
    }

    /// The word the open card explains.
    pub fn card_word(&self) -> Option<&str> {
        self.card.as_ref().map(|c| c.word.as_str())
    }

    /// The open card's word and context, to ask for its role.
    pub fn card_words(&self) -> Option<(&str, &str)> {
        let c = self.card.as_ref()?;
        Some((c.word.as_str(), c.context.as_str()))
    }

    /// The role the open card shows, for tests.
    pub fn card_role(&self) -> Option<&Role> {
        self.card.as_ref()?.role.as_ref()
    }

    /// The tagger's answer for the open card, if it is still `word`'s.
    pub fn set_role(&mut self, word: &str, role: Role) {
        mareader_engine::word_card::set_role(&mut self.card, word, role);
    }

    /// The open card's answer, once it came, for tests.
    pub fn card_answer(&self) -> Option<&WordInfo> {
        self.answers.get(&self.card.as_ref()?.key).map(|s| &s.info)
    }

    /// Who gave the open card's answer, for tests.
    pub fn card_by(&self) -> Option<&str> {
        self.answers
            .get(&self.card.as_ref()?.key)
            .map(|s| s.by.as_str())
    }

    /// Explain pressed: marks the word if it can and opens its card.
    pub fn explain(&mut self, ctx: &Context, main: &mut Library, at: Explained<'_>, wall_ms: u64) {
        let Explained {
            pane,
            book,
            name,
            asked,
            rect,
        } = at;
        let new = asked
            .spot
            .map(|spot| NewMark::explain(asked.word.clone(), asked.context.clone(), spot));
        let (lib, book) = self.shelf(main, book, name);
        let key = match new.and_then(|n| lib.add_mark(book, n, wall_ms)) {
            Some(id) => Key::Mark(id),
            None => {
                self.next_loose += 1;
                Key::Loose(self.next_loose)
            }
        };
        self.open(ctx, main, pane, key, (asked.word, asked.context), rect);
    }

    pub(super) fn open(
        &mut self,
        ctx: &Context,
        main: &Library,
        pane: PaneId,
        key: Key,
        words: (String, String),
        at: Rect,
    ) {
        let (word, context) = words;
        self.menu = None;
        self.note = None;
        // A failed answer is asked for again; a saved one is shown.
        self.failed.remove(&key);
        if let Key::Mark(id) = key
            && !self.answers.contains_key(&key)
            && let Some(said) = self.mark(main, id).and_then(Said::of)
        {
            self.answers.insert(key, said);
        }
        if !self.answers.contains_key(&key) && !self.asking.contains_key(&key) {
            self.ask(ctx, key, &word, &context);
        }
        self.card = Some(Card {
            pane,
            key,
            word,
            context,
            at,
            seen: true,
            role: None,
        });
    }

    pub(super) fn ask(&mut self, ctx: &Context, key: Key, word: &str, context: &str) {
        let explainer = Arc::clone(&self.explainer);
        let by = explainer.by();
        self.partial.remove(&key);
        if self.inline || cfg!(target_arch = "wasm32") {
            match explainer.explain(word, context, &|_| {}) {
                Ok(info) => {
                    let info = info.clamped();
                    self.done.push((key, Said { info, by }));
                }
                Err(e) => {
                    self.failed.insert(key, e);
                }
            }
            return;
        }
        let ctx = ctx.clone();
        let wake: Waker = Arc::new(move || ctx.request_repaint());
        let rx = asking::ask(wake, explainer, by, word.to_owned(), context.to_owned());
        self.asking.insert(key, rx);
    }

    /// Takes in answers, finished or still filling in; marks keep theirs.
    pub fn receive(&mut self, main: &mut Library) {
        let mut done = Vec::new();
        for (key, rx) in &self.asking {
            loop {
                match rx.try_recv() {
                    Ok(Got::Partial(info)) => {
                        self.partial.insert(*key, info.clamped());
                    }
                    Ok(Got::Done(answer)) => {
                        done.push((*key, Some(answer)));
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        done.push((*key, None));
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
        }
        for (key, answer) in done {
            self.asking.remove(&key);
            self.partial.remove(&key);
            match answer {
                Some(Ok(said)) => self.done.push((key, said)),
                Some(Err(e)) => {
                    self.failed.insert(key, e);
                }
                None => {}
            }
        }
        self.save(main);
    }

    /// Files finished answers: in the session, and with their marks.
    pub(super) fn save(&mut self, main: &mut Library) {
        let wall = mareader_engine::library::wall_ms();
        for (key, said) in std::mem::take(&mut self.done) {
            if let Key::Mark(id) = key {
                let gloss = Some(said.gloss());
                if !main.set_gloss(id, gloss.clone(), wall) {
                    self.loose.set_gloss(id, gloss, wall);
                }
            }
            self.answers.insert(key, said);
        }
    }
}
