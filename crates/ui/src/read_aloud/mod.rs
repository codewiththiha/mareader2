//! Reads a narrated book aloud: its player, the phrase lit, the bar.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use egui::{Color32, Context, CornerRadius, Id, Order, Rect, Ui};
use mareader_audio::{Clip, Fault, Fetch, Player, Segment};
use mareader_epub::{Files, Narration, Run, Skips};
use mareader_session::{Anchor, Document};
use mareader_settings::ReadAloudSettings;
use mareader_speech::Spot;
use mareader_workspace::PaneId;

use crate::command::{Command, Commands, Download};
use crate::documents::{Documents, Reading};
use crate::icons::Icon;
use crate::motion;
use crate::selection::Pages;
use crate::theme::Tokens;
use crate::widgets;

mod booth;
mod card;
mod control;
pub mod marker;
mod spoken;
mod words;

pub use booth::Booth;
use control::{Act, Fit, View};
use spoken::Spoken;

/// How often the lit phrase is checked while it plays.
const TICK: Duration = Duration::from_millis(50);
/// Seconds the player takes to rise into place.
const APPEAR: f64 = 0.22;
/// Seconds the pill takes to open into the card, and back.
const MORPH: f32 = 0.34;
/// Seconds the words lit may be out of sight before the card shows.
const HIDDEN: f64 = 0.8;
/// The player's place closes this share of its gap each second, roughly.
const GLIDE: f32 = 14.0;
/// Seconds after a card's word before its engine is let go.
const WORD_IDLE: f64 = 60.0;
/// The speaker on a word's card, and the same button while it speaks.
pub const SAY: &str = "Say aloud";
pub const STOP_SAYING: &str = "Stop saying";

/// The words being read, to paint in their pane.
#[derive(Clone, Debug, PartialEq)]
pub struct Lit {
    pub span: Span,
    /// The book's own highlight, if it names one.
    pub fill: Option<[u8; 3]>,
}

/// Where reading starts: the place or character under the pointer, or the page.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReadFrom {
    pub anchor: Option<Anchor>,
    pub at: Option<Spot>,
    /// Where it stops: a selection's end, read alone by the voice.
    pub until: Option<Spot>,
}

impl ReadFrom {
    pub fn here(&self) -> bool {
        self.anchor.is_some() || self.at.is_some()
    }
}

/// Where the words read are: by place in the book, or on its pages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Span {
    Anchors { start: Anchor, end: Anchor },
    Chars { start: Spot, end: Spot },
}

/// A narrated book being read aloud in one pane: its own recordings.
struct Narrated {
    pane: PaneId,
    narration: Arc<Narration>,
    document: Arc<Document>,
    /// Seconds of sound before each phrase, and in all at the end.
    lengths: Vec<f64>,
    /// Sought: the pane turns to the phrase, following or not.
    jump: bool,
    player: Player,
    /// What was last asked to play, a run per segment tag.
    runs: Vec<Run>,
    /// The phrase heard last; where playing last started.
    heard: Option<usize>,
    started: usize,
    /// The page last turned to, so the reader may look away between.
    followed: Option<usize>,
    skip: Skips,
    rate: f32,
    paused: bool,
    finished: bool,
    fault: Option<Fault>,
}

impl Narrated {
    fn play_from(&mut self, from: usize) {
        self.runs = self.narration.runs(from, self.skip);
        let segments = (self.runs.iter().enumerate())
            .map(|(tag, run)| Segment {
                path: run.path.clone(),
                from: run.from,
                until: run.until,
                tag: tag as u64,
            })
            .collect();
        self.player.set_rate(self.rate);
        self.player.play(segments);
        self.heard = Some(from);
        self.started = from;
        self.followed = None;
        self.finished = false;
        self.paused = false;
        self.fault = None;
    }

    /// The first phrase read at or after `i`.
    fn read_from(&self, i: usize) -> Option<usize> {
        match self.narration.reads(i, self.skip) {
            true => Some(i),
            false => self.narration.next(i, self.skip),
        }
    }

    fn toggle(&mut self) {
        if self.finished {
            if let Some(first) = self.read_from(0) {
                self.play_from(first);
            }
        } else if self.paused {
            self.player.resume();
            self.paused = false;
        } else {
            self.player.pause();
            self.paused = true;
        }
    }

    fn step(&mut self, to: Option<usize>) {
        if let Some(to) = to {
            self.play_from(to);
        }
    }

