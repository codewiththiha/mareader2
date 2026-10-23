//! Local models: sentences made ahead on a thread, played without gaps.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mareader_audio::{Clip, Fetch, Player, Segment};

use crate::models::Knob;
use crate::studio::{Key, Pcm, Studio};
use crate::{Caps, Event, Fault, Tone, Voice, Voicer, Waker};

/// What a model is asked to say.
pub(crate) struct Line<'a> {
    pub text: &'a str,
    /// 1 is the voice's own pace.
    pub speed: f32,
    /// The model's settings, in the order it lists them.
    pub knobs: Vec<f32>,
    /// Said now: a short first piece starts the sound sooner.
    pub lead: bool,
}

/// Takes each piece, its rate, and whether it is last; false stops.
pub(crate) type Out<'a> = dyn FnMut(&[f32], u32, bool) -> bool + 'a;

/// A model run on the engine's thread: it loads voices and says lines.
pub(crate) trait Synth: Send + 'static {
    /// Whether `voice` needs a model read from disk before it speaks.
    fn cold(&self, voice: &str) -> bool;
    /// Gets `voice` ready to speak; quick once it is.
    fn load(&mut self, voice: &str) -> Result<(), String>;
    /// Lets go of what `load` holds.
    fn unload(&mut self);
    fn loaded(&self) -> bool;
    /// Says `line` in the voice loaded, piece by piece, into `out`.
    fn render(&mut self, line: &Line<'_>, out: &mut Out<'_>) -> Result<(), String>;
}

/// What an engine is, besides its model.
pub(crate) struct Setup {
    /// The model's name with its build's, so its speech is its own.
    pub name: Arc<str>,
    /// Its downloaded voices; the first is the default.
    pub voices: Vec<Voice>,
    pub speeds: RangeInclusive<f32>,
    pub knobs: &'static [Knob],
}

/// How often a loaded model's idle time is checked against the setting.
const RECHECK: Duration = Duration::from_secs(30);
/// Quiet left at a join inside a sentence, beside the pause owed, in seconds.
const JOIN: f64 = 0.02;
/// Samples quieter than this count as silence at a join.
const QUIET: f32 = 0.003;
/// A piece counts as heard this close to its end, in seconds.
const END: f64 = 0.01;

/// What the thread is asked to make.
#[derive(Default)]
struct Plan {
    /// The sentence to say now, by its ticket.
    now: Option<(u64, Key)>,
    /// Sentences likely next, made once nothing is said.
    ahead: Vec<Key>,
    closed: bool,
}

/// The plan, and a bell rung when it changes.
#[derive(Default)]
struct Desk {
    plan: Mutex<Plan>,
    bell: Condvar,
    /// The model is being read from disk.
    loading: AtomicBool,
}

/// What the thread sends back about the sentence said now.
enum Made {
    /// A piece of it, in order; `last` ends it.
    Piece {
        ticket: u64,
        pcm: Pcm,
        last: bool,
    },
    Failed {
        ticket: u64,
        why: String,
    },
}

/// Who a queued piece belongs to.
#[derive(Clone, Debug, PartialEq)]
enum Owner {
    Said(u64),
    /// The sentence likely said next, queued before it is asked for.
    Next(Key),
}

/// A piece queued in the player.
struct Part {
    tag: u64,
    owner: Owner,
    last: bool,
    seconds: f64,
}

/// A local model, ready to speak in its downloaded voices.
pub(crate) struct Engine {
    /// The build's name, so its speech isn't taken for another's.
    name: Arc<str>,
    voices: Vec<Voice>,
    voice: String,
    first: String,
    speed: f32,
    speeds: RangeInclusive<f32>,
    knobs: &'static [Knob],
    /// Each knob's value, as bits.
    tuned: Vec<u32>,
    studio: Studio,
    desk: Arc<Desk>,
    made: Receiver<Made>,
    player: Player,
    clips: Arc<Mutex<HashMap<String, Clip>>>,
    /// Pieces the player has queued, in order.
    line: VecDeque<Part>,
    tags: u64,
    tickets: u64,
    /// The ticket being made to say now, and the sentence's id.
    making: Option<(u64, u64)>,
    started: Option<u64>,
    /// The sentences last prepared.
    ahead: Vec<String>,
    paused: bool,
    fault: Option<String>,
    events: Vec<Event>,
}

