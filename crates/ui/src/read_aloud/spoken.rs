//! Unnarrated books, voiced sentence by sentence; a thread reads pages ahead.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};

use egui::{Context, Ui};
use mareader_session::{Anchor, DocId, Document, TextLayer};
use mareader_settings::ReadAloudSettings;
use mareader_speech::{Event, Glyph, Script, Sentence, Shelf, Spot, Tone, Voicer, Waker};
use mareader_workspace::PaneId;

use super::{Lit, Span, TICK, button, ending, follow, options, speed};
use crate::command::Commands;
use crate::documents::{Documents, Reading};
use crate::icons::Icon;
use crate::theme::Tokens;

/// Sentences read ahead of the voice; the thread waits past this.
const AHEAD: usize = 8;
/// Sentences kept behind the voice, for stepping back.
const BEHIND: usize = 200;
/// Sentences after the current, handed to the voice to make ahead.
const LOOK: usize = 3;

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
}

impl Spoken {
    pub fn new(
        ctx: &Context,
        reading: &Reading,
        start: Start,
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
    pub fn restart(&mut self, reading: &Reading, start: Start, on: (&ReadAloudSettings, &Shelf)) {
        self.document = Arc::clone(&reading.document);
        self.apply(on);
        self.paused = false;
        self.read_from(start);
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
        self.behind.clear();
        self.ahead.clear();
        self.current = None;
        // No voice, nothing worth reading for.
        self.feed = self.voicer.as_ref().map(|_| {
            let document = Arc::clone(&self.document);
            spawn(document, start, self.skip_folios, Arc::clone(&self.wake))
        });
    }

    /// Takes up changed settings; a changed voice says the sentence again.
    fn apply(&mut self, (settings, shelf): (&ReadAloudSettings, &Shelf)) {
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
        if again && self.saying {
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
        match v.say(self.id, &said.sentence.text) {
            Ok(()) => self.saying = true,
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
        while self.ahead.len() < LOOK
            && let Some(said) = self.pull()
        {
            self.ahead.push_back(said);
        }
        let texts: Vec<&str> = (self.ahead.iter().take(LOOK))
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
    }

    pub fn toggle(&mut self) {
        if self.finished {
            self.paused = false;
            self.read_from(Start::Spot(Spot::default()));
        } else if self.paused {
            self.paused = false;
            let resumed = self.saying && self.voicer.as_mut().is_some_and(|v| v.resume());
            if !resumed {
                self.go_on();
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

    /// Each frame: hears the voice, says on, turns pages; false on a new book.
    pub fn tick(
        &mut self,
        ctx: &Context,
        documents: &mut Documents,
        (settings, shelf): (&ReadAloudSettings, &Shelf),
    ) -> bool {
        let Some(reading) = documents.reading(self.pane) else {
            return false;
        };
        if reading.doc != self.doc || reading.document.narration().is_some() {
            return false;
        }
        if !Arc::ptr_eq(&reading.document, &self.document) {
            // Laid out again: new pages, so read on from the same place.
            self.document = Arc::clone(&reading.document);
            let page = Start::Spot(Spot {
                page: reading.current,
                char: 0,
            });
            let at = self.place().unwrap_or(page);
            self.read_from(at);
        }
        self.apply((settings, shelf));
        let events = self.voicer.as_mut().map(|v| v.events()).unwrap_or_default();
        for event in events {
            if event == Event::Finished(self.id) && self.saying {
                self.saying = false;
                self.pass();
            }
        }
        if !self.paused && !self.saying && !self.finished {
            self.go_on();
        }
        if !self.finished {
            self.look_ahead();
        }
        let page = self.current.as_ref().map(|s| s.sentence.start.page);
        // Only a real turn shows the new page's first sentence.
        let turned = page.is_some() && page != self.followed && page != Some(reading.current);
        follow(documents, &reading, settings, page, &mut self.followed);
        self.reveal(documents, reading.pane, settings, turned);
        if !self.paused && !self.finished {
            ctx.request_repaint_after(TICK);
        }
        true
    }

    /// Keeps the sentence said in view; a turned page shows its first.
    fn reveal(&mut self, docs: &mut Documents, pane: PaneId, s: &ReadAloudSettings, turned: bool) {
        let Some(said) = self.current.as_ref().map(|c| &c.sentence) else {
            return;
        };
        let (start, end) = (said.start, said.end);
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
        let said = self.current.as_ref().filter(|_| !self.finished)?;
        Some(Lit {
            span: Span::Chars {
                start: said.sentence.start,
                end: said.sentence.end,
            },
            fill: None,
        })
    }

    /// The bar's buttons; true when closed.
    pub fn controls(
        &mut self,
        ui: &mut Ui,
        settings: &mut ReadAloudSettings,
        (t, out): (&Tokens, &mut Commands),
    ) -> bool {
        let can = self.voicer.is_some();
        let back = !self.behind.is_empty() && can;
        if button(ui, Icon::SkipBack, "Previous sentence", back).clicked() {
            self.back();
        }
        let (icon, name) = match self.paused || self.finished {
            true => (Icon::Resume, "Play"),
            false => (Icon::Pause, "Pause"),
        };
        if button(ui, icon, name, can).clicked() {
            self.toggle();
        }
        let next = !self.finished && can;
        if button(ui, Icon::SkipForward, "Next sentence", next).clicked() {
            self.forward();
        }
        crate::widgets::vrule(ui, super::SIZE * 0.5);
        speed(ui, settings, t);
        options(ui, out, |ui| {
            ui.checkbox(&mut settings.skip_page_numbers, "Skip page numbers");
            ui.checkbox(&mut settings.follow, "Turn pages to follow");
        });
        let loading = self.voicer.as_ref().is_some_and(|v| v.loading());
        let note = match (&self.fault, self.finished) {
            (Some(fault), _) => Some((fault.clone(), t.danger)),
            (None, true) => Some(("The end".to_owned(), t.muted)),
            (None, false) if loading => Some(("Loading voice…".to_owned(), t.muted)),
            (None, false) => None,
        };
        ending(ui, note, t)
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

/// Reads `document` into sentences from `start` on a thread of its own.
fn spawn(document: Arc<Document>, start: Start, skip_folios: bool, wake: Waker) -> Receiver<Said> {
    let (tx, rx) = std::sync::mpsc::sync_channel(AHEAD);
    let thread = std::thread::Builder::new().name("read-aloud".into());
    // A thread that won't start sends nothing: the reading just ends.
    let _ = thread.spawn(move || read(&document, start, skip_folios, &tx, &wake));
    rx
}

/// Sends `document`'s sentences until the end, or until no one listens.
fn read(document: &Document, start: Start, skip_folios: bool, tx: &SyncSender<Said>, wake: &Waker) {
    let (first, from) = match start {
        Start::Spot(spot) => (spot.page, spot.char),
        Start::Anchor(anchor) => locate(document, anchor),
    };
    let mut script = Script::new(skip_folios);
    // The page before, for a sentence that started on it.
    let mut before: Option<(usize, TextLayer)> = None;
    for page in first..document.page_count() {
        let Ok(layer) = document.text_layer(page) else {
            continue;
        };
        let chars = layer.text.chars().iter();
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