    /// Follows what is heard and turns `here`, its pane, if shown.
    fn tick(
        &mut self,
        ctx: &Context,
        documents: &mut Documents,
        here: Option<&Reading>,
        settings: &ReadAloudSettings,
    ) -> bool {
        if let Some(reading) = here {
            self.document = Arc::clone(&reading.document);
        }
        if (settings.rate - self.rate).abs() > f32::EPSILON {
            self.rate = settings.rate;
            self.player.set_rate(self.rate);
        }
        let status = self.player.status();
        let run = |tag: u64| usize::try_from(tag).ok().and_then(|t| self.runs.get(t));
        if let Some((run, time)) = status.at.and_then(|(tag, time)| Some((run(tag)?, time))) {
            self.heard = self.narration.phrase_at(run, time).or(self.heard);
        }
        self.paused = status.paused;
        self.finished = status.done && status.fault.is_none() && !self.runs.is_empty();
        self.fault = status.fault;
        let skip = skips(settings);
        if skip != self.skip {
            self.skip = skip;
            let from = self.heard.unwrap_or(self.started);
            if let Some(from) = self.read_from(from).filter(|_| !self.finished) {
                self.play_from(from);
            }
        }
        let shown = self.heard.and_then(|i| self.narration.phrases.get(i));
        let page = shown.and_then(|p| self.document.locate(p.start));
        // Away from its pane, the book reads on unseen.
        if let Some(reading) = here {
            if std::mem::take(&mut self.jump)
                && let Some(page) = page
            {
                documents.go_to(self.pane, page);
                self.followed = Some(page);
            }
            follow(documents, reading, settings, page, &mut self.followed);
        }
        if !self.paused && !self.finished {
            ctx.request_repaint_after(TICK);
        }
        true
    }

    /// What the player shows: phrases for steps, the recordings for time.
    fn view<'a>(&self, hint: &'a dyn Fn(f32) -> Option<String>) -> View<'a> {
        let heard = self.heard.unwrap_or(self.started);
        let count = self.narration.phrases.len().max(1);
        let total = self.lengths.last().copied().unwrap_or_default();
        let before = self.lengths.get(heard).copied().unwrap_or(total);
        let rate = f64::from(self.rate.max(0.1));
        let note = match (&self.fault, self.finished) {
            (Some(fault), _) => Some((fault_text(fault), true)),
            (None, true) => Some(("The end".to_owned(), false)),
            (None, false) => None,
        };
        View {
            playing: !self.paused && !self.finished,
            ready: true,
            loading: false,
            back: self.narration.previous(heard, self.skip).is_some(),
            next: self.narration.next(heard, self.skip).is_some(),
            unit: "phrase",
            escape: self.narration.escape(heard, self.skip).is_some(),
            at: Some(if self.finished {
                1.0
            } else {
                heard as f32 / count as f32
            }),
            left: Some(if self.finished {
                0.0
            } else {
                (total - before) / rate
            }),
            total: Some(total / rate),
            seek: true,
            ticks: Vec::new(),
            note,
            hint,
            narrated: true,
        }
    }

    /// The phrase at `share` of the book.
    fn phrase_at(&self, share: f32) -> usize {
        let count = self.narration.phrases.len();
        ((share * count as f32) as usize).min(count.saturating_sub(1))
    }

    fn hint(&self, share: f32) -> Option<String> {
        let phrase = self.narration.phrases.get(self.phrase_at(share))?;
        let page = self.document.locate(phrase.start)?;
        Some(place_name(&self.document, page))
    }

    /// Does what the player asked; true to stop.
    fn act(&mut self, act: Act) -> bool {
        let heard = self.heard.unwrap_or(self.started);
        match act {
            Act::Back => self.step(self.narration.previous(heard, self.skip)),
            Act::Next => self.step(self.narration.next(heard, self.skip)),
            Act::Escape => self.step(self.narration.escape(heard, self.skip)),
            Act::Toggle => self.toggle(),
            Act::Seek(share) => {
                let to = self.read_from(self.phrase_at(share));
                self.jump = to.is_some();
                self.step(to);
            }
            Act::Stop => return true,
            Act::Card(_) | Act::Reveal => {}
        }
        false
    }

    /// Whether `reading` is this narration's book, as it was opened.
    fn ours(&self, reading: &Reading) -> bool {
        let narration = reading.document.narration();
        narration.is_some_and(|n| Arc::ptr_eq(&n, &self.narration))
    }

    /// Reads on in `reading`, the book opened again; false if it differs.
    fn rehome(&mut self, reading: &Reading) -> bool {
        let Some(narration) = reading.document.narration() else {
            return false;
        };
        // Its phrases' places are the book's own, whatever the type.
        if narration.phrases.len() != self.narration.phrases.len() {
            return false;
        }
        self.narration = narration;
        self.document = Arc::clone(&reading.document);
        self.pane = reading.pane;
        (self.jump, self.followed) = (true, None);
        true
    }

    /// The page the phrase heard is on, and where it is named.
    fn page(&self) -> Option<usize> {
        let phrase = self.narration.phrases.get(self.heard?)?;
        self.document.locate(phrase.start)
    }

    fn lit(&self) -> Option<Lit> {
        let phrase = self.narration.phrases.get(self.heard?)?;
        Some(Lit {
            span: Span::Anchors {
                start: phrase.start,
                end: phrase.end,
            },
            fill: self.narration.highlight,
        })
    }
}

