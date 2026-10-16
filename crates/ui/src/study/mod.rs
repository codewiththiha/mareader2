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
use mareader_appearance::Rgb;
use mareader_library::Gloss as MarkGloss;
use mareader_library::{
    BookId, ContentId, DeviceId, Format, Library, Mark, MarkId, MarkKind, Measure, NewMark, Opened,
    Pen, Spot,
};
use mareader_settings::{GlossSettings, HighlightSettings};
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
    /// A highlighter in the selection bar; a colour if one was picked.
    Highlight(Pen, Option<Rgb>),
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
    /// Asks the AI chosen now; its answer replaces the saved one.
    AskAgain,
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
    Done(Result<Said, Failure>),
}

/// A finished answer and who gave it.
#[derive(Clone, Debug, PartialEq)]
struct Said {
    info: WordInfo,
    by: String,
}

impl Said {
    fn of(mark: &Mark) -> Option<Self> {
        let g = mark.gloss.value.as_ref()?;
        let info = WordInfo {
            pos: g.pos.clone(),
            meaning: g.meaning.clone(),
            synonyms: g.synonyms.clone(),
            usages: g.usages.clone(),
        };
        Some(Self {
            info,
            by: g.by.clone(),
        })
    }

    fn gloss(&self) -> MarkGloss {
        MarkGloss {
            pos: self.info.pos.clone(),
            meaning: self.info.meaning.clone(),
            synonyms: self.info.synonyms.clone(),
            usages: self.info.usages.clone(),
            by: self.by.clone(),
        }
    }
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
    /// Answers this session; a mark's are saved with it too.
    answers: HashMap<Key, Said>,
    /// Answers still arriving, as far as they got.
    partial: HashMap<Key, WordInfo>,
    failed: HashMap<Key, Failure>,
    asking: HashMap<Key, Receiver<Got>>,
    /// Finished answers not yet filed, as inline asks leave them.
    done: Vec<(Key, Said)>,
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
            done: Vec::new(),
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
        // Saved answers stay; Ask again reaches the new AI.
        self.explainer = explainer;
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
        self.answers.get(&self.card.as_ref()?.key).map(|s| &s.info)
    }

    /// Who gave the open card's answer, for tests.
    pub fn card_by(&self) -> Option<&str> {
        self.answers
            .get(&self.card.as_ref()?.key)
            .map(|s| s.by.as_str())
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

    /// `book`'s marks, else the session document `name`'s; AI marks last.
    pub fn marks_of(
        &self,
        main: &Library,
        book: Option<BookId>,
        name: &str,
        look: &HighlightSettings,
    ) -> Vec<marks::Shown> {
        let (lib, id) = match book.filter(|b| main.book(*b).is_some()) {
            Some(id) => (main, id),
            None => (&self.loose, BookId::original(loose_content(name))),
        };
        let mut out: Vec<marks::Shown> = lib
            .marks(id)
            .into_iter()
            .map(|m| marks::Shown {
                id: m.id,
                spot: m.spot,
                ink: ink(m, look),
            })
            .collect();
        out.sort_by_key(|m| m.ink.ai);
        out
    }

    /// The reader's highlighter: marks the text in `pen`.
    pub fn highlight(&mut self, main: &mut Library, at: Highlighted<'_>, wall_ms: u64) {
        let color = at.color.map(|Rgb(r, g, b)| [r, g, b]);
        let new = NewMark::highlight(at.text, at.spot, at.pen, color);
        let (lib, book) = self.shelf(main, at.book, at.name);
        lib.add_mark(book, new, wall_ms);
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

    /// A click on a mark: its card again; a highlight's menu.
    pub fn open_mark(&mut self, ctx: &Context, main: &Library, pane: PaneId, id: MarkId, at: Rect) {
        let Some(m) = self.mark(main, id) else {
            return;
        };
        if m.kind == MarkKind::Highlight {
            let p = ctx.pointer_latest_pos().unwrap_or(at.left_bottom());
            self.open_menu(id, p);
            return;
        }
        let words = (m.word.clone(), m.context.clone());
        self.open(ctx, main, pane, Key::Mark(id), words, at);
    }

    fn open(
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
        });
    }

    fn ask(&mut self, ctx: &Context, key: Key, word: &str, context: &str) {
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
        let (tx, rx) = channel();
        let (word, context, ctx) = (word.to_owned(), context.to_owned(), ctx.clone());
        std::thread::spawn(move || {
            let partial = |info| {
                let _ = tx.send(Got::Partial(info));
                ctx.request_repaint();
            };
            let done = explainer.explain(&word, &context, &partial);
            let said = done.map(|i| Said {
                info: i.clamped(),
                by,
            });
            let _ = tx.send(Got::Done(said));
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
            Action::AskAgain => {
                // The saved answer stays until a new one replaces it.
                if let Some(card) = self.card.clone()
                    && !self.asking.contains_key(&card.key)
                {
                    self.answers.remove(&card.key);
                    self.failed.remove(&card.key);
                    self.ask(ctx, card.key, &card.word, &card.context);
                }
                self.save(main);
            }
            Action::Close => {
                self.card = None;
                self.menu = None;
            }
        }
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
    fn save(&mut self, main: &mut Library) {
        let wall = crate::library::wall_ms();
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
                (Some(said), _) => card::Answer::Ready(&said.info, &said.by),
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

/// The reader's highlight, as the app gathers it.
pub struct Highlighted<'a> {
    pub book: Option<BookId>,
    pub name: &'a str,
    pub text: String,
    pub spot: Spot,
    pub pen: Pen,
    /// A colour picked for it; none follows the pen's.
    pub color: Option<Rgb>,
}

/// How `m` is drawn; a highlight never takes a reserved look.
fn ink(m: &Mark, look: &HighlightSettings) -> marks::Ink {
    if m.kind == MarkKind::Explain {
        return marks::Ink {
            pen: look.ai.pen,
            color: look.ai.color,
            ai: true,
        };
    }
    let picked = m.color.map(|[r, g, b]| Rgb(r, g, b));
    let color = picked
        .filter(|c| look.allowed(m.pen, *c))
        .unwrap_or(look.color(m.pen));
    marks::Ink {
        pen: m.pen,
        color,
        ai: false,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use mareader_study::Canned;

    use super::*;

    fn marked(said: &str) -> (Library, MarkId) {
        let mut main = Library::new(DeviceId(1));
        let opened = Opened {
            path: PathBuf::from("/a.pdf"),
            measure: Measure {
                content: ContentId(7),
                stamp: Default::default(),
            },
            format: Format::Pdf,
            title: "A".into(),
            author: None,
        };
        let book = main.record_open(opened, 1);
        let spot = Spot::Page {
            page: 0,
            rect: [1.0, 2.0, 3.0, 4.0],
        };
        let new = NewMark::explain("quick".into(), "the quick fox".into(), spot);
        let id = main.add_mark(book, new, 2).unwrap();
        let gloss = MarkGloss {
            meaning: said.into(),
            by: "Ollama · gemma".into(),
            ..MarkGloss::default()
        };
        main.set_gloss(id, Some(gloss), 3);
        (main, id)
    }

    #[test]
    fn a_saved_answer_shows_without_asking_and_ask_again_replaces_it() {
        let (mut main, id) = marked("Fast.");
        let ctx = Context::default();
        // A fresh session whose AI can't answer: the saved answer shows.
        let mut study = Study::default();
        study.set_inline(true);
        study.open_mark(&ctx, &main, PaneId(1), id, Rect::ZERO);
        let meaning = study.card_answer().map(|a| a.meaning.clone());
        assert_eq!(meaning.as_deref(), Some("Fast."));
        assert_eq!(study.card_by(), Some("Ollama · gemma"));

        study.set_explainer(Arc::new(Canned));
        let mut out = Commands::new();
        study.act(&ctx, &mut main, Action::AskAgain, &mut out);
        assert_eq!(study.card_by(), Some("Sample answers"));
        let saved = main.mark(id).and_then(|m| m.gloss.value.clone()).unwrap();
        assert_eq!(saved.by, "Sample answers");
        assert_ne!(saved.meaning, "Fast.");
    }
}
