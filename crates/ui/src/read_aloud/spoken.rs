//! Unnarrated books, voiced sentence by sentence; a thread reads pages ahead.

use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

use egui::Context;
use mareader_session::{Anchor, DocId, Document, TextLayer};
use mareader_settings::{Highlight, ReadAloudSettings};
use mareader_speech::{Event, Glyph, Heard, Script, Sentence, Shelf, Spot, Tone, Voicer, Waker};
use mareader_workspace::PaneId;

use super::card::Lyrics;
use super::control::{Act, View};
use super::words::{self, Clock, Pace};
use super::{Lit, Span, TICK, follow, place_name};
use crate::documents::{Documents, Reading};

/// Sentences read ahead of the voice; the thread waits past this.
const AHEAD: usize = 8;
/// Sentences kept behind the voice, for stepping back.
const BEHIND: usize = 200;
/// Sentences after the current handed to the voice: at least this many,
const LOOK: usize = 3;
/// then on to about twenty seconds of speech, in characters,
const LOOK_CHARS: usize = 300;
/// but never more than this many.
const LOOK_MOST: usize = 8;

/// Characters on each page the thread has read, for time left.
type Counts = Arc<Mutex<BTreeMap<usize, usize>>>;

/// Where reading starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Start {
    Anchor(Anchor),
    Spot(Spot),
}

/// A sentence off the pages, and its place in the book, if it has one.
#[derive(Clone, Debug)]
struct Said {
    sentence: Sentence,
    anchor: Option<Anchor>,
}

/// One book being said in one pane.
pub(super) struct Spoken {
    pub pane: PaneId,
    doc: DocId,
    document: Arc<Document>,
    voicer: Option<Box<dyn Voicer>>,
    wake: Waker,
    feed: Option<Receiver<Said>>,
    behind: VecDeque<Said>,
    /// The sentence said now, or next once the voice is free.
    current: Option<Said>,
    /// Sentences after the current: stepped back over, or read ahead.
    ahead: VecDeque<Said>,
    /// The voice's id for `current`, and whether it is saying it.
    id: u64,
    saying: bool,
    paused: bool,
    finished: bool,
    fault: Option<String>,
    engine: String,
    /// The model build opened, for a model's engine.
    build: Option<&'static str>,
    voice: Option<String>,
    tone: Tone,
    /// The model's own settings, as last given to it.
    knobs: std::collections::BTreeMap<String, f32>,
    skip_folios: bool,
    followed: Option<usize>,
    /// The sentence last brought into view.
    revealed: Option<Spot>,
    /// Set anew: the sentence said is being found on the new pages.
    relaid: bool,
    /// A selection read alone: where it began, and where it stops.
    chosen: Option<(Start, Spot)>,
    /// How much is lit, and words at a time.
    highlight: (Highlight, u8),
    /// Where the voice says it is in the sentence said.
    heard: Option<Heard>,
    /// The voice has said where it is: till it does again, it is starting.
    timed: bool,
    clock: Clock,
    pace: Pace,
    counts: Counts,
    /// Where the first sentence since reading began starts.
    began: Option<Spot>,
    /// Sought: the pane turns to this page, following or not.
    jump: Option<usize>,
    /// Just found again in a pane, which should turn to the words read.
    homing: bool,
}

impl Spoken {
    pub fn new(
        ctx: &Context,
        reading: &Reading,
        (start, until): (Start, Option<Spot>),
        (settings, shelf): (&ReadAloudSettings, &Shelf),
    ) -> Self {
        let ctx = ctx.clone();
        let wake: Waker = Arc::new(move || ctx.request_repaint());
        let mut spoken = Self {
            pane: reading.pane,
            doc: reading.doc,
            document: Arc::clone(&reading.document),
            voicer: None,
            wake,
            feed: None,
            behind: VecDeque::new(),
            current: None,
            ahead: VecDeque::new(),
            id: 0,
            saying: false,
            paused: false,
            finished: false,
            fault: None,
            engine: String::new(),
            build: None,
            voice: None,
            tone: Tone::default(),
            knobs: std::collections::BTreeMap::new(),
            skip_folios: settings.skip_page_numbers,
            followed: None,
            revealed: None,
            relaid: false,
            chosen: until.map(|until| (start, until)),
            highlight: (settings.highlight, settings.highlight_words),
            heard: None,
            timed: false,
            clock: Clock::default(),
            pace: Pace::default(),
            counts: Counts::default(),
            began: None,
            jump: None,
            homing: false,
        };
        spoken.open(settings, shelf);
        spoken.read_from(start);
        spoken
    }