/// What reads: the book's narration, or a voice saying its text.
enum Session {
    Narrated(Narrated),
    Spoken(Box<Spoken>),
}

impl Session {
    fn pane(&self) -> PaneId {
        match self {
            Self::Narrated(s) => s.pane,
            Self::Spoken(s) => s.pane,
        }
    }

    fn hint(&self, share: f32) -> Option<String> {
        match self {
            Self::Narrated(s) => s.hint(share),
            Self::Spoken(s) => s.hint(share),
        }
    }

    /// Whether `reading` shows this book as it was when reading began.
    fn ours(&self, reading: &Reading) -> bool {
        match self {
            Self::Narrated(s) => s.ours(reading),
            Self::Spoken(s) => s.ours(reading),
        }
    }

    /// Reads on in `reading`, the book opened again; false if unsuitable.
    fn rehome(&mut self, reading: &Reading) -> bool {
        match self {
            Self::Narrated(s) => s.rehome(reading),
            Self::Spoken(s) => s.rehome(reading),
        }
    }

    fn view<'a>(&self, hint: &'a dyn Fn(f32) -> Option<String>) -> View<'a> {
        match self {
            Self::Narrated(s) => s.view(hint),
            Self::Spoken(s) => s.view(hint),
        }
    }

    /// Does what the player asked; true to stop.
    fn act(&mut self, act: Act) -> bool {
        match self {
            Self::Narrated(s) => s.act(act),
            Self::Spoken(s) => s.act(act),
        }
    }

    /// The page read, and the characters lit on it when known.
    fn spot(&self) -> Option<(usize, Option<std::ops::Range<usize>>)> {
        match self {
            Self::Narrated(s) => Some((s.page()?, None)),
            Self::Spoken(s) => s.spot().map(|(page, chars)| (page, Some(chars))),
        }
    }

    /// What the card says about the book, named `title`.
    fn about<'a>(&'a self, title: &'a str) -> card::About<'a> {
        let (place, narrator, playing) = match self {
            Self::Narrated(s) => {
                let place = s.page().map(|p| place_name(&s.document, p));
                let playing = !s.paused && !s.finished;
                (place, s.narration.narrator.as_deref(), playing)
            }
            Self::Spoken(s) => (s.whereabouts(), None, s.playing()),
        };
        card::About {
            title,
            place,
            narrator,
            playing,
        }
    }

    fn lyrics(&self) -> Option<card::Lyrics> {
        match self {
            Self::Narrated(_) => None,
            Self::Spoken(s) => s.lyrics(),
        }
    }
}

/// Where the player may show, and whether the words read are in sight.
pub struct Stage<'a> {
    /// The panes are up: the reader is the page shown.
    pub reader: bool,
    pub panes: &'a [(PaneId, Rect)],
    /// The window's room below the title bar.
    pub content: Rect,
    /// Whether the words lit are in sight, when they can be told.
    pub shown: Option<bool>,
    /// The title of the book read.
    pub title: &'a str,
}

/// The player's shape and place, kept between frames.
#[derive(Default)]
struct Shape {
    /// The card or the pill, asked by hand until the need for it changes.
    chosen: Option<bool>,
    /// The words read can't be seen, so the card would help.
    auto: bool,
    /// Since when the words lit have been out of sight.
    hidden_since: Option<f64>,
    /// Whether the card is wanted, this frame.
    card: bool,
    /// The foot centre it sits on, gliding where it should.
    at: Option<egui::Pos2>,
    time: Option<f64>,
    scroll: card::Scroll,
    /// When the player first showed for this reading.
    appeared: Option<f64>,
}

/// Read-aloud for the window: at most one book reads at a time.
#[derive(Default)]
pub struct ReadAloud {
    session: Option<Session>,
    /// Voices to pick from and try, while settings show them.
    pub booth: Booth,
    /// Voice models on offer, downloaded or coming down.
    pub shelf: mareader_speech::Shelf,
    /// Says a card's word in the voice books are read in.
    word: Booth,
    /// The word being said, and when one was last asked.
    saying: Option<String>,
    said_at: f64,
    shape: Shape,
    /// The book read, to find it again once its pane shows another.
    origin: Option<Origin>,
    origin_at: Option<PaneId>,
    /// Its pane shows another book, or is gone: it reads on unseen.
    away: bool,
}

/// The book being read, as the library and files know it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Origin {
    pub book: Option<mareader_library::BookId>,
    pub path: Option<std::path::PathBuf>,
    pub name: String,
}