impl Engine {
    /// `synth` speaking on a thread of its own, as `setup` says.
    pub(crate) fn open<S: Synth>(
        synth: S,
        setup: Setup,
        studio: Studio,
        wake: Waker,
    ) -> Result<Self, Fault> {
        let Setup {
            name,
            voices,
            speeds,
            knobs,
        } = setup;
        let first = voices.first().map(|v| v.id.clone());
        let first = first.ok_or_else(|| Fault::Unavailable(format!("{name} has no voice yet")))?;
        let desk = Arc::new(Desk::default());
        let (tx, made) = channel();
        let worker = Worker {
            synth,
            crew: Crew {
                studio: studio.clone(),
                desk: Arc::clone(&desk),
                out: tx,
                wake,
            },
        };
        let thread = std::thread::Builder::new().name(name.to_lowercase());
        thread
            .spawn(move || worker.run())
            .map_err(|e| Fault::Unavailable(format!("{name} couldn't start: {e}")))?;
        let clips: Arc<Mutex<HashMap<String, Clip>>> = Arc::default();
        let held = Arc::clone(&clips);
        let fetch: Fetch = Arc::new(move |path: &str| lock(&held).get(path).cloned());
        let tuned = knobs.iter().map(|k| k.clamp(None).to_bits()).collect();
        Ok(Self {
            name,
            voices,
            voice: first.clone(),
            first,
            speed: 1.0,
            speeds,
            knobs,
            tuned,
            studio,
            desk,
            made,
            player: Player::new(fetch),
            clips,
            line: VecDeque::new(),
            tags: 0,
            tickets: 0,
            making: None,
            started: None,
            ahead: Vec::new(),
            paused: false,
            fault: None,
            events: Vec::new(),
        })
    }

    fn key(&self, text: &str) -> Key {
        Key {
            model: Arc::clone(&self.name),
            voice: self.voice.clone(),
            speed: self.speed.to_bits(),
            knobs: self.tuned.clone(),
            text: text.to_owned(),
        }
    }

    fn plan(&self, change: impl FnOnce(&mut Plan)) {
        change(&mut lock(&self.desk.plan));
        self.desk.bell.notify_all();
    }

    fn plan_ahead(&self) {
        let keys = self.ahead.iter().map(|t| self.key(t)).collect();
        self.plan(|p| p.ahead = keys);
    }

    /// Drops everything queued or being made to say.
    fn cut(&mut self) {
        self.player.stop();
        self.line.clear();
        lock(&self.clips).clear();
        self.making = None;
        self.started = None;
        self.plan(|p| p.now = None);
    }

    /// Takes the queued next sentence as `id`, if it is `key`.
    fn adopt(&mut self, id: u64, key: &Key) -> bool {
        let next = Owner::Next(key.clone());
        if self.line.front().is_none_or(|p| p.owner != next) {
            return false;
        }
        for part in self.line.iter_mut().filter(|p| p.owner == next) {
            part.owner = Owner::Said(id);
        }
        if self.ahead.first() == Some(&key.text) {
            self.ahead.remove(0);
            self.plan_ahead();
        }
        true
    }

    /// Queues a piece; an empty last piece ends its sentence at once.
    fn push(&mut self, owner: Owner, pcm: Pcm, last: bool) {
        if pcm.is_empty() {
            if !last {
                return;
            }
            match self.line.iter_mut().rev().find(|p| p.owner == owner) {
                Some(part) => part.last = true,
                None => self.over(&owner),
            }
            return;
        }
        self.tags += 1;
        let tag = self.tags;
        let seconds = pcm.seconds();
        let clip = Clip::Pcm {
            samples: pcm.samples,
            rate: pcm.rate,
        };
        lock(&self.clips).insert(tag.to_string(), clip);
        let segment = Segment {
            path: tag.to_string(),
            from: 0.0,
            until: None,
            tag,
        };
        if self.line.is_empty() {
            self.player.play(vec![segment]);
            if self.paused {
                self.player.pause();
            }
        } else {
            self.player.queue(vec![segment]);
        }
        self.line.push_back(Part {
            tag,
            owner,
            last,
            seconds,
        });
    }