    /// True when it reads `reading`'s book in its pane.
    pub fn reads(&self, reading: &Reading) -> bool {
        self.pane == reading.pane && self.doc == reading.doc
    }

    /// Reads again from `start`, as asked from the page's menu.
    pub fn restart(
        &mut self,
        reading: &Reading,
        (start, until): (Start, Option<Spot>),
        on: (&ReadAloudSettings, &Shelf),
    ) {
        self.document = Arc::clone(&reading.document);
        self.chosen = until.map(|until| (start, until));
        self.apply(on);
        self.paused = false;
        self.read_from(start);
    }

    /// Where the sentence said starts in the book, while it is placed.
    pub fn said(&self) -> Option<Anchor> {
        self.current.as_ref().filter(|_| !self.relaid)?.anchor
    }

    /// The engine read with.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn engine(&self) -> &str {
        &self.engine
    }

    /// Starts the engine settings name, keeping its fault if it fails.
    fn open(&mut self, settings: &ReadAloudSettings, shelf: &Shelf) {
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.engine = settings.engine.clone();
        self.build = mareader_speech::build_of(&self.engine, settings.build(), shelf);
        self.voice = settings.voice().map(str::to_owned);
        self.tone = tone(settings);
        self.knobs = settings.knobs();
        let wake = Arc::clone(&self.wake);
        match mareader_speech::open(&self.engine, self.build, shelf, wake) {
            Ok(mut voicer) => {
                voicer.set_voice(self.voice.as_deref());
                voicer.set_tone(self.tone);
                voicer.set_knobs(&self.knobs);
                self.voicer = Some(voicer);
                self.fault = None;
            }
            Err(fault) => {
                self.voicer = None;
                self.fault = Some(fault.to_string());
            }
        }
        self.saying = false;
    }

    /// Drops what was read and reads the pages afresh from `start`.
    fn read_from(&mut self, start: Start) {
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.saying = false;
        self.finished = false;
        self.followed = None;
        self.revealed = None;
        self.relaid = false;
        self.behind.clear();
        self.ahead.clear();
        self.current = None;
        self.began = None;
        // No voice, nothing worth reading for.
        let until = self.chosen.map(|(_, until)| until);
        self.feed = self.voicer.as_ref().map(|_| {
            let document = Arc::clone(&self.document);
            let span = Reach { start, until };
            let to = (Arc::clone(&self.wake), Arc::clone(&self.counts));
            spawn(document, span, self.skip_folios, to)
        });
    }

    /// Takes up changed settings; a changed voice says the sentence again.
    fn apply(&mut self, (settings, shelf): (&ReadAloudSettings, &Shelf)) {
        self.highlight = (settings.highlight, settings.highlight_words);
        let build = mareader_speech::build_of(&settings.engine, settings.build(), shelf);
        if settings.engine != self.engine || build != self.build {
            let at = self.place();
            self.open(settings, shelf);
            if let Some(at) = at {
                self.read_from(at);
            }
            return;
        }
        let voice = settings.voice().map(str::to_owned);
        let tone = tone(settings);
        let Some(v) = self.voicer.as_mut() else {
            return;
        };
        let mut again = false;
        if voice != self.voice {
            v.set_voice(voice.as_deref());
            self.voice = voice;
            again = true;
        }
        if tone != self.tone {
            v.set_tone(tone);
            self.tone = tone;
            again = true;
        }
        let knobs = settings.knobs();
        if knobs != self.knobs {
            v.set_knobs(&knobs);
            self.knobs = knobs;
            again = true;
        }
        // Paused mid-sentence, it is said anew in the new voice on play.
        if again && self.saying && self.paused {
            v.stop();
            self.saying = false;
        } else if again && self.saying {
            self.say();
        }
        if settings.skip_page_numbers != self.skip_folios {
            self.skip_folios = settings.skip_page_numbers;
            let at = self.current.as_ref().map(|s| Start::Spot(s.sentence.start));
            if let Some(at) = at {
                self.read_from(at);
            }
        }
    }

    /// Where the sentence said now starts, by place where it has one.
    fn place(&self) -> Option<Start> {
        let said = self.current.as_ref()?;
        Some(match said.anchor {
            Some(anchor) => Start::Anchor(anchor),
            None => Start::Spot(said.sentence.start),
        })
    }

    /// Hands the current sentence to the voice.
    fn say(&mut self) {
        let (Some(v), Some(said)) = (self.voicer.as_mut(), self.current.as_ref()) else {
            return;
        };
        self.id = self.id.wrapping_add(1);
        self.heard = None;
        match v.say(self.id, &said.sentence.text) {
            Ok(()) => {
                self.saying = true;
                self.clock.start();
            }
            Err(fault) => {
                self.fault = Some(fault.to_string());
                self.saying = false;
                self.paused = true;
            }
        }
    }

    /// The next sentence; none while the thread reads, the end once done.
    fn next(&mut self) -> Option<Said> {
        self.ahead.pop_front().or_else(|| self.pull())
    }

    /// A new sentence off the pages, if the thread has one.
    fn pull(&mut self) -> Option<Said> {
        match self.feed.as_ref()?.try_recv() {
            Ok(said) => Some(said),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.feed = None;
                None
            }
        }
    }

    /// Hands the voice the sentences after the current, to make ahead.
    fn look_ahead(&mut self) {
        let texts = |ahead: &VecDeque<Said>| enough(ahead.iter().map(|s| s.sentence.text.as_str()));
        let mut wanted = texts(&self.ahead);
        while wanted.is_none()
            && let Some(said) = self.pull()
        {
            self.ahead.push_back(said);
            wanted = texts(&self.ahead);
        }
        let count = wanted.unwrap_or(self.ahead.len());
        let texts: Vec<&str> = (self.ahead.iter().take(count))
            .map(|s| s.sentence.text.as_str())
            .collect();
        if let Some(v) = self.voicer.as_mut() {
            v.prepare(&texts);
        }
    }

    /// Moves past the current sentence, keeping it to step back to.
    fn pass(&mut self) {
        if let Some(said) = self.current.take() {
            self.behind.push_back(said);
            if self.behind.len() > BEHIND {
                self.behind.pop_front();
            }
        }
    }

    /// Says the current sentence, fetching it first if there is none.
    fn go_on(&mut self) {
        if self.current.is_none() {
            self.current = self.next();
        }
        match (self.current.is_some(), self.feed.is_some()) {
            (true, _) if !self.paused => self.say(),
            (true, _) => {}
            (false, true) => {}
            (false, false) => self.finished = true,
        }
    }

    pub fn playing(&self) -> bool {
        !self.paused && !self.finished && self.voicer.is_some()
    }

    pub fn pause(&mut self) {
        if self.paused || self.finished {
            return;
        }
        if let Some(v) = self.voicer.as_mut()
            && !v.pause()
        {
            v.stop();
            self.saying = false;
        }
        self.paused = true;
        self.clock.stop();
    }

    pub fn toggle(&mut self) {
        if self.finished {
            // Again from the selection's start, else the book's.
            self.paused = false;
            let start = self.chosen.map(|(start, _)| start);
            self.read_from(start.unwrap_or(Start::Spot(Spot::default())));
        } else if self.paused {
            self.paused = false;
            let resumed = self.saying && self.voicer.as_mut().is_some_and(|v| v.resume());
            match resumed {
                true => self.clock.go(),
                false => self.go_on(),
            }
        } else {
            self.pause();
        }
    }

    fn back(&mut self) {
        let Some(said) = self.behind.pop_back() else {
            return;
        };
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.saying = false;
        self.finished = false;
        if let Some(current) = self.current.replace(said) {
            self.ahead.push_front(current);
        }
        self.go_on();
    }

    fn forward(&mut self) {
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.saying = false;
        self.pass();
        self.go_on();
    }

    /// Whether `reading` shows this book, perhaps set in new type.
    pub fn ours(&self, reading: &Reading) -> bool {
        let same = reading.doc == self.doc || reading.document.same_book(&self.document);
        reading.document.narration().is_none() && same
    }

    /// Reads on in `reading`, the book opened again; false if it differs.
    pub fn rehome(&mut self, reading: &Reading) -> bool {
        if reading.document.narration().is_some() {
            return false;
        }
        let fixed = self.document.layout().is_none();
        if fixed && reading.document.page_count() != self.document.page_count() {
            return false;
        }
        self.pane = reading.pane;
        (self.followed, self.revealed) = (None, None);
        self.homing = true;
        // Fixed pages are the same pages; a laid-out book is found anew.
        if fixed {
            self.doc = reading.doc;
            self.document = Arc::clone(&reading.document);
        }
        true
    }

    /// Each frame: hears the voice, says on, turns `here`; false once done.
    pub fn tick(
        &mut self,
        ctx: &Context,
        documents: &mut Documents,
        here: Option<&Reading>,
        (settings, shelf): (&ReadAloudSettings, &Shelf),
    ) -> bool {
        // Set in new type or opened again, the place is found anew.
        if let Some(reading) = here {
            self.doc = reading.doc;
            if !Arc::ptr_eq(&reading.document, &self.document) {
                self.document = Arc::clone(&reading.document);
                self.relay(reading.current);
            }
        }
        self.apply((settings, shelf));
        let events = self.voicer.as_mut().map(|v| v.events()).unwrap_or_default();
        for event in events {
            if event == Event::Finished(self.id) && self.saying {
                self.saying = false;
                let bytes = self.current.as_ref().map_or(0, |s| s.sentence.text.len());
                self.pace.learn(bytes, self.clock.seconds(), self.tone.rate);
                self.pass();
            }
        }
        let heard = self.voicer.as_ref().and_then(|v| v.heard());
        self.heard = heard.filter(|h| h.id == self.id && self.saying);
        self.timed |= heard.is_some();
        if self.relaid {
            self.relocate();
        }
        if self.relaid {
            // Nothing more is said until the place is found.
            ctx.request_repaint_after(TICK);
            return true;
        }
        if !self.paused && !self.saying && !self.finished {
            self.go_on();
        }
        if !self.finished {
            self.look_ahead();
        }
        if self.began.is_none() {
            self.began = self.current.as_ref().map(|s| s.sentence.start);
        }
        // Away from its pane, the book reads on unseen.
        let Some(reading) = here else {
            self.jump = None;
            if !self.paused && !self.finished {
                ctx.request_repaint_after(TICK);
            }
            return true;
        };
        // Back in a pane, it shows the words read at once.
        if std::mem::take(&mut self.homing) {
            self.jump = self.lit_spots().map(|(start, _)| start.page);
        }
        if let Some(page) = self.jump.take() {
            documents.go_to(self.pane, page);
            self.followed = Some(page);
        }
        let lit = self.lit_spots();
        let page = lit.map(|(start, _)| start.page);
        // Only a real turn shows the new page's first sentence.
        let turned = page.is_some() && page != self.followed && page != Some(reading.current);
        follow(documents, reading, settings, page, &mut self.followed);
        self.reveal(documents, reading.pane, settings, turned);
        if !self.paused && !self.finished {
            ctx.request_repaint_after(TICK);
        }
        true
    }

    /// New pages: the voice goes on while its place is found on them.
    fn relay(&mut self, page: usize) {
        // Pages laid anew hold new counts.
        self.counts = Counts::default();
        self.began = None;
        // A selection's end was on the old pages: its reading ends here.
        if self.chosen.is_some() {
            if let Some(v) = self.voicer.as_mut() {
                v.stop();
            }
            (self.saying, self.finished) = (false, true);
            (self.feed, self.current) = (None, None);
            self.ahead.clear();
            return;
        }
        let anchor = self.current.as_ref().and_then(|s| s.anchor);
        let (Some(anchor), Some(_)) = (anchor, self.voicer.as_ref()) else {
            self.read_from(Start::Spot(Spot { page, char: 0 }));
            return;
        };
        // Places behind and ahead are the old pages'; ahead comes again.
        self.behind.clear();
        self.ahead.clear();
        self.followed = None;
        self.revealed = None;
        self.relaid = true;
        let document = Arc::clone(&self.document);
        let to = (Arc::clone(&self.wake), Arc::clone(&self.counts));
        let span = Reach {
            start: Start::Anchor(anchor),
            until: None,
        };
        self.feed = Some(spawn(document, span, self.skip_folios, to));
    }

    /// The new pages' first sentence is the one said: it takes its place.
    fn relocate(&mut self) {
        let Some(found) = self.pull() else {
            // The book ended first: the old place has to do.
            self.relaid = self.feed.is_some();
            return;
        };
        self.relaid = false;
        // Said to its end meanwhile: it is behind, and reading goes on.
        let Some(said) = self.current.as_mut() else {
            return;
        };
        said.sentence.start = found.sentence.start;
        said.sentence.end = found.sentence.end;
        said.sentence.words = found.sentence.words;
        said.anchor = found.anchor;
    }

    /// Keeps the sentence said in view; a turned page shows its first.
    fn reveal(&mut self, docs: &mut Documents, pane: PaneId, s: &ReadAloudSettings, turned: bool) {
        let Some((start, end)) = self.lit_spots() else {
            return;
        };
        if turned {
            self.revealed = Some(start);
        }
        if !s.follow || self.revealed == Some(start) {
            return;
        }
        let last = if end.page == start.page {
            end.char
        } else {
            start.char + 1
        };
        if docs.reveal_said(pane, start.page, start.char..last) {
            self.revealed = Some(start);
        }
    }

    pub fn lit(&self) -> Option<Lit> {
        let (start, end) = self
            .lit_spots()
            .filter(|_| !self.finished && !self.relaid)?;
        Some(Lit {
            span: Span::Chars { start, end },
            fill: None,
        })
    }

    /// Where the words lit start and end: the whole sentence until heard.
    fn lit_spots(&self) -> Option<(Spot, Spot)> {
        let said = &self.current.as_ref()?.sentence;
        let whole = Some((said.start, said.end));
        if self.highlight.0 == Highlight::Sentence || said.words.is_empty() {
            return whole;
        }
        let lit = self.lit_words(self.highlight)?;
        let first = said.words.get(lit.start)?;
        let last = said.words.get(lit.end.checked_sub(1)?)?;
        Some((first.start, last.end))
    }

    /// The words of the sentence said lit as `highlight` asks, by index.
    fn lit_words(&self, highlight: (Highlight, u8)) -> Option<std::ops::Range<usize>> {
        let said = &self.current.as_ref()?.sentence;
        let heard = self.heard.filter(|_| self.saying);
        // The voice's own clock, else ours: either way, pauses left out.
        let seconds = match heard {
            Some(h) => h.seconds,
            None if self.timed => 0.0,
            None => self.clock.seconds(),
        };
        let guess = self.pace.bytes(seconds, self.tone.rate);
        let at = words::heard(&said.words, heard.and_then(|h| h.word), guess);
        Some(words::group(&said.text, &said.words, at, highlight))
    }

    /// The sentences round the one said; phrases lit for page sentences.
    pub fn lyrics(&self) -> Option<Lyrics> {
        let said = &self.current.as_ref().filter(|_| !self.finished)?.sentence;
        let highlight = match self.highlight.0 {
            Highlight::Sentence => (Highlight::Phrases, 1),
            _ => self.highlight,
        };
        let lit = self.lit_words(highlight).unwrap_or_default();
        let start = said.start;
        Some(Lyrics {
            before: self.behind.back().map(|s| s.sentence.text.clone()),
            now: said.text.clone(),
            words: said.words.iter().map(|w| w.bytes.clone()).collect(),
            lit,
            after: self.ahead.front().map(|s| s.sentence.text.clone()),
            key: (start.page as u64) << 32 | start.char as u64,
        })
    }

    /// The page and chapter read, as the card names them.
    pub fn whereabouts(&self) -> Option<String> {
        let (start, _) = self.lit_spots()?;
        Some(place_name(&self.document, start.page))
    }

    /// The pane, the page read, and the characters lit on it.
    pub fn spot(&self) -> Option<(usize, std::ops::Range<usize>)> {
        let (start, end) = self
            .lit_spots()
            .filter(|_| !self.finished && !self.relaid)?;
        let last = if end.page == start.page {
            end.char
        } else {
            start.char + 1
        };
        Some((start.page, start.char..last.max(start.char + 1)))
    }

    /// What the player shows: sentences for steps, pages for time.
    pub fn view<'a>(&self, hint: &'a dyn Fn(f32) -> Option<String>) -> View<'a> {
        let can = self.voicer.is_some();
        let progress = self.progress();
        let note = match (&self.fault, self.finished) {
            (Some(fault), _) => Some((fault.clone(), true)),
            (None, true) if self.chosen.is_some() => Some(("Selection read".to_owned(), false)),
            (None, true) => Some(("The end".to_owned(), false)),
            (None, false) => None,
        };
        View {
            playing: self.playing(),
            ready: can,
            loading: self.voicer.as_ref().is_some_and(|v| v.loading()),
            back: !self.behind.is_empty() && can,
            next: !self.finished && can,
            unit: "sentence",
            escape: false,
            at: progress.map(|p| p.0),
            left: progress.map(|p| p.1),
            total: progress.map(|p| p.2),
            seek: can && self.chosen.is_none(),
            ticks: self.ticks(),
            note,
            hint,
            narrated: false,
        }
    }

    /// How much is read, and seconds left and in all, from pages' lengths.
    fn progress(&self) -> Option<(f32, f64, f64)> {
        let counts = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        let usual = counts.values().sum::<usize>() as f64 / counts.len().max(1) as f64;
        if counts.is_empty() || usual < 1.0 {
            return None;
        }
        // A spot as pages read: its page, and the share of it before.
        let pages = |s: Spot| {
            let on = counts.get(&s.page).map_or(usual, |&n| n as f64).max(1.0);
            s.page as f64 + (s.char as f64 / on).min(1.0)
        };
        let (from, to) = match self.chosen {
            Some((_, until)) => (pages(self.began?), pages(until)),
            None => (0.0, self.document.page_count() as f64),
        };
        let here = match self.finished {
            true => to,
            false => self.lit_spots().map_or(from, |(start, _)| pages(start)),
        };
        let here = here.clamp(from, to.max(from));
        let span = (to - from).max(1e-6);
        let speed = self.pace.bytes(1.0, self.tone.rate).max(1.0);
        let seconds = |pages: f64| pages * usual / speed;
        Some((
            ((here - from) / span) as f32,
            seconds(to - here),
            seconds(span),
        ))
    }

    /// Where the book's top chapters start, as shares of it.
    fn ticks(&self) -> Vec<f32> {
        let outline = self.document.outline();
        let count = self.document.page_count().max(1) as f32;
        let top = outline.iter().map(|e| e.depth).min();
        (outline.iter())
            .filter(|e| Some(e.depth) == top && self.chosen.is_none())
            .map(|e| e.page as f32 / count)
            .collect()
    }

    /// The page at `share` of the book.
    fn page_at(&self, share: f32) -> usize {
        let count = self.document.page_count();
        ((share * count as f32) as usize).min(count.saturating_sub(1))
    }

    pub fn hint(&self, share: f32) -> Option<String> {
        let none = self.chosen.is_some();
        (!none).then(|| place_name(&self.document, self.page_at(share)))
    }

    /// Does what the player asked; true to stop.
    pub fn act(&mut self, act: Act) -> bool {
        match act {
            Act::Back => self.back(),
            Act::Next => self.forward(),
            Act::Toggle => self.toggle(),
            Act::Seek(share) if self.chosen.is_none() && self.voicer.is_some() => {
                let page = self.page_at(share);
                self.read_from(Start::Spot(Spot { page, char: 0 }));
                self.jump = Some(page);
            }
            Act::Seek(_) | Act::Escape | Act::Card(_) | Act::Reveal => {}
            Act::Stop => return true,
        }
        false
    }
}