impl Origin {
    /// Whether a pane showing `book` from `path` shows this book.
    pub fn is(
        &self,
        book: Option<mareader_library::BookId>,
        path: Option<&std::path::Path>,
    ) -> bool {
        let same_book = self.book.is_some() && book == self.book;
        let same_file = self.path.is_some() && path == self.path.as_deref();
        same_book || same_file
    }
}

impl ReadAloud {
    /// Does what a voice-model button asked; a failed delete says why.
    pub fn act(
        &mut self,
        (item, action): (mareader_speech::Item, Download),
        settings: &mut ReadAloudSettings,
    ) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        match action {
            Download::Start => self.shelf.download(item),
            Download::Pause => self.shelf.pause(item),
            Download::Cancel => self.shelf.cancel(item),
            Download::Remove => {
                let removed = self.shelf.remove(item);
                // Samples may hold the model; the booth opens again when shown.
                self.booth.close();
                self.stop_word();
                self.word.close();
                self.let_go(settings);
                return removed;
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (item, action, settings);
        Ok(())
    }

    /// Off a model no longer downloaded, onto the system voices.
    #[cfg(not(target_arch = "wasm32"))]
    fn let_go(&mut self, settings: &mut ReadAloudSettings) {
        let engine = settings.engine.as_str();
        let shelf = &self.shelf;
        let gone = shelf.model(engine).is_some() && !shelf.ready().iter().any(|m| m.id == engine);
        if !gone {
            return;
        }
        if let Some(Session::Spoken(s)) = &self.session
            && s.engine() == engine
        {
            self.stop();
        }
        settings.engine = mareader_speech::SYSTEM.to_owned();
    }

    pub fn active(&self) -> bool {
        self.session.is_some()
    }

    /// Pauses the book and silences samples and card words.
    pub fn hush(&mut self) {
        self.pause();
        self.booth.stop_trying();
        self.stop_word();
    }

    /// Plays or pauses; false when nothing is reading.
    pub fn toggle(&mut self) -> bool {
        match self.session.as_mut() {
            Some(Session::Narrated(s)) => s.toggle(),
            Some(Session::Spoken(s)) => s.toggle(),
            None => return false,
        }
        true
    }

    /// True while reading, not paused or done.
    pub fn playing(&self) -> bool {
        match self.session.as_ref() {
            Some(Session::Narrated(s)) => !s.paused && !s.finished,
            Some(Session::Spoken(s)) => s.playing(),
            None => false,
        }
    }

    /// Pauses the reading, as another sound starts.
    pub fn pause(&mut self) {
        match self.session.as_mut() {
            Some(Session::Narrated(s)) if !s.paused && !s.finished => {
                s.player.pause();
                s.paused = true;
            }
            Some(Session::Spoken(s)) => s.pause(),
            _ => {}
        }
    }

    /// Reads from `from`, `at` or the page: narration, else a chosen voice.
    pub fn start(
        &mut self,
        ctx: &Context,
        reading: &Reading,
        asked: ReadFrom,
        settings: &ReadAloudSettings,
    ) {
        let (from, at, until) = (asked.anchor, asked.at, asked.until);
        // What reads is in this pane now, whatever read before.
        self.away = false;
        if self.home() != Some(reading.pane) {
            self.origin = None;
        }
        // A selection is read just as it is, so by the voice.
        if reading.document.narration().is_none() || until.is_some() {
            let start = match (from, at) {
                (Some(anchor), _) => spoken::Start::Anchor(anchor),
                (None, Some(spot)) => spoken::Start::Spot(spot),
                (None, None) => spoken::Start::Spot(Spot {
                    page: reading.current,
                    char: 0,
                }),
            };
            if let Some(Session::Spoken(s)) = self.session.as_mut()
                && s.reads(reading)
            {
                s.restart(reading, (start, until), (settings, &self.shelf));
                return;
            }
            let on = (settings, &self.shelf);
            let spoken = Spoken::new(ctx, reading, (start, until), on);
            self.session = Some(Session::Spoken(Box::new(spoken)));
            return;
        }
        let Some(narration) = reading.document.narration() else {
            return;
        };
        let skip = skips(settings);
        let from = from.or_else(|| reading.document.anchor(reading.current));
        let first = from.and_then(|a| narration.from_anchor(a, skip));
        let Some(first) = first.or_else(|| narration.from_anchor(Anchor::default(), skip)) else {
            return;
        };
        let same = match self.session.as_ref() {
            Some(Session::Narrated(s)) => Arc::ptr_eq(&s.narration, &narration),
            _ => false,
        };
        if !same {
            let Some(files) = reading.document.files() else {
                return;
            };
            self.session = Some(Session::Narrated(Narrated {
                pane: reading.pane,
                lengths: lengths(&narration),
                narration,
                document: Arc::clone(&reading.document),
                jump: false,
                player: Player::new(fetch(files)),
                runs: Vec::new(),
                heard: None,
                started: first,
                followed: None,
                skip,
                rate: settings.rate,
                paused: false,
                finished: false,
                fault: None,
            }));
        }
        if let Some(Session::Narrated(s)) = self.session.as_mut() {
            s.pane = reading.pane;
            s.skip = skip;
            s.rate = settings.rate;
            s.play_from(first);
        }
    }

    pub fn stop(&mut self) {
        self.session = None;
    }

    /// Each frame: follows what is heard; away from its pane, reads on unseen.
    pub fn tick(&mut self, ctx: &Context, documents: &mut Documents, settings: &ReadAloudSettings) {
        self.tick_word(ctx);
        let Some(session) = self.session.as_mut() else {
            self.origin = None;
            return;
        };
        let here = documents.reading(session.pane());
        let here = here.filter(|r| session.ours(r));
        self.away = here.is_none();
        let here = here.as_ref();
        let going = match session {
            Session::Narrated(s) => s.tick(ctx, documents, here, settings),
            Session::Spoken(s) => s.tick(ctx, documents, here, (settings, &self.shelf)),
        };
        if !going {
            self.session = None;
        }
    }

    /// The book read while its pane shows another, or none.
    pub fn away(&self) -> Option<&Origin> {
        self.origin
            .as_ref()
            .filter(|_| self.away && self.session.is_some())
    }

    /// The book read, while it reads.
    pub fn origin(&self) -> Option<&Origin> {
        self.origin.as_ref().filter(|_| self.session.is_some())
    }

    /// Notes what the book read is, once for each pane it shows in.
    pub fn remember(&mut self, origin: impl FnOnce(PaneId) -> Origin) {
        let Some(pane) = self.home() else {
            return;
        };
        if self.origin.is_none() || self.origin_at != Some(pane) {
            self.origin = Some(origin(pane));
            self.origin_at = Some(pane);
        }
    }

    /// Reads on in `pane`, the book read opened again there.
    pub fn rehome(&mut self, documents: &Documents, pane: PaneId) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        let Some(reading) = documents.reading(pane) else {
            return false;
        };
        let homed = session.rehome(&reading);
        self.away &= !homed;
        homed
    }

