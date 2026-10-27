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
use crate::timing::Mark;
use crate::{Caps, Event, Fault, Heard, Tone, Voice, Voicer, Waker};

/// What a model is asked to say.
pub(crate) struct Line<'a> {
    pub text: &'a str,
    /// 1 is the voice's own pace.
    pub speed: f32,
    /// The model's settings, in the order it lists them.
    pub knobs: Vec<f32>,
    /// Short pieces first, then growing; off only for whole-sentence tests.
    pub lead: bool,
}

/// Takes a piece, its rate, if last, and its words' marks; false stops.
pub(crate) type Out<'a> = dyn FnMut(&[f32], u32, bool, &[Mark]) -> bool + 'a;

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

/// With no limit set, a model goes this long after reading pauses or stops.
const GRACE: Duration = Duration::from_secs(30);
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
    /// Something is playing or about to; idle time counts from quiet.
    speaking: bool,
    /// When playing last stopped, by pause, stop or the end.
    quiet_since: Option<Instant>,
    /// Pieces made ahead so far, unsent; a say takes them at once.
    early: Option<(Key, Vec<(Pcm, bool)>)>,
}

/// The plan, and a bell rung when it changes.
#[derive(Default)]
struct Desk {
    plan: Mutex<Plan>,
    bell: Condvar,
    /// The model is being read from disk.
    loading: AtomicBool,
}

impl crate::studio::Bell for Desk {
    fn ring(&self) {
        // Held, so the ring can't fall between a look and a wait.
        let _plan = lock(&self.plan);
        self.bell.notify_all();
    }
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
    /// Seconds of its sentence before it.
    from: f64,
    /// Its words: the second each starts in it, and its byte.
    words: Vec<(f64, usize)>,
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
    /// Seconds queued of the sentence queued last.
    queued: Option<(Owner, f64)>,
    heard: Option<Heard>,
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
        let bell: Arc<dyn crate::studio::Bell> = desk.clone();
        studio.hear(Arc::downgrade(&bell));
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
            queued: None,
            heard: None,
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

    /// Tells the thread whether anything is playing or about to.
    fn mark(&self) {
        let speaking = !self.paused && (self.making.is_some() || !self.line.is_empty());
        let mut plan = lock(&self.desk.plan);
        if plan.speaking != speaking {
            plan.speaking = speaking;
            plan.quiet_since = (!speaking).then(Instant::now);
            self.desk.bell.notify_all();
        }
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
        (self.queued, self.heard) = (None, None);
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
        if let Some((owner, _)) = self.queued.as_mut().filter(|(o, _)| *o == next) {
            *owner = Owner::Said(id);
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
        let from = match self.queued.as_mut() {
            Some((o, total)) if *o == owner => std::mem::replace(total, *total + seconds),
            _ => {
                self.queued = Some((owner.clone(), seconds));
                0.0
            }
        };
        let rate = f64::from(pcm.rate.max(1));
        let words = pcm.marks.iter().map(|m| (m.at as f64 / rate, m.byte));
        let words = words.collect();
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
            from,
            words,
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
        self.heard = hear(part, time, self.heard);
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
        // Saying is never paused: a step back while paused, then play.
        if self.paused {
            self.paused = false;
            self.player.resume();
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
                let mut early = Vec::new();
                self.plan(|p| {
                    if p.early.as_ref().is_some_and(|(k, _)| *k == key) {
                        early = p.early.take().map(|e| e.1).unwrap_or_default();
                    }
                    // All made already: nothing is left to ask for.
                    if !early.iter().any(|e| e.1) {
                        p.now = Some((ticket, key));
                    }
                });
                if !early.iter().any(|e| e.1) {
                    self.making = Some((ticket, id));
                }
                for (pcm, last) in early {
                    self.push(Owner::Said(id), pcm, last);
                }
            }
        }
        self.mark();
        Ok(())
    }

    fn stop(&mut self) {
        self.cut();
        self.mark();
    }

    fn pause(&mut self) -> bool {
        self.paused = true;
        self.player.pause();
        self.mark();
        true
    }