impl Drop for Spoken {
    fn drop(&mut self) {
        // Some systems speak on after their handle is gone.
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
    }
}

pub(super) fn tone(settings: &ReadAloudSettings) -> Tone {
    Tone {
        rate: settings.rate,
        pitch: settings.pitch,
        volume: settings.volume,
    }
}

/// Where a thread reads from, and where it stops if not at the end.
#[derive(Clone, Copy, Debug)]
struct Reach {
    start: Start,
    until: Option<Spot>,
}

/// How many of `texts` make enough speech to have ready, if they do.
fn enough<'a>(texts: impl Iterator<Item = &'a str>) -> Option<usize> {
    let mut chars = 0;
    for (n, text) in (1..).zip(texts) {
        chars += text.chars().count();
        if n >= LOOK_MOST || (n >= LOOK && chars >= LOOK_CHARS) {
            return Some(n);
        }
    }
    None
}

/// Reads `document` into sentences over `span` on a thread of its own.
fn spawn(
    document: Arc<Document>,
    span: Reach,
    skip_folios: bool,
    (wake, counts): (Waker, Counts),
) -> Receiver<Said> {
    let (tx, rx) = std::sync::mpsc::sync_channel(AHEAD);
    let thread = std::thread::Builder::new().name("read-aloud".into());
    // A thread that won't start sends nothing: the reading just ends.
    let _ = thread.spawn(move || read(&document, span, skip_folios, (&tx, &wake, &counts)));
    rx
}