    /// A sentence said whole.
    fn over(&mut self, owner: &Owner) {
        if let Owner::Said(id) = *owner {
            if self.started != Some(id) {
                self.events.push(Event::Started(id));
            }
            self.events.push(Event::Finished(id));
            self.started = None;
        }
    }

    /// Lets go of pieces heard, telling of sentences started and ended.
    fn follow(&mut self) {
        let Some((tag, time)) = self.player.status().at else {
            return;
        };
        let Some(heard) = self.line.iter().position(|p| p.tag == tag) else {
            return;
        };
        for _ in 0..heard {
            self.pop();
        }
        let Some(part) = self.line.front() else {
            return;
        };
        if let Owner::Said(id) = part.owner
            && self.started != Some(id)
        {
            self.started = Some(id);
            self.events.push(Event::Started(id));
        }
        if time >= part.seconds - END {
            self.pop();
        }
    }

    fn pop(&mut self) {
        let Some(part) = self.line.pop_front() else {
            return;
        };
        lock(&self.clips).remove(&part.tag.to_string());
        if part.last {
            self.over(&part.owner);
        }
    }

    /// Once the sentence said is all queued, the next waits behind it.
    fn queue_next(&mut self) {
        let whole = (self.line.back()).is_some_and(|p| p.last && matches!(p.owner, Owner::Said(_)));
        if !whole || self.making.is_some() {
            return;
        }
        let Some(key) = self.ahead.first().map(|t| self.key(t)) else {
            return;
        };
        let Some(pcm) = self.studio.takes().get(&key) else {
            return;
        };
        self.push(Owner::Next(key), pcm, true);
    }
}

impl Voicer for Engine {
    fn caps(&self) -> Caps {
        Caps {
            rate: true,
            pitch: false,
            volume: true,
            voices: true,
            pause: true,
        }
    }

    fn voices(&self) -> Vec<Voice> {
        self.voices.clone()
    }

    fn set_voice(&mut self, id: Option<&str>) {
        let known = id.filter(|id| self.voices.iter().any(|v| v.id == *id));
        let voice = known.unwrap_or(&self.first).to_owned();
        if voice != self.voice {
            self.voice = voice;
            self.plan_ahead();
        }
    }

    fn set_tone(&mut self, tone: Tone) {
        let speed = match tone.rate.is_finite() {
            true => tone.rate.clamp(*self.speeds.start(), *self.speeds.end()),
            false => 1.0,
        };
        self.player.set_volume(tone.volume);
        if speed != self.speed {
            self.speed = speed;
            self.plan_ahead();
        }
    }