    /// The page read and the characters lit, seen or not.
    pub fn reading_spot(&self) -> Option<(usize, Option<std::ops::Range<usize>>)> {
        self.session.as_ref()?.spot()
    }

    /// The pane reading, while it shows the book read.
    pub fn home(&self) -> Option<PaneId> {
        let s = self.session.as_ref().filter(|_| !self.away)?;
        Some(s.pane())
    }

    /// Says `word` in the engine and voice books are read in.
    pub fn say_word(
        &mut self,
        ctx: &Context,
        settings: &ReadAloudSettings,
        word: &str,
    ) -> Result<(), String> {
        self.word.open(ctx, &settings.engine, settings, &self.shelf);
        self.word.say(settings, word);
        self.said_at = ctx.input(|i| i.time);
        if let Some(why) = self.word.fault() {
            self.saying = None;
            return Err(format!("Couldn't say “{word}”: {why}"));
        }
        self.saying = self.word.trying().then(|| word.to_owned());
        Ok(())
    }

    pub fn stop_word(&mut self) {
        self.word.stop_trying();
        self.saying = None;
    }

    /// The card word being said, if any.
    pub fn saying(&self) -> Option<&str> {
        self.saying.as_deref()
    }

    /// Words said from cards since the window opened.
    pub fn words_said(&self) -> u64 {
        self.word.samples()
    }

    /// Hears the word end; lets the engine go a while after.
    fn tick_word(&mut self, ctx: &Context) {
        if !self.word.is_open() {
            return;
        }
        self.word.tick(ctx);
        if !self.word.trying() {
            self.saying = None;
        }
        let idle = ctx.input(|i| i.time) - self.said_at;
        if self.saying.is_none() && idle > WORD_IDLE {
            self.word.close();
        }
    }

    /// The pane read in, and where in its book the words heard start.
    pub fn said(&self) -> Option<(PaneId, Anchor)> {
        self.home()?;
        match self.session.as_ref()? {
            Session::Narrated(s) => {
                let phrase = s.narration.phrases.get(s.heard?)?;
                Some((s.pane, phrase.start))
            }
            Session::Spoken(s) => Some((s.pane, s.said()?)),
        }
    }

    /// The words to light in `pane`, if it is reading.
    pub fn lit(&self, pane: PaneId) -> Option<Lit> {
        match self
            .session
            .as_ref()
            .filter(|_| self.home() == Some(pane))?
        {
            Session::Narrated(s) => s.lit(),
            Session::Spoken(s) => s.lit(),
        }
    }