/// Sends `document`'s sentences until the end, or until no one listens.
fn read(
    document: &Document,
    span: Reach,
    skip_folios: bool,
    (tx, wake, counts): (&SyncSender<Said>, &Waker, &Counts),
) {
    let (first, from) = match span.start {
        Start::Spot(spot) => (spot.page, spot.char),
        Start::Anchor(anchor) => locate(document, anchor),
    };
    let last = span.until.map_or(usize::MAX, |u| u.page);
    let mut script = Script::new(skip_folios);
    // The page before, for a sentence that started on it.
    let mut before: Option<(usize, TextLayer)> = None;
    for page in first..document.page_count().min(last.saturating_add(1)) {
        let Ok(layer) = document.text_layer(page) else {
            continue;
        };
        let mut known = counts.lock().unwrap_or_else(PoisonError::into_inner);
        known.insert(page, layer.text.len());
        drop(known);
        // A selection's last page is read only up to its end.
        let upto = match span.until {
            Some(u) if u.page == page => u.char,
            _ => usize::MAX,
        };
        let chars = layer.text.chars().iter().take(upto);
        let glyphs: Vec<Glyph> = chars
            .map(|c| Glyph {
                ch: c.ch,
                line: c.line,
                space_before: c.space_before,
                wraps: layer.text.wraps(c.line),
            })
            .collect();
        let from = if page == first { from } else { 0 };
        for sentence in script.page(page, &glyphs, from) {
            let start = sentence.start;
            let on = match start.page == page {
                true => Some(&layer),
                false => before
                    .as_ref()
                    .filter(|(p, _)| *p == start.page)
                    .map(|(_, l)| l),
            };
            let anchor = on.and_then(|l| l.place(start.char)).map(anchor);
            if tx.send(Said { sentence, anchor }).is_err() {
                return;
            }
            wake();
        }
        before = Some((page, layer));
    }
    for sentence in script.finish() {
        if tx
            .send(Said {
                sentence,
                anchor: None,
            })
            .is_err()
        {
            return;
        }
    }
    wake();
}

fn anchor((block, offset): (usize, usize)) -> Anchor {
    Anchor { block, offset }
}

/// The page and character where `anchor` is drawn now.
fn locate(document: &Document, at: Anchor) -> (usize, usize) {
    let page = document.locate(at).unwrap_or_default();
    let Ok(layer) = document.text_layer(page) else {
        return (page, 0);
    };
    let count = layer.text.len();
    let char = (0..count).find(|&i| layer.place(i).is_some_and(|p| anchor(p) >= at));
    (page, char.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_twenty_seconds_of_speech_is_made_ahead() {
        let short = ["Yes."; 9];
        assert_eq!(enough(short.into_iter()), Some(LOOK_MOST), "never more");
        let long = "a".repeat(LOOK_CHARS);
        let texts = [long.as_str(), "Yes.", "Yes.", "Yes."];
        assert_eq!(enough(texts.into_iter()), Some(LOOK), "never fewer");
        let some = "a".repeat(LOOK_CHARS / 5);
        assert_eq!(enough([some.as_str(); 9].into_iter()), Some(5));
        assert_eq!(enough(["Yes."; 4].into_iter()), None, "wants more");
    }
}