    fn say(&mut self, id: u64, text: &str) -> Result<(), Fault> {
        if let Some(why) = self.fault.take() {
            return Err(Fault::Failed(why));
        }
        let key = self.key(text);
        if self.adopt(id, &key) {
            return Ok(());
        }
        self.cut();
        let made = self.studio.takes().get(&key);
        match made {
            Some(pcm) => self.push(Owner::Said(id), pcm, true),
            None => {
                self.tickets += 1;
                let ticket = self.tickets;
                self.making = Some((ticket, id));
                self.plan(|p| p.now = Some((ticket, key)));
            }
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.cut();
    }

    fn pause(&mut self) -> bool {
        self.paused = true;
        self.player.pause();
        true
    }

    fn resume(&mut self) -> bool {
        self.paused = false;
        self.player.resume();
        true
    }

    fn set_knobs(&mut self, values: &BTreeMap<String, f32>) {
        let tuned: Vec<u32> = (self.knobs.iter())
            .map(|k| k.clamp(values.get(k.id).copied()).to_bits())
            .collect();
        if tuned != self.tuned {
            self.tuned = tuned;
            self.plan_ahead();
        }
    }

    fn loading(&self) -> bool {
        self.desk.loading.load(Ordering::Relaxed)
    }

    fn prepare(&mut self, texts: &[&str]) {
        if self
            .ahead
            .iter()
            .map(String::as_str)
            .eq(texts.iter().copied())
        {
            return;
        }
        self.ahead = texts.iter().map(|t| (*t).to_owned()).collect();
        self.plan_ahead();
    }

    fn events(&mut self) -> Vec<Event> {
        while let Ok(made) = self.made.try_recv() {
            let id = match (self.making, &made) {
                (Some((t, id)), Made::Piece { ticket, .. } | Made::Failed { ticket, .. })
                    if t == *ticket =>
                {
                    id
                }
                _ => continue,
            };
            match made {
                Made::Piece { pcm, last, .. } => {
                    if last {
                        self.making = None;
                    }
                    self.push(Owner::Said(id), pcm, last);
                }
                Made::Failed { why, .. } => {
                    self.making = None;
                    self.fault = Some(why);
                    self.over(&Owner::Said(id));
                }
            }
        }
        self.follow();
        self.queue_next();
        std::mem::take(&mut self.events)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.plan(|p| p.closed = true);
    }
}

/// What the thread shares with the engine.
struct Crew {
    studio: Studio,
    desk: Arc<Desk>,
    out: Sender<Made>,
    wake: Waker,
}

/// Makes the sentence said now first, then those ahead; idle, it lets go.
struct Worker<S> {
    synth: S,
    crew: Crew,
}

impl<S: Synth> Worker<S> {
    fn run(mut self) {
        while let Some((ticket, key)) = self.next() {
            self.make(ticket, &key);
        }
    }

    /// The next sentence to make, waiting for one; `None` once closed.
    fn next(&mut self) -> Option<(Option<u64>, Key)> {
        let since = Instant::now();
        let crew = &self.crew;
        let mut plan = lock(&crew.desk.plan);
        loop {
            if plan.closed {
                return None;
            }
            if let Some((ticket, key)) = plan.now.clone() {
                return Some((Some(ticket), key));
            }
            let takes = crew.studio.takes();
            let ahead = plan.ahead.iter().find(|k| !takes.has(k)).cloned();
            drop(takes);
            if let Some(key) = ahead {
                return Some((None, key));
            }
            let idle = crew.studio.idle();
            if self.synth.loaded() && idle.is_some_and(|d| since.elapsed() >= d) {
                self.synth.unload();
                (crew.wake)();
            }
            let bell = &crew.desk.bell;
            plan = match (self.synth.loaded(), idle) {
                (false, _) => bell.wait(plan).unwrap_or_else(PoisonError::into_inner),
                (true, idle) => {
                    let left = idle.map_or(RECHECK, |d| d.saturating_sub(since.elapsed()));
                    let waited = bell.wait_timeout(plan, left.min(RECHECK));
                    waited.unwrap_or_else(PoisonError::into_inner).0
                }
            };
        }
    }

    /// Makes one sentence, sending it on in pieces if it is said now.
    fn make(&mut self, ticket: Option<u64>, key: &Key) {
        let crew = &self.crew;
        let kept = ticket.and_then(|_| crew.studio.takes().get(key));
        let was_kept = kept.is_some();
        let made = match kept {
            Some(pcm) => Ok(Some(pcm)),
            None => self.render(ticket, key),
        };
        let crew = &self.crew;
        match made {
            Ok(Some(pcm)) if was_kept => crew.send(ticket, pcm, true),
            Ok(Some(pcm)) => crew.studio.takes().put(key.clone(), pcm),
            Ok(None) => {}
            Err(why) => {
                if let Some(ticket) = ticket {
                    let _ = crew.out.send(Made::Failed { ticket, why });
                    (crew.wake)();
                }
            }
        }
        let mut plan = lock(&crew.desk.plan);
        match ticket {
            Some(t) if plan.now.as_ref().is_some_and(|(n, _)| *n == t) => plan.now = None,
            Some(_) => {}
            None => plan.ahead.retain(|k| k != key),
        }
    }