    /// The pane read in, the page, and the characters lit on it.
    pub fn spot(&self) -> Option<(PaneId, usize, Option<std::ops::Range<usize>>)> {
        let pane = self.home()?;
        let (page, chars) = self.session.as_ref()?.spot()?;
        Some((pane, page, chars))
    }

    /// Whether the player is a card, or on its way to one.
    pub fn carded(&self) -> bool {
        self.session.is_some() && self.shape.card
    }

    /// The player: a pill by its pane, or a card when the words are away.
    pub fn bar(
        &mut self,
        ctx: &Context,
        stage: Stage<'_>,
        settings: &mut ReadAloudSettings,
        out: &mut Commands,
        cover: &mut dyn FnMut(&Ui, Rect) -> bool,
    ) {
        let home = self.home();
        let Some(s) = self.session.as_mut() else {
            self.shape = Shape::default();
            return;
        };
        let shape = &mut self.shape;
        let now = ctx.input(|i| i.time);
        let pane = (stage.panes.iter())
            .find(|(p, _)| stage.reader && Some(*p) == home)
            .map(|(_, r)| *r);
        match stage.shown {
            Some(false) => _ = *shape.hidden_since.get_or_insert(now),
            _ => shape.hidden_since = None,
        }
        // The page shows the words unless they are elsewhere or drift.
        let drifted = shape.hidden_since.is_some_and(|t| now - t >= HIDDEN);
        let auto = pane.is_none() || !settings.follow || drifted;
        if auto != shape.auto {
            shape.auto = auto;
            shape.chosen = None;
        }
        let room = pane.unwrap_or(stage.content);
        let fits = room.height() >= card::HEIGHT + 120.0 && room.width() >= 300.0;
        shape.card = shape.chosen.unwrap_or(auto) && fits;
        let morph = motion::secs(ctx, MORPH);
        let k = ctx.animate_bool_with_time(Id::new("read-aloud-card"), shape.card, morph);
        let k = ease_in_out(k);
        let since = now - *shape.appeared.get_or_insert(now);
        let shown = match motion::enabled(ctx) {
            true => ease_out((since / APPEAR).clamp(0.0, 1.0)) as f32,
            false => 1.0,
        };
        if shown < 1.0 {
            ctx.request_repaint();
        }
        // It sits over the page bar in a pane, else near the window's foot.
        let foot = match pane.is_some() || stage.reader {
            true => room.bottom() - crate::page_bar::BAR_H - 12.0,
            false => room.bottom() - 20.0,
        };
        let goal = egui::pos2(room.center().x, foot);
        let dt = shape.time.map_or(0.0, |t| (now - t) as f32).min(0.1);
        shape.time = Some(now);
        let at = match shape.at {
            Some(at) if motion::enabled(ctx) => at + (goal - at) * (1.0 - (-dt * GLIDE).exp()),
            _ => goal,
        };
        shape.at = Some(at);
        if (goal - at).length() > 0.5 {
            ctx.request_repaint();
        }
        let act = {
            let hint = |share: f32| s.hint(share);
            let view = s.view(&hint);
            let about = s.about(stage.title);
            let lyrics = s.lyrics();
            let pill = Fit::new(room.width() - 24.0, view.escape).width(view.escape);
            let width = card::lerp(pill, card::WIDTH, k)
                .min(room.width() - 16.0)
                .max(0.0);
            let height = card::lerp(control::HEIGHT, card::HEIGHT, k);
            let bottom = at.y + (1.0 - shown) * 10.0;
            let rect = Rect::from_min_size(
                egui::pos2(at.x - width / 2.0, bottom - height),
                egui::vec2(width, height),
            );
            let fit = Fit::new(width, view.escape);
            let mut act = None;
            egui::Area::new(Id::new("read-aloud-bar"))
                .fade_in(false)
                .order(Order::Foreground)
                .fixed_pos(rect.min)
                .constrain(false)
                .show(ctx, |ui| {
                    ui.set_opacity(shown);
                    ui.set_clip_rect(room);
                    let (r, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
                    card::frame(ui, r, k);
                    let row = r.with_min_y(r.bottom() - control::HEIGHT);
                    // The words wait until the card has room for them.
                    let words = ((k - 0.35) / 0.65).clamp(0.0, 1.0);
                    if words > 0.0 {
                        let top = r.with_max_y(row.top());
                        let both = (&about, lyrics.as_ref());
                        if card::show(
                            ui,
                            top,
                            ease_out(f64::from(words)) as f32,
                            both,
                            &mut shape.scroll,
                            cover,
                        ) {
                            act = Some(Act::Reveal);
                        }
                    }
                    if let Some(a) = control::show(ui, row, (&view, fit, k), settings, out) {
                        act = Some(a);
                    }
                });
            act
        };
        match act {
            Some(Act::Card(to)) => shape.chosen = Some(to),
            Some(Act::Reveal) => out.push(Command::ShowReading),
            Some(act) if s.act(act) => self.session = None,
            _ => {}
        }
    }
}

/// Slow, quick, then slow: for things changing shape.
fn ease_in_out(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// Fast at first, then settling: for things arriving.
fn ease_out(k: f64) -> f64 {
    1.0 - (1.0 - k).powi(3)
}

/// Seconds of sound before each phrase, ending with the whole.
fn lengths(narration: &Narration) -> Vec<f64> {
    let known: Vec<f64> = (narration.phrases.iter())
        .filter_map(|p| Some(p.audio.end? - p.audio.begin))
        .collect();
    // A phrase played to its file's end counts as the usual length.
    let usual = match known.len() {
        0 => 3.0,
        n => known.iter().sum::<f64>() / n as f64,
    };
    let mut out = Vec::with_capacity(narration.phrases.len() + 1);
    let mut sum = 0.0;
    out.push(sum);
    for p in &narration.phrases {
        sum += p
            .audio
            .end
            .map_or(usual, |end| end - p.audio.begin)
            .max(0.0);
        out.push(sum);
    }
    out
}

/// "Page 12 · Chapter Three": where a page is, for the seek track.
fn place_name(document: &Document, page: usize) -> String {
    let outline = document.outline();
    let chapter = mareader_pdf::active_entry(outline, page).and_then(|i| outline.get(i));
    match chapter {
        Some(c) if !c.title.trim().is_empty() => format!("Page {} · {}", page + 1, c.title.trim()),
        _ => format!("Page {}", page + 1),
    }
}

/// Turns `reading`'s pane to `page` when it is new and following is on.
fn follow(
    documents: &mut Documents,
    reading: &Reading,
    settings: &ReadAloudSettings,
    page: Option<usize>,
    followed: &mut Option<usize>,
) {
    if settings.follow && page.is_some() && page != *followed {
        *followed = page;
        if let Some(page) = page.filter(|p| *p != reading.current) {
            documents.go_to(reading.pane, page);
        }
    }
}

/// The speaker on a word's card: says it, or stops it; accented meanwhile.
pub fn say_button(ui: &mut Ui, word: &str, saying: bool, out: &mut Commands) {
    // No voice here speaks Burmese and its kin.
    if crate::shaped::needs_shaping(word) {
        return;
    }
    let (label, command) = match saying {
        true => (STOP_SAYING, Command::StopSaying),
        false => (SAY, Command::SayWord(word.to_owned())),
    };
    let accent = Tokens::get(ui.ctx()).accent;
    let r = ui.scope(|ui| {
        if saying {
            ui.visuals_mut().override_text_color = Some(accent);
        }
        widgets::small_icon_button(ui, Icon::Speak, label)
    });
    if r.inner.clicked() {
        out.push(command);
    }
}

pub(crate) fn rate_label(rate: f32) -> String {
    let text = format!("{rate:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}×")
}

fn fault_text(fault: &Fault) -> String {
    match fault {
        Fault::NoDevice => "No sound output".to_owned(),
        Fault::Unreadable(_) => "A sound file won't play".to_owned(),
    }
}

/// What `settings` leaves out.
fn skips(settings: &ReadAloudSettings) -> Skips {
    Skips {
        notes: settings.skip_notes,
        page_numbers: settings.skip_page_numbers,
    }
}

/// Reads sound from `files`, keeping the last file for the next run.
pub(crate) fn fetch(files: Files) -> Fetch {
    let last: Mutex<Option<(String, Clip)>> = Mutex::new(None);
    Arc::new(move |path: &str| {
        let mut last = last.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, clip)) = last.as_ref().filter(|(p, _)| p == path) {
            return Some(clip.clone());
        }
        let media = files.media(path)?;
        let clip = Clip::File {
            data: Arc::new(media.data),
            extension: media.extension,
        };
        *last = Some((path.to_owned(), clip.clone()));
        Some(clip)
    })
}

