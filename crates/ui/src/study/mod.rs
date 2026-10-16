//! Study tools on the page: the selection bar, marks and the word card;
//! see `docs/study.md`.

mod bar;
mod card;
pub mod marks;
pub mod words;

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Context, Pos2, Rect};
use mareader_library::{
    BookId, ContentId, DeviceId, Format, Library, Mark, MarkId, Measure, NewMark, Opened, Spot,
};
use mareader_settings::GlossSettings;
use mareader_study::{Explainer, Failure, Unavailable, WordInfo};
use mareader_workspace::PaneId;

pub use bar::{BarPress, show as show_bar};
pub use words::Asked;

use crate::command::{Command, Commands};
use crate::theme::Tokens;
use crate::toast::Toast;

/// How long Undo stays offered after a removal.
pub const UNDO_FOR: f64 = 6.0;

/// What a pane's pages report to the study tools this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Gloss {
    /// Explain was pressed on the pane's selection.
    Explain,
    /// The selection reached the clipboard.
    Copied,
    /// A mark was clicked: its card, beside it.
    Open { mark: MarkId, at: Rect },
    /// A mark was right-clicked.
    Menu { mark: MarkId, at: Pos2 },
    /// Where the open card's mark is now; none while its text loads.
    Anchor(Option<Rect>),
}

/// What the card and the mark menu ask of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Remove(Vec<MarkId>),
    /// Undo: the removed marks come back.
    Restore(Vec<MarkId>),
    /// Asks again after a failure.
    Retry,
    Close,
}

/// Which answer a card shows: a mark's, or one kept by nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Mark(MarkId),
    Loose(u64),
}

#[derive(Clone, Debug, PartialEq)]
struct Card {
    pane: PaneId,
    key: Key,
    word: String,
    context: String,
    /// The word on screen, as of the last frame it was seen.
    at: Rect,
    /// Seen on screen this frame; a mark scrolled away closes it.
    seen: bool,
}

/// The right-click menu on a mark.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MarkMenu {
    mark: MarkId,
    at: Pos2,
}

/// What a worker sends back: the answer so far, then the end.
enum Got {
    Partial(WordInfo),
    Done(Result<WordInfo, Failure>),
}

/// Answers, the open card and menu, and marks kept by no library.
pub struct Study {
    explainer: Arc<dyn Explainer>,
    /// Answers on this thread, as tests and the browser need.
    inline: bool,
    /// Documents outside the library keep marks here, for the session.
    loose: Library,
    card: Option<Card>,
    menu: Option<MarkMenu>,
    answers: HashMap<Key, WordInfo>,
    /// Answers still arriving, as far as they got.
    partial: HashMap<Key, WordInfo>,
    failed: HashMap<Key, Failure>,
    asking: HashMap<Key, Receiver<Got>>,
    next_loose: u64,
    /// Last frame's card and menu, so a press outside closes them.
    card_rect: Option<Rect>,
    menu_rect: Option<Rect>,
}

impl Default for Study {
    fn default() -> Self {
        Self {
            explainer: Arc::new(Unavailable),
            inline: false,
            loose: Library::new(DeviceId(0)),
            card: None,
            menu: None,
            answers: HashMap::new(),
            partial: HashMap::new(),
            failed: HashMap::new(),
            asking: HashMap::new(),
            next_loose: 0,
            card_rect: None,
            menu_rect: None,
        }
    }
}

/// A document outside the library, known by its name for the session.
fn loose_content(name: &str) -> ContentId {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    ContentId(u128::from(h.finish()))
}

