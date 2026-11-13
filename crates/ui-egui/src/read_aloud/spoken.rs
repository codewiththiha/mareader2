//! Unnarrated books, voiced sentence by sentence; a thread reads pages ahead.

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, PoisonError};

use egui::Context;
use mareader_engine::read_aloud::words::{self, Clock, Pace, Steady};
use mareader_session::{Anchor, DocId, Document};
use mareader_settings::{Highlight, Pass, ReadAloudSettings, Skips};
use mareader_speech::{Event, Heard, Shelf, Spot, Tone, Voicer, Waker};
use mareader_workspace::PaneId;

use crate::documents::{Documents, Reading};
pub use mareader_engine::read_aloud::feed::{Counts, Reach, Said, Start, enough, spawn};
use mareader_engine::read_aloud::lit::{Lit, Span};
use mareader_engine::read_aloud::parts::{self, Part};
use mareader_engine::read_aloud::place::{Panes, Stand, follow, place_name};
use mareader_engine::read_aloud::player::{Act, Lyrics, View};

/// Sentences kept behind the voice, for stepping back.
const BEHIND: usize = 200;

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
    /// The pace over the whole reading, for the time left.
    steady: Steady,
    counts: Counts,
    /// Where the first sentence since reading began starts.
    began: Option<Spot>,
    /// Sought: the pane turns to this page, following or not.
    jump: Option<usize>,
    /// Freshly found again in a pane, which should turn to the words read.
    homing: bool,
    /// The part being skipped: its sentences are passed unsaid.
    leaving: Option<Part>,
    /// What reading does on reaching each kind of part.
    skips: Skips,
    /// Explicit overrides belonging only to this book.
    pub book_mask: u8,
    /// The parts read on into: they are not asked about again.
    cleared: Vec<Part>,
    /// The part asked about, held at its start for an answer.
    asking: Option<Part>,
    /// Read afresh: the parts it starts in are read unasked.
    entered: bool,
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
            steady: Steady::default(),
            counts: Counts::default(),
            began: None,
            jump: None,
            homing: false,
            leaving: None,
            skips: settings.skips,
            book_mask: 0,
            cleared: Vec::new(),
            asking: None,
            entered: true,
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
        self.tone = mareader_engine::read_aloud::tone(settings);
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
        self.leaving = None;
        self.cleared.clear();
        self.entered = true;
        if self.asking.take().is_some() {
            self.paused = false;
        }
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
        self.skips = settings.skips;
        for kind in parts::Kind::ALL {
            if self.book_mask & kind.bit() != 0 {
                kind.set_skip(&mut self.skips);
            }
        }
        // Asked no more, the question answers itself.
        match self.asking.map(|p| p.kind.pass(&self.skips)) {
            Some(Pass::Read) => self.answer(true),
            Some(Pass::Skip) => self.answer(false),
            _ => {}
        }
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
        let tone = mareader_engine::read_aloud::tone(settings);
        let Some(v) = self.voicer.as_mut() else {
            return;
        };
        let mut again = false;
        if voice != self.voice {
            // Another voice keeps its own pace.
            self.steady = Steady::default();
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
        while let Some(said) = self.ahead.pop_front().or_else(|| self.pull()) {
            if self.leaving.is_some_and(|p| said.parts.contains(&p)) {
                self.keep(said);
                continue;
            }
            self.leaving = None;
            self.cleared.retain(|p| said.parts.contains(p));
            if std::mem::take(&mut self.entered) {
                self.cleared.clone_from(&said.parts);
                return Some(said);
            }
            // Outermost first: a skipped list passes the tables in it.
            let mut held = None;
            for &part in &said.parts {
                if self.cleared.contains(&part) {
                    continue;
                }
                match part.kind.pass(&self.skips) {
                    Pass::Read => self.cleared.push(part),
                    pass => {
                        held = Some((part, pass));
                        break;
                    }
                }
            }
            match held {
                Some((part, Pass::Skip)) => {
                    self.leaving = Some(part);
                    self.keep(said);
                }
                Some((part, _)) => {
                    self.ask(part);
                    return Some(said);
                }
                None => return Some(said),
            }
        }
        None
    }

    /// Holds at a part's start, asking whether to read it.
    fn ask(&mut self, part: Part) {
        self.asking = Some(part);
        self.paused = true;
        self.clock.stop();
    }

    /// Estimate only when the buffered sentences include the part's end.
    pub fn ask_cost(&self, rate: f32) -> Option<String> {
        let part = self.asking?;
        let first = self.current.as_ref()?;
        let mut bytes = first.sentence.text.len();
        let mut ended = false;
        for said in &self.ahead {
            if !said.parts.contains(&part) {
                ended = true;
                break;
            }
            bytes = bytes.saturating_add(said.sentence.text.len());
        }
        if !ended && self.ahead.is_empty() {
            return None;
        }
        let seconds = (bytes as f32 / (14.0 * rate.clamp(0.5, 3.0)))
            .ceil()
            .max(1.0);
        let prefix = if ended { "~" } else { "at least ~" };
        Some(format!("{prefix}{} s", seconds as u32))
    }

    pub fn is_asking(&self) -> bool {
        self.asking.is_some()
    }

    /// The answer to the part asked about: read it, or pass it over.
    fn answer(&mut self, read: bool) {
        let Some(part) = self.asking.take() else {
            return;
        };
        self.paused = false;
        if read {
            self.cleared.push(part);
        } else {
            self.leaving = Some(part);
            self.pass();
        }
        self.go_on();
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
            self.keep(said);
        }
    }

    /// Keeps `said` to step back to, forgetting the oldest.
    fn keep(&mut self, said: Said) {
        self.behind.push_back(said);
        if self.behind.len() > BEHIND {
            self.behind.pop_front();
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
        // Play, asked about a part, reads it.
        if self.asking.is_some() {
            self.answer(true);
        } else if self.finished {
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
        self.leaving = None;
        if self.asking.take().is_some() {
            self.paused = false;
        }
        // Stepped back into, its parts are read unasked.
        self.cleared.clone_from(&said.parts);
        if let Some(current) = self.current.replace(said) {
            self.ahead.push_front(current);
        }
        self.go_on();
    }

    /// Skips the rest of the innermost part read, to the sentence after it.
    fn escape(&mut self) {
        if self.asking.is_some() {
            self.answer(false);
            return;
        }
        let Some(&part) = self.current.as_ref().and_then(|s| s.parts.last()) else {
            return;
        };
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.saying = false;
        self.leaving = Some(part);
        self.pass();
        self.go_on();
    }

    fn forward(&mut self) {
        // Stepped on by hand, the part asked about is read.
        if let Some(part) = self.asking.take() {
            self.cleared.push(part);
            self.paused = false;
        }
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
        let same = reading.document.layout() == self.document.layout()
            && reading.document.page_count() == self.document.page_count();
        if fixed && !same {
            return false;
        }
        self.pane = reading.pane;
        (self.followed, self.revealed) = (None, None);
        self.homing = true;
        // Known by its new doc from now on, else it seems away again.
        self.doc = reading.doc;
        // Pages laid alike are the same; others are found anew in `tick`.
        if same {
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
        (settings, shelf, clear): (&ReadAloudSettings, &Shelf, f32),
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
                self.steady
                    .learn(bytes, self.clock.seconds(), self.tone.rate);
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
            ctx.request_repaint_after(mareader_engine::read_aloud::TICK);
            return true;
        }
        // Paused, a step still fetches the sentence it lands on.
        let fetch = self.current.is_none() && self.feed.is_some();
        if (!self.paused || fetch) && !self.saying && !self.finished {
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
                ctx.request_repaint_after(mareader_engine::read_aloud::TICK);
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
        let stand = reading.stand();
        follow(documents, &stand, settings, page, &mut self.followed);
        self.reveal(documents, (reading.pane, clear), settings, turned);
        if !self.paused && !self.finished {
            ctx.request_repaint_after(mareader_engine::read_aloud::TICK);
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

    /// Keeps the sentence said in view, above the player `clear` high.
    fn reveal(
        &mut self,
        docs: &mut Documents,
        (pane, clear): (PaneId, f32),
        s: &ReadAloudSettings,
        turned: bool,
    ) {
        let Some((start, end)) = self.lit_spots() else {
            return;
        };
        // A turn lands first; the line is brought up once it has.
        if turned || !s.follow || self.revealed == Some(start) {
            return;
        }
        let last = if end.page == start.page {
            end.char
        } else {
            start.char + 1
        };
        if docs.reveal_said(pane, start.page, start.char..last, clear) {
            self.revealed = Some(start);
        }
    }

    pub fn lit(&self) -> Option<Lit> {
        let (start, end) = self
            .lit_spots()
            .filter(|_| !self.finished && !self.relaid)?;
        Some(Lit {
            span: Span::Chars { start, end },
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
    fn lit_words(&self, highlight: (Highlight, u8)) -> Option<Range<usize>> {
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
        let preferred = self.lit_words(self.highlight).unwrap_or_default();
        let heard = self.lit_words((Highlight::Words, 1)).map_or(0, |r| r.start);
        let start = said.start;
        Some(Lyrics {
            before: self.behind.back().map(|s| s.sentence.text.clone()),
            now: said.text.clone(),
            words: said.words.iter().map(|w| w.bytes.clone()).collect(),
            lit,
            preferred,
            heard,
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
    pub fn spot(&self) -> Option<(usize, Range<usize>)> {
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
            escape: (self.current.as_ref())
                .and_then(|s| s.parts.last())
                .map(|p| p.kind)
                .filter(|_| can && !self.finished && self.asking.is_none()),
            ask: self.asking.filter(|_| can).map(|p| p.kind),
            at: progress.map(|p| p.0),
            left: progress.map(|p| p.1),
            total: progress.map(|p| p.2),
            seek: can && self.chosen.is_none(),
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
        let speed = self.steady.bytes(1.0, self.tone.rate).max(1.0);
        let seconds = |pages: f64| pages * usual / speed;
        Some((
            ((here - from) / span) as f32,
            seconds(to - here),
            seconds(span),
        ))
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
            Act::Escape => self.escape(),
            Act::Answer(read) => self.answer(read),
            Act::Seek(_) | Act::Card(_) | Act::Reveal => {}
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

/// The reader turns and lights the panes the front end holds.
impl Panes for Documents {
    fn go_to(&mut self, pane: PaneId, page: usize) {
        Documents::go_to(self, pane, page);
    }

    fn reveal_said(&mut self, pane: PaneId, page: usize, chars: Range<usize>, clear: f32) -> bool {
        Documents::reveal_said(self, pane, page, chars, clear)
    }
}

/// A pane's book, as the reader below the front end sees it.
impl Reading {
    /// Where it stands: its pane, its book, and the page showing.
    pub fn stand(&self) -> Stand {
        Stand {
            pane: self.pane,
            doc: self.doc,
            document: Arc::clone(&self.document),
            current: self.current,
        }
    }
}