/// The words read on the pages shown, in the book's colour.
pub fn paint(ui: &Ui, pages: &Pages<'_>, lit: &Lit, t: &Tokens) {
    let fill = fill(lit.fill, t);
    let painter = ui.painter();
    for sheet in pages.sheets.iter().filter(|s| s.shown.is_positive()) {
        let Some(layer) = pages.texts.get(pages.doc, sheet.page) else {
            continue;
        };
        let mut runs = Vec::new();
        match lit.span {
            Span::Anchors { start, end } => {
                let inside = |at: Option<(u32, u32)>| {
                    at.is_some_and(|(b, i)| {
                        let at = Anchor {
                            block: b as usize,
                            offset: i as usize,
                        };
                        start <= at && at < end
                    })
                };
                let mut from = None;
                let count = layer.places.len();
                for i in 0..=count {
                    let on = i < count && inside(layer.places.get(i).copied().flatten());
                    match (on, from) {
                        (true, None) => from = Some(i),
                        (false, Some(f)) => {
                            runs.push(f..i);
                            from = None;
                        }
                        _ => {}
                    }
                }
            }
            Span::Chars { start, end } => {
                let page = sheet.page;
                if (start.page..=end.page).contains(&page) {
                    let from = if page == start.page { start.char } else { 0 };
                    let to = if page == end.page {
                        end.char
                    } else {
                        layer.text.len()
                    };
                    runs.push(from..to.min(layer.text.len()));
                }
            }
        }
        for run in runs.into_iter().filter(|r| !r.is_empty()) {
            for b in layer.text.line_boxes(run) {
                let r = sheet.to_screen(b).intersect(sheet.shown);
                if r.is_positive() {
                    painter.rect_filled(r, CornerRadius::same(2), fill);
                }
            }
        }
    }
}

