//! Study tools on the page: the selection bar, marks and the word card;
//! see `docs/study.md`.

mod acting;
mod answers;
mod bar;
mod card;
mod gloss;
pub use gloss::Gloss;
mod keeping;
mod marking;
pub mod marks;
mod note;
mod picking;
pub use picking::STYLE_ID as PICK_STYLE;
mod restyle;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use egui::{Context, Pos2, Rect};
use mareader_appearance::Rgb;
use mareader_cefr::Role;
use mareader_engine::study::asking::{Got, Said};
use mareader_engine::study::loose::{loose_book, loose_content};
use mareader_library::{
    BookId, DeviceId, Library, Mark, MarkId, MarkKind, NewMark, Pen, Spot, Style,
};
use mareader_settings::{GlossSettings, HighlightSettings};
use mareader_study::{Explainer, Failure, Unavailable, WordInfo};
use mareader_workspace::PaneId;

pub use bar::{BarPress, show as show_bar};
use mareader_engine::study::words::Asked;

use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use mareader_engine::command::Command;

use mareader_engine::study::Action;

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
    /// The word's role in its sentence, once the tagger said.
    role: Option<Role>,
}

mareader_engine::impl_card!(Card);

/// The right-click menu on a mark.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MarkMenu {
    pane: PaneId,
    mark: MarkId,
    at: Pos2,
    /// The mark's words on screen, for a note opened from here.
    rect: Rect,
    /// A highlight's: whether it has a note; none for an AI mark.
    noted: Option<bool>,
    /// A highlight's look, to change; none for an AI mark.
    style: Option<Style>,
    /// Its words were drawn this frame: it follows them, else goes.
    seen: bool,
}

/// A highlight's note beside it: read, or being written.
#[derive(Clone, Debug, PartialEq)]
struct Note {
    pane: PaneId,
    /// The mark as it was opened, its note kept up to date here.
    mark: Mark,
    at: Rect,
    seen: bool,
    /// The text being written; none while the note is only read.
    draft: Option<String>,
    /// The highlight was made for this note: Cancel takes it back.
    placeholder: bool,
    /// Frames left to give the field focus, as the card measures first.
    focus: u8,
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
    note: Option<Note>,
    /// Marks picked to remove together, in one pane.
    picking: Option<picking::Picking>,
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
    note_rect: Option<Rect>,
    /// Books' marks as last listed: every pane asks each frame.
    listed: RefCell<Vec<Listed>>,
}

/// A book's bookmarks and where each is.
pub type Bookmarks = Arc<[(MarkId, Spot)]>;

/// Books listed at most; a ninth lets the oldest go.
const LISTED_CAP: usize = 8;

/// A book's marks and bookmarks, while none changed and the look held.
struct Listed {
    stamp: u64,
    book: BookId,
    look: HighlightSettings,
    shown: Arc<[marks::Shown]>,
    bookmarks: Bookmarks,
}

impl Default for Study {
    fn default() -> Self {
        Self {
            explainer: Arc::new(Unavailable),
            inline: false,
            loose: Library::new(DeviceId(0)),
            card: None,
            menu: None,
            note: None,
            picking: None,
            answers: HashMap::new(),
            partial: HashMap::new(),
            failed: HashMap::new(),
            asking: HashMap::new(),
            done: Vec::new(),
            next_loose: 0,
            card_rect: None,
            menu_rect: None,
            note_rect: None,
            listed: RefCell::default(),
        }
    }
}