impl Study {
    pub fn set_explainer(&mut self, explainer: Arc<dyn Explainer>) {
        self.explainer = explainer;
        self.answers.clear();
        self.partial.clear();
        self.failed.clear();
    }

    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
    }

    /// The mark whose card is open in `pane`.
    pub fn card_mark(&self, pane: PaneId) -> Option<MarkId> {
        match self.card.as_ref().filter(|c| c.pane == pane)?.key {
            Key::Mark(id) => Some(id),
            Key::Loose(_) => None,
        }
    }

    pub fn card_open(&self) -> bool {
        self.card.is_some()
    }

    /// The word the open card explains, for tests.
    pub fn card_word(&self) -> Option<&str> {
        self.card.as_ref().map(|c| c.word.as_str())
    }

    /// The open card's answer, once it came, for tests.
    pub fn card_answer(&self) -> Option<&WordInfo> {
        self.answers.get(&self.card.as_ref()?.key)
    }

    /// The book's library: the app's, else the session's for `name`.
    fn shelf<'a>(
        &'a mut self,
        main: &'a mut Library,
        book: Option<BookId>,
        name: &str,
    ) -> (&'a mut Library, BookId) {
        if let Some(id) = book.filter(|b| main.book(*b).is_some()) {
            return (main, id);
        }
        let opened = Opened {
            path: PathBuf::from(format!("session:{name}")),
            measure: Measure {
                content: loose_content(name),
                stamp: Default::default(),
            },
            format: Format::Pdf,
            title: name.to_owned(),
            author: None,
        };
        let id = self.loose.record_open(opened, 0);
        (&mut self.loose, id)
    }

    /// `book`'s marks, or those of the session's document called `name`.
    pub fn marks_of(
        &self,
        main: &Library,
        book: Option<BookId>,
        name: &str,
    ) -> Vec<(MarkId, Spot)> {
        let (lib, id) = match book.filter(|b| main.book(*b).is_some()) {
            Some(id) => (main, id),
            None => (&self.loose, BookId::original(loose_content(name))),
        };
        lib.marks(id).into_iter().map(|m| (m.id, m.spot)).collect()
    }

    fn mark<'a>(&'a self, main: &'a Library, id: MarkId) -> Option<&'a Mark> {
        main.mark(id).or_else(|| self.loose.mark(id))
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
        let new = asked.spot.map(|spot| NewMark {
            word: asked.word.clone(),
            context: asked.context.clone(),
            spot,
        });
        let (lib, book) = self.shelf(main, book, name);
        let key = match new.and_then(|n| lib.add_mark(book, n, wall_ms)) {
            Some(id) => Key::Mark(id),
            None => {
                self.next_loose += 1;
                Key::Loose(self.next_loose)
            }
        };
        self.open(ctx, pane, key, (asked.word, asked.context), rect);
    }

    /// A click on a mark: its card again.
    pub fn open_mark(&mut self, ctx: &Context, main: &Library, pane: PaneId, id: MarkId, at: Rect) {
        let Some(m) = self.mark(main, id) else {
            return;
        };
        let words = (m.word.clone(), m.context.clone());
        self.open(ctx, pane, Key::Mark(id), words, at);
    }

    fn open(&mut self, ctx: &Context, pane: PaneId, key: Key, words: (String, String), at: Rect) {
        let (word, context) = words;
        self.menu = None;
        // A failed answer is asked for again.
        self.failed.remove(&key);
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
        });
    }

    fn ask(&mut self, ctx: &Context, key: Key, word: &str, context: &str) {
        let explainer = Arc::clone(&self.explainer);
        self.partial.remove(&key);
        if self.inline || cfg!(target_arch = "wasm32") {
            match explainer.explain(word, context, &|_| {}) {
                Ok(info) => {
                    self.answers.insert(key, info.clamped());
                }
                Err(e) => {
                    self.failed.insert(key, e);
                }
            }
            return;
        }
        let (tx, rx) = channel();
        let (word, context, ctx) = (word.to_owned(), context.to_owned(), ctx.clone());
        std::thread::spawn(move || {
            let partial = |info| {
                let _ = tx.send(Got::Partial(info));
                ctx.request_repaint();
            };
            let done = explainer.explain(&word, &context, &partial);
            let _ = tx.send(Got::Done(done));
            ctx.request_repaint();
        });
        self.asking.insert(key, rx);
    }

    /// The right-click menu on a mark.
    pub fn open_menu(&mut self, mark: MarkId, at: Pos2) {
        self.card = None;
        self.menu = Some(MarkMenu { mark, at });
    }

    /// The open card's mark, seen this frame at `at`, or maybe there.
    pub fn anchor(&mut self, pane: PaneId, at: Option<Rect>) {
        if let Some(card) = self.card.as_mut().filter(|c| c.pane == pane) {
            card.at = at.unwrap_or(card.at);
            card.seen = true;
        }
    }

    /// `pane` drew no pages this frame, as while it re-sets: the card stays.
    pub fn hold(&mut self, pane: PaneId) {
        if let Some(card) = self.card.as_mut().filter(|c| c.pane == pane) {
            card.seen = true;
        }
    }

    /// Escape: closes the card or menu; false if neither was open.
    pub fn escape(&mut self) -> bool {
        let open = self.card.is_some() || self.menu.is_some();
        self.card = None;
        self.menu = None;
        open
    }

    /// Takes the card's and menu's asks; marks change in the libraries.
    pub fn act(&mut self, ctx: &Context, main: &mut Library, action: Action, out: &mut Commands) {
        let wall = crate::library::wall_ms();
        match action {
            Action::Remove(ids) => {
                let n = main.set_marks_alive(&ids, false, wall)
                    + self.loose.set_marks_alive(&ids, false, wall);
                let gone = |c: &Card| matches!(c.key, Key::Mark(id) if ids.contains(&id));
                if self.card.as_ref().is_some_and(gone) {
                    self.card = None;
                }
                self.menu = None;
                if n > 0 {
                    let message = match n {
                        1 => "Removed highlight".to_owned(),
                        n => format!("Removed {n} highlights"),
                    };
                    let undo = Command::Study(Action::Restore(ids));
                    out.push(Command::Notify(Toast::info(message).undo(undo)));
                }
            }
            Action::Restore(ids) => {
                main.set_marks_alive(&ids, true, wall);
                self.loose.set_marks_alive(&ids, true, wall);
                out.push(Command::DismissToast);
            }
            Action::Retry => {
                if let Some(card) = self.card.clone() {
                    self.failed.remove(&card.key);
                    self.ask(ctx, card.key, &card.word, &card.context);
                }
            }
            Action::Close => {
                self.card = None;
                self.menu = None;
            }
        }
    }

    /// Takes in answers, finished or still filling in.
    pub fn receive(&mut self) {
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
                Some(Ok(info)) => {
                    self.answers.insert(key, info.clamped());
                }
                Some(Err(e)) => {
                    self.failed.insert(key, e);
                }
                None => {}
            }
        }
    }

    /// Before the panes draw: marks must show themselves again.
    pub fn begin_frame(&mut self) {
        if let Some(card) = &mut self.card {
            card.seen = matches!(card.key, Key::Loose(_));
        }
    }

    /// After the panes: the card and the mark menu, over them.
    pub fn show(
        &mut self,
        ctx: &Context,
        panes: &[(PaneId, Rect)],
        look: &GlossSettings,
        out: &mut Commands,
    ) {
        let pressed = ctx.input(|i| {
            let p = i.pointer.press_origin();
            p.filter(|_| i.pointer.any_pressed())
        });
        let outside = |r: Option<Rect>| pressed.is_some_and(|p| !r.is_some_and(|r| r.contains(p)));
        if self.card.is_some() && outside(self.card_rect) {
            self.card = None;
        }
        if self.menu.is_some() && outside(self.menu_rect) {
            self.menu = None;
        }
        // A mark that scrolled away, or a pane that closed, takes its card.
        let room = |pane| panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r);
        if self
            .card
            .as_ref()
            .is_some_and(|c| !c.seen || room(c.pane).is_none())
        {
            self.card = None;
        }
        let t = Tokens::get(ctx);
        self.card_rect = self.card.as_ref().and_then(|c| {
            let answer = match (self.answers.get(&c.key), self.failed.get(&c.key)) {
                (Some(info), _) => card::Answer::Ready(info),
                (None, Some(e)) => card::Answer::Failed(e),
                (None, None) => match self.partial.get(&c.key) {
                    Some(info) => card::Answer::Partial(info),
                    None => card::Answer::Waiting,
                },
            };
            let view = card::View {
                word: &c.word,
                answer,
                at: c.at,
                room: room(c.pane)?,
                density: look.density,
            };
            Some(card::show(ctx, &view, &t, out))
        });
        self.menu_rect = self.menu.map(|m| card::menu(ctx, m.mark, m.at, &t, out));
    }
}

/// Explain's ask, as the app gathers it.
pub struct Explained<'a> {
    pub pane: PaneId,
    pub book: Option<BookId>,
    /// The document's name: a session's marks are kept by it.
    pub name: &'a str,
    pub asked: Asked,
    /// The word on screen.
    pub rect: Rect,
}

/// The marks' fill: the gloss colour, or the accent, at its opacity.
pub fn tint(look: &GlossSettings, t: &Tokens) -> egui::Color32 {
    let base = look
        .color
        .rgb(look.custom)
        .map_or(t.accent, crate::theme::color);
    base.gamma_multiply(look.opacity.clamp(0.0, 1.0))
}