/// The book's highlight, else the accent, toned to the page's paper.
fn fill(book: Option<[u8; 3]>, t: &Tokens) -> Color32 {
    let paper = t.book.paper.map(|c| c.clamp(0.0, 1.0) * 255.0);
    let light = paper.iter().sum::<f32>() > 3.0 * 128.0;
    let accent = [t.accent.r(), t.accent.g(), t.accent.b()];
    // A light page takes the book's colour as is; a dark one a tint.
    let (colour, share) = match (book, light) {
        (Some(c), true) => (c, 1.0),
        (Some(c), false) => (c, 0.22),
        (None, true) => (accent, 0.30),
        (None, false) => (accent, 0.26),
    };
    let target = std::array::from_fn(|i| {
        let p = paper.get(i).copied().unwrap_or(255.0);
        p + (f32::from(colour.get(i).copied().unwrap_or(255)) - p) * share
    });
    over(paper, target)
}

/// The darkest fill that turns `paper` into `target`, so ink stays dark.
fn over(paper: [f32; 3], target: [f32; 3]) -> Color32 {
    let pairs = || paper.into_iter().zip(target);
    let darker = |(p, t): (f32, f32)| (p - t).max(0.0) / p.max(1.0);
    let lighter = |(p, t): (f32, f32)| (t - p).max(0.0) / (255.0 - p).max(1.0);
    // Darkening sets the strength; lightening only when nothing darkens.
    let dark = pairs().map(darker).fold(0.0_f32, f32::max);
    let need = match dark > 0.01 {
        true => dark,
        false => pairs().map(lighter).fold(0.0_f32, f32::max),
    };
    let alpha = need.clamp(0.12, 1.0);
    let channel = |(p, t): (f32, f32)| ((t - (1.0 - alpha) * p) / alpha).clamp(0.0, 255.0);
    let [r, g, b] = {
        let mut out = [0u8; 3];
        for (slot, pair) in out.iter_mut().zip(pairs()) {
            *slot = channel(pair).round() as u8;
        }
        out
    };
    Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_and_speech_name_the_system_voice_alike() {
        // Settings can't see speech, so each spells the id itself.
        assert_eq!(ReadAloudSettings::SYSTEM, mareader_speech::SYSTEM);
    }

    /// What `fill` over `under` shows, as egui blends it.
    fn shown(fill: Color32, under: f32) -> [f32; 3] {
        let a = f32::from(fill.a()) / 255.0;
        let c = [fill.r(), fill.g(), fill.b()];
        c.map(|c| f32::from(c) + under * (1.0 - a))
    }

    #[test]
    fn the_fill_turns_paper_to_the_book_colour_and_keeps_ink_dark() {
        let fill = over([250.0; 3], [255.0, 255.0, 187.0]);
        let paper = shown(fill, 250.0);
        assert!(
            (paper[2] - 187.0).abs() < 2.0 && paper[0] > 248.0,
            "{paper:?}"
        );
        let ink = shown(fill, 0.0);
        assert!(ink.iter().all(|c| *c < 90.0), "{ink:?}");
        // On a dark page the tint lightens, and is faint.
        let fill = over([30.0; 3], [60.0, 60.0, 40.0]);
        assert!(fill.a() < 64, "{fill:?}");
        assert!((shown(fill, 30.0)[0] - 60.0).abs() < 2.0);
    }

    #[test]
    fn speeds_read_short() {
        assert_eq!(rate_label(1.0), "1×");
        assert_eq!(rate_label(1.25), "1.25×");
        assert_eq!(rate_label(0.75), "0.75×");
        assert_eq!(rate_label(1.5), "1.5×");
    }
}