    fn resume(&mut self) -> bool {
        self.paused = false;
        self.player.resume();
        self.mark();
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

    fn heard(&self) -> Option<Heard> {
        self.heard
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
        self.mark();
        std::mem::take(&mut self.events)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.plan(|p| p.closed = true);
    }
}

/// A sentence spoken, `None` if stopped part way, or why it failed.
type Rendered = Result<Option<Pcm>, String>;

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
        // Closed: the model goes with the thread, its memory back too.
        drop(self);
        mareader_memory::give_back();
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
            // Idle from when playing stopped or the work ran out, the later.
            let quiet = plan.quiet_since.map_or(since, |q| q.max(since));
            let idle = crew.studio.idle().unwrap_or(GRACE);
            let left = idle.saturating_sub(quiet.elapsed());
            let counting = self.synth.loaded() && !plan.speaking;
            if counting && left.is_zero() {
                // Unlocked: letting go takes a moment, and reading may resume.
                drop(plan);
                self.synth.unload();
                mareader_memory::give_back();
                (crew.wake)();
                plan = lock(&crew.desk.plan);
                continue;
            }
            let bell = &crew.desk.bell;
            plan = match counting {
                false => bell.wait(plan).unwrap_or_else(PoisonError::into_inner),
                true => {
                    let waited = bell.wait_timeout(plan, left);
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
        // Made ahead, it may be said part way: `to` is who it went to.
        let (to, made) = match kept {
            Some(pcm) => (ticket, Ok(Some(pcm))),
            None => self.render(ticket, key),
        };
        let crew = &self.crew;
        match made {
            Ok(Some(pcm)) if was_kept => crew.send(ticket, pcm, true),
            Ok(Some(pcm)) => crew.studio.takes().put(key.clone(), pcm),
            Ok(None) => {}
            Err(why) => {
                if let Some(ticket) = to {
                    let _ = crew.out.send(Made::Failed { ticket, why });
                    (crew.wake)();
                }
            }
        }
        let mut plan = lock(&crew.desk.plan);
        if to.is_some() && plan.now.as_ref().map(|(n, _)| *n) == to {
            plan.now = None;
        }
        if ticket.is_none() {
            plan.ahead.retain(|k| k != key);
            plan.early = None;
        }
    }

    /// Who it went to, and `key` spoken; `None` if stopped part way.
    fn render(&mut self, ticket: Option<u64>, key: &Key) -> (Option<u64>, Rendered) {
        if let Err(why) = self.load(&key.voice) {
            return (ticket, Err(why));
        }
        let crew = &self.crew;
        if crew.stale(ticket, key) {
            return (ticket, Ok(None));
        }
        let line = Line {
            text: &key.text,
            speed: f32::from_bits(key.speed),
            knobs: key.knobs.iter().map(|b| f32::from_bits(*b)).collect(),
            lead: true,
        };
        let (mut all, mut rate, mut to) = (Vec::<i16>::new(), 0, ticket);
        let mut words = Vec::new();
        let (mut ended, mut stopped) = (false, false);
        let rendered = self.synth.render(&line, &mut |audio, r, last, marks| {
            let piece: Vec<i16> = audio.iter().map(|&s| sample(s)).collect();
            let pcm = Pcm::marked(&piece, r, marks);
            let moved = marks.iter().map(|m| Mark {
                at: m.at + all.len(),
                byte: m.byte,
            });
            words.extend(moved);
            match to {
                Some(_) => crew.send(to, pcm, last),
                None => to = crew.offer(key, pcm, last),
            }
            (rate, ended) = (r, ended || last);
            all.extend(piece);
            stopped = crew.stale(to, key);
            !stopped
        });
        if let Err(why) = rendered {
            return (to, Err(why));
        }
        if !ended {
            if stopped {
                return (to, Ok(None));
            }
            crew.send(to, Pcm::new(&[], rate), true);
        }
        (to, Ok(Some(Pcm::marked(&all, rate, &words))))
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

    /// Sends a piece made ahead if `key` is said now, else keeps it.
    fn offer(&self, key: &Key, pcm: Pcm, last: bool) -> Option<u64> {
        let mut plan = lock(&self.desk.plan);
        let wanted = plan.now.as_ref().filter(|(_, k)| k == key);
        if let Some(&(ticket, _)) = wanted {
            drop(plan);
            self.send(Some(ticket), pcm, last);
            return Some(ticket);
        }
        match &mut plan.early {
            Some((k, pieces)) if k == key => pieces.push((pcm, last)),
            early => *early = Some((key.clone(), vec![(pcm, last)])),
        }
        None
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

/// Runs `first` beside `second`, so a voice's files load at once.
pub(crate) fn both<A: Send, B>(
    first: impl FnOnce() -> Result<A, String> + Send,
    second: impl FnOnce() -> Result<B, String>,
) -> Result<(A, B), String> {
    std::thread::scope(|s| {
        let named = std::thread::Builder::new().name("voice load".into());
        let Ok(beside) = named.spawn_scoped(s, first) else {
            return Err("no thread to load the voice on".to_owned());
        };
        let second = second();
        let first = (beside.join()).map_err(|_| "loading the voice stopped short".to_owned())?;
        Ok((first?, second?))
    })
}

/// Trims the lead-in if `start`, the tail to `end` s; returns those cut first.
pub(crate) fn trim_join(audio: &mut Vec<f32>, rate: u32, start: bool, end: Option<f64>) -> usize {
    let rate = f64::from(rate);
    let keep = (rate * JOIN) as usize;
    if let Some(pause) = end {
        let owed = keep.saturating_add((rate * pause.clamp(0.0, 1.0)) as usize);
        let quiet = audio.iter().rev().take_while(|s| s.abs() < QUIET).count();
        audio.truncate(audio.len().saturating_sub(quiet.saturating_sub(owed)));
    }
    if !start {
        return 0;
    }
    let quiet = audio.iter().take_while(|s| s.abs() < QUIET).count();
    let cut = quiet.saturating_sub(keep).min(audio.len());
    audio.drain(..cut);
    cut
}

/// Where `part` is heard `time` into it; a word begun before is kept.
fn hear(part: &Part, time: f64, before: Option<Heard>) -> Option<Heard> {
    let Owner::Said(id) = part.owner else {
        return None;
    };
    let begun = part.words.iter().rev().find(|(s, _)| *s <= time);
    let kept = before.filter(|h| h.id == id).and_then(|h| h.word);
    // Before its first word, that word is next: a guess would run ahead.
    let first = part.words.first().map(|w| w.1);
    Some(Heard {
        id,
        seconds: part.from + time,
        word: begun.map(|w| w.1).or(kept).or(first),
    })
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
        /// Each word waits for a go from here, if set.
        gate: Option<std::sync::mpsc::Receiver<()>>,
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
            let text = line.text;
            let begins = |&(i, c): &(usize, char)| {
                let after = text
                    .get(..i)
                    .is_none_or(|b| b.is_empty() || b.ends_with(' '));
                !c.is_whitespace() && after
            };
            let starts: Vec<usize> = text.char_indices().filter(begins).map(|(i, _)| i).collect();
            let words = starts.len();
            let level = line.knobs.first().copied().unwrap_or(0.5);
            for (i, &byte) in starts.iter().enumerate() {
                if let Some(gate) = &self.gate {
                    let _ = gate.recv();
                }
                let mark = [Mark { at: 0, byte }];
                if !out(&[level; 2_400], RATE, i + 1 == words, &mark) {
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
    fn a_sentence_said_while_made_ahead_plays_what_is_made_at_once() {
        let (go, gate) = std::sync::mpsc::channel();
        let mut e = engine_with(Fake {
            gate: Some(gate),
            ..Fake::default()
        });
        let text = "One two three four five six seven eight.";
        e.prepare(&[text]);
        for _ in 0..2 {
            let _ = go.send(());
        }
        let early = |e: &Engine| lock(&e.desk.plan).early.as_ref().map_or(0, |p| p.1.len());
        settle(&mut e, |e| early(e) == 2);
        assert_eq!(e.say(7, text), Ok(()));
        assert!(e.making.is_some(), "not made whole yet");
        let mine = |e: &Engine| e.line.iter().filter(|p| p.owner == Owner::Said(7)).count();
        let queued = |e: &Engine| {
            let said = e.line.iter().filter(|p| p.owner == Owner::Said(7));
            said.map(|p| p.seconds).sum::<f64>()
        };
        assert!(
            (queued(&e) - 0.2).abs() < 1e-6,
            "what is made plays at once"
        );
        let _ = go.send(());
        settle(&mut e, |e| queued(e) > 0.25);
        assert!(!e.studio.takes().has(&e.key(text)), "queued before made");
        drop(go);
        settle(&mut e, |e| e.making.is_none());
        assert!((queued(&e) - 0.8).abs() < 1e-6, "each word once");
        assert_eq!(mine(&e), 8, "two made early, then one by one");
        let lasts = e
            .line
            .iter()
            .filter(|p| p.last && p.owner == Owner::Said(7));
        assert_eq!(lasts.count(), 1);
        settle(&mut e, |e| e.studio.takes().has(&e.key(text)));
    }

    #[test]
    fn a_sentence_made_ahead_in_full_but_not_kept_yet_is_queued_whole() {
        let mut e = engine();
        let text = "Two words.";
        let key = e.key(text);
        let piece = |last| (pcm(2400), last);
        e.plan(|p| p.early = Some((key, vec![piece(false), piece(true)])));
        assert_eq!(e.say(8, text), Ok(()));
        assert!(e.making.is_none(), "nothing left to make");
        assert!(lock(&e.desk.plan).now.is_none());
        assert_eq!(e.line.iter().filter(|p| p.last).count(), 1);
        assert_eq!(e.line.len(), 2);
    }

    #[test]
    fn each_word_is_heard_where_its_piece_says() {
        let mut e = engine();
        assert_eq!(e.say(9, "One two three."), Ok(()));
        settle(&mut e, |e| e.making.is_none());
        let words: Vec<(f64, Vec<(f64, usize)>)> =
            e.line.iter().map(|p| (p.from, p.words.clone())).collect();
        let tenths: Vec<i64> = words.iter().map(|w| (w.0 * 10.0).round() as i64).collect();
        assert_eq!(tenths, [0, 1, 2], "each a tenth on");
        let bytes: Vec<Vec<(f64, usize)>> = words.into_iter().map(|w| w.1).collect();
        assert_eq!(bytes, [vec![(0.0, 0)], vec![(0.0, 4)], vec![(0.0, 8)]]);
        let key = e.key("One two three.");
        let kept = e.studio.takes().get(&key).map(|p| p.marks.to_vec());
        let at: Option<Vec<(usize, usize)>> =
            kept.map(|m| m.iter().map(|m| (m.at, m.byte)).collect());
        assert_eq!(
            at,
            Some(vec![(0, 0), (2400, 4), (4800, 8)]),
            "kept whole too"
        );
    }

    #[test]
    fn the_word_heard_is_the_last_begun_or_the_one_before() {
        let part = |owner, words| Part {
            tag: 1,
            owner,
            last: false,
            seconds: 1.0,
            from: 2.0,
            words,
        };
        let said = part(Owner::Said(3), vec![(0.2, 10), (0.6, 15)]);
        let at = |time, before| hear(&said, time, before).and_then(|h| h.word);
        assert_eq!(at(0.7, None), Some(15));
        assert_eq!(at(0.3, None), Some(10));
        assert_eq!(
            at(0.1, None),
            Some(10),
            "nothing begun yet: the first is next"
        );
        let before = Heard {
            id: 3,
            seconds: 1.9,
            word: Some(4),
        };
        assert_eq!(at(0.1, Some(before)), Some(4), "begun in the piece before");
        let other = Heard { id: 2, ..before };
        assert_eq!(
            at(0.1, Some(other)),
            Some(10),
            "another sentence's isn't kept"
        );
        let bare = part(Owner::Said(3), vec![]);
        let unmarked = hear(&bare, 0.1, None).map(|h| h.word);
        assert_eq!(unmarked, Some(None), "no marks: the reader guesses");
        assert_eq!(hear(&said, 0.5, None).map(|h| h.seconds), Some(2.5));
        assert!(hear(&part(Owner::Next(e_key()), vec![]), 0.1, None).is_none());
    }

    fn e_key() -> Key {
        engine().key("Next.")
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
        let e = live_saying(e, what, english);
        catch_up(e, what);
    }

    /// Says a long sentence while it is made ahead, timing its first sound.
    fn catch_up(mut e: Engine, what: &str) {
        let long = "When the evening came, the travellers, tired from the long \
            road, stopped at a small inn beside the river, where the keeper, an \
            old man with a kind face, gave them bread, cheese and warm soup, and \
            told them stories of the hills until the fire burned low.";
        let asked = Instant::now();
        e.prepare(&[long]);
        std::thread::sleep(Duration::from_millis(300));
        let said = Instant::now();
        assert_eq!(e.say(3, long), Ok(()));
        let mut heard = None;
        while e.making.is_some() && said.elapsed() < Duration::from_secs(600) {
            assert!(e.events().is_empty(), "nothing plays without a device");
            let queued = e.line.iter().any(|p| p.owner == Owner::Said(3));
            heard = heard.or(queued.then(|| said.elapsed()));
            std::thread::sleep(Duration::from_millis(10));
        }
        let (whole, made) = (said.elapsed(), asked.elapsed());
        assert!(e.fault.is_none(), "{:?}", e.fault);
        let heard = heard.unwrap_or(whole);
        println!(
            "{what}: said 0.3 s into making ahead; first sound ready {heard:?} \
             after, whole {whole:?} after (made in {made:?})"
        );
    }

    /// Times `synth` loading `voice` from nothing, as the first word does.
    pub(crate) fn time_load(mut synth: impl Synth, voice: &str, what: &str) {
        let start = std::time::Instant::now();
        assert_eq!(synth.load(voice), Ok(()));
        println!("{what}: loaded from nothing in {:?}", start.elapsed());
    }

    /// As `live`, saying `first` then making `next` ahead.
    pub(crate) fn live_saying(mut e: Engine, what: &str, (first, next): (&str, &str)) -> Engine {
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
        e
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