    /// `key` spoken; `None` if it stopped being wanted part way.
    fn render(&mut self, ticket: Option<u64>, key: &Key) -> Result<Option<Pcm>, String> {
        self.load(&key.voice)?;
        let crew = &self.crew;
        if crew.stale(ticket, key) {
            return Ok(None);
        }
        let line = Line {
            text: &key.text,
            speed: f32::from_bits(key.speed),
            knobs: key.knobs.iter().map(|b| f32::from_bits(*b)).collect(),
            lead: ticket.is_some(),
        };
        let (mut all, mut rate) = (Vec::<i16>::new(), 0);
        let (mut ended, mut stopped) = (false, false);
        self.synth.render(&line, &mut |audio, r, last| {
            let piece: Vec<i16> = audio.iter().map(|&s| sample(s)).collect();
            crew.send(ticket, Pcm::new(&piece, r), last);
            (rate, ended) = (r, ended || last);
            all.extend(piece);
            stopped = crew.stale(ticket, key);
            !stopped
        })?;
        if !ended {
            if stopped {
                return Ok(None);
            }
            crew.send(ticket, Pcm::new(&[], rate), true);
        }
        Ok(Some(Pcm::new(&all, rate)))
    }

    /// Loads `voice`, telling the window while a model is read from disk.
    fn load(&mut self, voice: &str) -> Result<(), String> {
        let crew = &self.crew;
        let cold = self.synth.cold(voice);
        if cold {
            crew.desk.loading.store(true, Ordering::Relaxed);
            (crew.wake)();
        }
        let loaded = self.synth.load(voice);
        if cold {
            crew.desk.loading.store(false, Ordering::Relaxed);
            (crew.wake)();
        }
        loaded
    }
}

impl Crew {
    fn send(&self, ticket: Option<u64>, pcm: Pcm, last: bool) {
        if let Some(ticket) = ticket {
            let _ = self.out.send(Made::Piece { ticket, pcm, last });
            (self.wake)();
        }
    }