impl Study {
    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
    }

    pub fn inline(&self) -> bool {
        self.inline
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
        let id = loose_book(&mut self.loose, name);
        (&mut self.loose, id)
    }

    /// The library keeping `book`'s marks, and its id there.
    fn lib_of<'a>(
        &'a self,
        main: &'a Library,
        book: Option<BookId>,
        name: &str,
    ) -> (&'a Library, BookId) {
        match book.filter(|b| main.book(*b).is_some()) {
            Some(id) => (main, id),
            None => (&self.loose, BookId::original(loose_content(name))),
        }
    }

    fn mark<'a>(&'a self, main: &'a Library, id: MarkId) -> Option<&'a Mark> {
        main.mark(id).or_else(|| self.loose.mark(id))
    }

    /// `pane` drew no pages this frame, as while it re-sets: the card stays.
    pub fn hold(&mut self, pane: PaneId) {
        if let Some(card) = self.card.as_mut().filter(|c| c.pane == pane) {
            card.seen = true;
        }
        if let Some(note) = self.note.as_mut().filter(|n| n.pane == pane) {
            note.seen = true;
        }
        if let Some(menu) = self.menu.as_mut().filter(|m| m.pane == pane) {
            menu.seen = true;
        }
    }

    /// Escape: drops a draft, closes the card or menu, else stops picking.
    pub fn escape(&mut self, main: &mut Library) -> bool {
        if self.note.is_some() {
            self.cancel_note(main);
            return true;
        }
        if self.card.is_some() || self.menu.is_some() {
            self.card = None;
            self.menu = None;
            return true;
        }
        self.picking.take().is_some()
    }

    /// Before the panes draw: marks must show themselves again.
    pub fn begin_frame(&mut self) {
        if let Some(card) = &mut self.card {
            card.seen = matches!(card.key, Key::Loose(_));
        }
        // A draft stays while its words scroll away; a read note goes.
        if let Some(note) = &mut self.note {
            note.seen = note.draft.is_some();
        }
        if let Some(menu) = &mut self.menu {
            menu.seen = false;
        }
    }

    /// After the panes: the card and the mark menu, over them.
    pub fn show(
        &mut self,
        ctx: &Context,
        main: &Library,
        (panes, heard): (&[(PaneId, Rect)], crate::sounds::Heard<'_>),
        (look, pens): (&GlossSettings, &HighlightSettings),
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
        let room = |pane| panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r);
        // A press elsewhere, or its words scrolled away, closes the menu.
        let gone = |m: &MarkMenu| !m.seen || room(m.pane).is_none();
        if outside(self.menu_rect) || self.menu.as_ref().is_some_and(gone) {
            self.menu = None;
        }
        // A press elsewhere keeps a draft; a note only read just closes.
        if let Some(n) = &self.note
            && (outside(self.note_rect) || room(n.pane).is_none() || !n.seen)
        {
            match n.draft {
                Some(_) => out.push(Sent::Engine(Command::Study(Action::SaveNote(n.mark.id)))),
                None => self.note = None,
            }
        }
        // A mark that scrolled away, or a pane that closed, takes its card.
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
                role: c.role.as_ref(),
                answer,
                at: c.at,
                room: room(c.pane)?,
                density: look.density,
                saying: heard.saying(&c.word),
                sound: heard.lines(&c.word),
            };
            Some(card::show(ctx, &view, &t, out))
        });
        self.menu_rect = self.menu.map(|m| {
            card::menu(
                ctx,
                (m.pane, m.mark),
                (m.at, m.rect),
                m.noted,
                (m.style, pens),
                &t,
                out,
            )
        });
        self.note_rect = match &mut self.note {
            Some(n) => room(n.pane).map(|room| {
                // Opened from the panel, before its words were placed.
                let at = match n.at.is_finite() {
                    true => n.at,
                    false => Rect::from_center_size(room.center(), egui::Vec2::splat(1.0)),
                };
                let view = note::View {
                    mark: n.mark.id,
                    quote: &n.mark.word,
                    ink: ink(&n.mark, pens).color,
                    saved: n.mark.note.value.as_deref(),
                    draft: n.draft.as_mut(),
                    fresh: n.focus > 0,
                    at,
                    room,
                };
                n.focus = n.focus.saturating_sub(1);
                note::show(ctx, view, &t, out)
            }),
            None => None,
        };
        if self
            .picking
            .as_ref()
            .is_some_and(|p| room(p.pane).is_none())
        {
            self.picking = None;
        }
        if let Some(p) = &self.picking
            && let Some(r) = room(p.pane)
        {
            // The highlights picked, with their looks; AI marks keep theirs.
            let ids = p.ids.iter().filter_map(|id| self.mark(main, *id));
            let words = ids.filter(|m| m.kind == MarkKind::Highlight);
            let looks: Vec<_> = words.map(|m| (m.id, m.look())).collect();
            picking::bar(ctx, p, (r, &looks, pens), &t, out);
        }
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

/// A page to bookmark, or the bookmark already on it.
pub struct Bookmarking<'a> {
    pub book: Option<BookId>,
    pub name: &'a str,
    pub spot: Spot,
    /// Where it is, e.g. its chapter, kept with it.
    pub label: String,
    pub existing: Option<MarkId>,
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
    let style = m.look();
    let color = restyle::shown(style, look);
    marks::Ink {
        pen: style.pen,
        color,
        ai: false,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::path::PathBuf;

    use mareader_library::Gloss as MarkGloss;
    use mareader_library::{ContentId, Format, Measure, Opened};
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

    #[test]
    fn listed_marks_are_shared_until_one_changes() {
        let (mut main, id) = marked("Fast.");
        let study = Study::default();
        let look = HighlightSettings::default();
        let book = main.mark(id).map(|m| m.book.value);
        let first = study.marks_of(&main, book, "", &look);
        let again = study.marks_of(&main, book, "", &look);
        assert!(
            Arc::ptr_eq(&first, &again),
            "an unchanged book is not listed again"
        );
        assert!(main.set_note(id, Some("a note".into()), 4));
        let noted = study.marks_of(&main, book, "", &look);
        assert!(
            noted.first().is_some_and(|m| m.noted),
            "a change shows at once"
        );
        main.set_marks_alive(&[id], false, 5);
        assert!(study.marks_of(&main, book, "", &look).is_empty());
        assert!(study.bookmarks_of(&main, book, "").is_empty());
    }
}