    /// True once the work for `key` is no longer wanted.
    fn stale(&self, ticket: Option<u64>, key: &Key) -> bool {
        let plan = lock(&self.desk.plan);
        let now = plan.now.as_ref();
        match ticket {
            _ if plan.closed => true,
            Some(t) => now.is_none_or(|(n, _)| *n != t),
            None => match now {
                Some((_, k)) => k != key,
                None => !plan.ahead.contains(key),
            },
        }
    }
}

/// Trims a piece's lead-in if `start`, and its tail to `end` seconds.
pub(crate) fn trim_join(audio: &mut Vec<f32>, rate: u32, start: bool, end: Option<f64>) {
    let rate = f64::from(rate);
    let keep = (rate * JOIN) as usize;
    if let Some(pause) = end {
        let owed = keep.saturating_add((rate * pause.clamp(0.0, 1.0)) as usize);
        let quiet = audio.iter().rev().take_while(|s| s.abs() < QUIET).count();
        audio.truncate(audio.len().saturating_sub(quiet.saturating_sub(owed)));
    }
    if start {
        let quiet = audio.iter().take_while(|s| s.abs() < QUIET).count();
        audio.drain(..quiet.saturating_sub(keep));
    }
}

/// A model sample as 16 bits.
fn sample(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const RATE: u32 = 24_000;

    /// Says each word as a tenth of a second; fails to load if told to.
    #[derive(Default)]
    struct Fake {
        fail: bool,
        loaded: bool,
    }

    impl Synth for Fake {
        fn cold(&self, _: &str) -> bool {
            !self.loaded
        }

        fn load(&mut self, _: &str) -> Result<(), String> {
            self.loaded = !self.fail;
            match self.fail {
                true => Err("no model here".into()),
                false => Ok(()),
            }
        }

        fn unload(&mut self) {
            self.loaded = false;
        }

        fn loaded(&self) -> bool {
            self.loaded
        }

        fn render(&mut self, line: &Line<'_>, out: &mut Out<'_>) -> Result<(), String> {
            let words = line.text.split_whitespace().count();
            let level = line.knobs.first().copied().unwrap_or(0.5);
            for i in 0..words {
                if !out(&[level; 2_400], RATE, i + 1 == words) {
                    break;
                }
            }
            Ok(())
        }
    }

    const KNOBS: &[Knob] = &[Knob {
        id: "level",
        name: "Level",
        about: "How loud each word is",
        range: (0.0, 1.0),
    }];

    fn setup(name: &str) -> Setup {
        Setup {
            name: name.into(),
            voices: vec![Voice {
                id: "one".into(),
                name: "One".into(),
                language: "en-US".into(),
            }],
            speeds: 0.5..=2.0,
            knobs: KNOBS,
        }
    }

    fn engine_with(fake: Fake) -> Engine {
        let wake: Waker = Arc::new(|| {});
        match Engine::open(fake, setup("Fake"), Studio::default(), wake) {
            Ok(e) => e,
            Err(e) => panic!("{e}"),
        }
    }

    fn engine() -> Engine {
        engine_with(Fake::default())
    }

    fn pcm(samples: usize) -> Pcm {
        Pcm::new(&vec![1i16; samples], RATE)
    }

    /// Waits until `e` has nothing being made, gathering its events.
    fn settle(e: &mut Engine, until: impl Fn(&Engine) -> bool) -> Vec<Event> {
        let mut heard = Vec::new();
        for _ in 0..1000 {
            heard.extend(e.events());
            if until(e) {
                return heard;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("never settled; heard {heard:?}");
    }

    #[test]
    fn a_sentence_is_queued_piece_by_piece_and_kept_whole() {
        let mut e = engine();
        assert_eq!(e.say(1, "One two three."), Ok(()));
        let key = e.key("One two three.");
        settle(&mut e, |e| e.making.is_none() && e.studio.takes().has(&key));
        assert_eq!(e.line.len(), 3, "a piece a word");
        assert!(e.line.back().is_some_and(|p| p.last));
        let kept = e.studio.takes().get(&e.key("One two three."));
        assert_eq!(kept.map(|p| (p.len(), p.rate)), Some((7_200, RATE)));
    }

    #[test]
    fn a_changed_knob_is_a_different_sentence_and_out_of_range_is_held() {
        let mut e = engine();
        let before = e.key("Hello.");
        assert_eq!(before.knobs, [1f32.to_bits()], "unset is 1");
        e.set_knobs(&BTreeMap::from([("level".to_owned(), 0.25)]));
        assert_ne!(e.key("Hello."), before);
        assert_eq!(e.key("Hello.").knobs, [0.25f32.to_bits()]);
        e.set_knobs(&BTreeMap::from([("level".to_owned(), 7.0)]));
        assert_eq!(e.key("Hello.").knobs, [1f32.to_bits()]);
        e.set_tone(Tone {
            rate: 9.0,
            ..Tone::default()
        });
        assert_eq!(e.speed, 2.0, "held to the model's speeds");
    }

    #[test]
    fn a_sentence_said_before_is_queued_at_once() {
        let mut e = engine();
        let key = e.key("Hello.");
        e.studio.takes().put(key, pcm(2400));
        assert_eq!(e.say(1, "Hello."), Ok(()));
        assert!(e.making.is_none(), "nothing to make");
        assert_eq!(e.line.len(), 1);
        assert!(
            e.line
                .front()
                .is_some_and(|p| p.last && (p.seconds - 0.1).abs() < 1e-9)
        );
    }

    #[test]
    fn the_next_sentence_waits_behind_and_is_taken_when_said() {
        let mut e = engine();
        for text in ["One.", "Two."] {
            let key = e.key(text);
            e.studio.takes().put(key, pcm(2400));
        }
        assert_eq!(e.say(1, "One."), Ok(()));
        e.prepare(&["Two.", "Three."]);
        e.queue_next();
        assert_eq!(e.line.len(), 2, "two waits behind one");
        // One heard whole: two is at the front, so saying it goes on.
        e.pop();
        assert_eq!(e.events, [Event::Started(1), Event::Finished(1)]);
        let two = e.key("Two.");
        assert!(e.adopt(2, &two));
        assert_eq!(e.ahead, ["Three."]);
        assert!(e.line.front().is_some_and(|p| p.owner == Owner::Said(2)));
    }

    #[test]
    fn an_unmade_sentence_is_asked_for_and_a_failure_tells_on_the_next() {
        let mut e = engine_with(Fake {
            fail: true,
            ..Fake::default()
        });
        assert_eq!(e.say(4, "Not made yet."), Ok(()));
        assert!(e.making.is_some());
        let heard = settle(&mut e, |e| e.making.is_none());
        assert_eq!(heard, [Event::Started(4), Event::Finished(4)]);
        assert!(matches!(e.say(5, "Next."), Err(Fault::Failed(_))));
        assert_eq!(e.say(5, "Next."), Ok(()), "told once");
    }

    #[test]
    fn nothing_to_say_still_ends_the_sentence() {
        let mut e = engine();
        assert_eq!(e.say(6, "   "), Ok(()));
        let heard = settle(&mut e, |e| e.making.is_none());
        assert_eq!(heard, [Event::Started(6), Event::Finished(6)]);
    }

    /// Says two sentences with a downloaded model, timing the first sound.
    pub(crate) fn live(e: Engine, what: &str) {
        let english = (
            "Hello there, this is the first sentence, read aloud.",
            "And this is the one after it.",
        );
        live_saying(e, what, english);
    }

    /// As `live`, saying `first` then making `next` ahead.
    pub(crate) fn live_saying(mut e: Engine, what: &str, (first, next): (&str, &str)) {
        let start = std::time::Instant::now();
        assert_eq!(e.say(1, first), Ok(()));
        e.prepare(&[next]);
        let (said, ahead) = (e.key(first), e.key(next));
        let mut heard = None;
        while start.elapsed() < Duration::from_secs(600) {
            assert!(e.events().is_empty(), "nothing plays without a device");
            heard = heard.or((!e.line.is_empty()).then(|| start.elapsed()));
            if e.making.is_none() && e.studio.takes().has(&ahead) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let made = start.elapsed();
        assert!(e.fault.is_none(), "{:?}", e.fault);
        // Made ahead, it is queued behind on the next look.
        assert!(e.events().is_empty());
        assert!(e.making.is_none() && e.studio.takes().has(&ahead));
        let speech = [&said, &ahead].map(|k| e.studio.takes().get(k).map_or(0.0, |p| p.seconds()));
        assert!(speech.iter().all(|s| *s > 1.0), "{speech:?}");
        assert!(e.line.back().is_some_and(|p| p.owner == Owner::Next(ahead)));
        assert!(
            !e.studio.usage().models.is_empty(),
            "the model shows as loaded"
        );
        println!(
            "{what}: first sound ready after {heard:?}; {speech:?} s of speech made in {made:?}"
        );
    }

    #[test]
    fn joins_inside_a_sentence_keep_only_the_pause_owed() {
        let rate = RATE as usize;
        let keep = rate / 50;
        let piece = || [vec![0.0; rate], vec![0.5; 100], vec![0.0; rate]].concat();
        // Head of a sentence: lead-in kept, tail cut to a 0.1 s pause.
        let mut head = piece();
        trim_join(&mut head, RATE, false, Some(0.1));
        assert_eq!(head.len(), rate + 100 + keep + rate / 10);
        // Tail of a sentence: lead-in cut, its own ending kept.
        let mut tail = piece();
        trim_join(&mut tail, RATE, true, None);
        assert_eq!(tail.len(), keep + 100 + rate);
        // Silence alone, or nothing, never panics.
        let mut quiet = vec![0.0; 10];
        trim_join(&mut quiet, RATE, true, Some(0.0));
        assert!(quiet.len() <= 10);
        trim_join(&mut Vec::new(), RATE, true, Some(5.0));
    }

    #[test]
    fn samples_are_clamped_into_sixteen_bits() {
        assert_eq!(sample(0.0), 0);
        assert_eq!(sample(1.5), i16::MAX);
        assert_eq!(sample(-1.5), -i16::MAX);
    }
}
