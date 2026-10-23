//! Kokoro's voices: sentences made ahead on a thread, played without gaps.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use mareader_audio::{Clip, Fetch, Player, Segment};
use mareader_g2p::{Dialect, G2p};
use mareader_kokoro::{Kokoro, SAMPLE_RATE, SPEEDS, Voice as Pack};

use crate::studio::{Key, Pcm, Studio};
use crate::{Caps, Event, Fault, Tone, Voice, Voicer, Waker};

/// A model unused this long is let go; the next sentence loads it again.
const IDLE: Duration = Duration::from_secs(120);
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

/// Kokoro, ready to speak in its downloaded voices.
pub(crate) struct Engine {
    voices: Vec<Voice>,
    voice: String,
    first: String,
    speed: f32,
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
    /// Kokoro in `folder`, speaking `voices`; the first is the default.
    pub(crate) fn open(
        folder: PathBuf,
        voices: Vec<Voice>,
        studio: Studio,
        wake: Waker,
    ) -> Result<Self, Fault> {
        let first = voices.first().map(|v| v.id.clone());
        let first = first.ok_or_else(|| Fault::Unavailable("Kokoro has no voice yet".into()))?;
        let desk = Arc::new(Desk::default());
        let (tx, made) = channel();
        let worker = Worker {
            studio: studio.clone(),
            folder,
            desk: Arc::clone(&desk),
            out: tx,
            wake,
            loaded: None,
        };
        let thread = std::thread::Builder::new().name("kokoro".into());
        thread
            .spawn(move || worker.run())
            .map_err(|e| Fault::Unavailable(format!("Kokoro couldn't start: {e}")))?;
        let clips: Arc<Mutex<HashMap<String, Clip>>> = Arc::default();
        let held = Arc::clone(&clips);
        let fetch: Fetch = Arc::new(move |path: &str| lock(&held).get(path).cloned());
        Ok(Self {
            voices,
            voice: first.clone(),
            first,
            speed: 1.0,
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
            voice: self.voice.clone(),
            speed: self.speed.to_bits(),
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
        let seconds = pcm.len() as f64 / f64::from(SAMPLE_RATE);
        let clip = Clip::Pcm {
            samples: pcm,
            rate: SAMPLE_RATE,
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
            true => tone.rate.clamp(*SPEEDS.start(), *SPEEDS.end()),
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

/// What the thread holds while it works.
struct Loaded {
    kokoro: Arc<Kokoro>,
    g2p: Arc<G2p>,
    voice: (String, Pack),
}

/// Makes the sentence said now first, then those ahead; idle, it lets go.
struct Worker {
    studio: Studio,
    folder: PathBuf,
    desk: Arc<Desk>,
    out: Sender<Made>,
    wake: Waker,
    loaded: Option<Loaded>,
}

impl Worker {
    fn run(mut self) {
        while let Some((ticket, key)) = self.next() {
            self.make(ticket, &key);
        }
    }

    /// The next sentence to make, waiting for one; `None` once closed.
    fn next(&mut self) -> Option<(Option<u64>, Key)> {
        let mut plan = lock(&self.desk.plan);
        loop {
            if plan.closed {
                return None;
            }
            if let Some((ticket, key)) = plan.now.clone() {
                return Some((Some(ticket), key));
            }
            let takes = self.studio.takes();
            let ahead = plan.ahead.iter().find(|k| !takes.has(k)).cloned();
            drop(takes);
            if let Some(key) = ahead {
                return Some((None, key));
            }
            let (held, waited) = (self.desk.bell)
                .wait_timeout(plan, IDLE)
                .unwrap_or_else(PoisonError::into_inner);
            plan = held;
            if waited.timed_out() {
                self.loaded = None;
            }
        }
    }

    /// Makes one sentence, sending it on in pieces if it is said now.
    fn make(&mut self, ticket: Option<u64>, key: &Key) {
        let kept = ticket.and_then(|_| self.studio.takes().get(key));
        let was_kept = kept.is_some();
        let made = match kept {
            Some(pcm) => Ok(Some(pcm)),
            None => self.render(ticket, key),
        };
        match made {
            Ok(Some(pcm)) if was_kept => self.send(ticket, pcm, true),
            Ok(Some(pcm)) => self.studio.takes().put(key.clone(), pcm),
            Ok(None) => {}
            Err(why) => {
                if let Some(ticket) = ticket {
                    let _ = self.out.send(Made::Failed { ticket, why });
                    (self.wake)();
                }
            }
        }
        let mut plan = lock(&self.desk.plan);
        match ticket {
            Some(t) if plan.now.as_ref().is_some_and(|(n, _)| *n == t) => plan.now = None,
            Some(_) => {}
            None => plan.ahead.retain(|k| k != key),
        }
    }

    /// `key` spoken; `None` if it stopped being wanted part way.
    fn render(&mut self, ticket: Option<u64>, key: &Key) -> Result<Option<Pcm>, String> {
        let loaded = self.load(&key.voice)?;
        let (kokoro, g2p) = (Arc::clone(&loaded.kokoro), Arc::clone(&loaded.g2p));
        let mut phonemes = String::new();
        for token in g2p.tokens(&key.text) {
            phonemes.push_str(&token.phonemes);
            if token.whitespace {
                phonemes.push(' ');
            }
        }
        let chunks = kokoro.chunks(&kokoro.normalise(&phonemes));
        let speed = f32::from_bits(key.speed);
        let mut all: Vec<i16> = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            if self.stale(ticket, key) {
                return Ok(None);
            }
            let pack = self.loaded.as_ref().map(|l| &l.voice.1);
            let pack = pack.ok_or_else(|| "Kokoro was let go".to_owned())?;
            let audio = kokoro
                .render(chunk, pack, speed)
                .map_err(|e| e.to_string())?;
            let piece: Vec<i16> = audio.iter().map(|&s| sample(s)).collect();
            self.send(ticket, Arc::from(piece.as_slice()), i + 1 == chunks.len());
            all.extend(piece);
        }
        if chunks.is_empty() {
            self.send(ticket, Arc::from(Vec::new()), true);
        }
        Ok(Some(Arc::from(all)))
    }

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

    /// The model, the G2P and the voice for `voice`, loaded once.
    fn load(&mut self, voice: &str) -> Result<&Loaded, String> {
        let dialect = Dialect::for_kokoro_voice(voice).unwrap_or(Dialect::American);
        let fits = |l: &Loaded| l.voice.0 == voice && l.g2p.dialect() == dialect;
        if !self.loaded.as_ref().is_some_and(fits) {
            let folder = &self.folder;
            let kokoro =
                (self.studio).kokoro(|| Kokoro::load(folder).map_err(|e| e.to_string()))?;
            let g2p = self.studio.g2p(dialect, || {
                G2p::load(folder, dialect).map_err(|e| e.to_string())
            })?;
            let pack = Pack::load(&pack_path(folder, voice)).map_err(|e| e.to_string())?;
            self.loaded = Some(Loaded {
                kokoro,
                g2p,
                voice: (voice.to_owned(), pack),
            });
        }
        self.loaded
            .as_ref()
            .ok_or_else(|| "Kokoro was let go".to_owned())
    }
}

fn pack_path(folder: &Path, voice: &str) -> PathBuf {
    folder.join("voices").join(format!("{voice}.pt"))
}

/// A model sample as 16 bits.
fn sample(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_in(dir: PathBuf) -> Engine {
        let voices = vec![Voice {
            id: "af_heart".into(),
            name: "Heart".into(),
            language: "en-US".into(),
        }];
        let wake: Waker = Arc::new(|| {});
        match Engine::open(dir, voices, Studio::default(), wake) {
            Ok(e) => e,
            Err(e) => panic!("{e}"),
        }
    }

    fn engine() -> Engine {
        engine_in(std::env::temp_dir().join("mareader-no-kokoro"))
    }

    #[test]
    fn a_downloaded_model_makes_the_sentence_said_then_the_next() {
        // Only where a model folder is given, as the remote check does.
        let Some(dir) = std::env::var_os("MAREADER_KOKORO") else {
            return;
        };
        let mut e = engine_in(PathBuf::from(dir));
        let (first, next) = (
            "Hello there, this is the first sentence, read aloud.",
            "And this is the one after it.",
        );
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
        // Made ahead, it is queued behind on the next look.
        assert!(e.events().is_empty());
        assert!(e.making.is_none() && e.studio.takes().has(&ahead));
        let speech = [&said, &ahead].map(|k| e.studio.takes().get(k).map_or(0, |p| p.len()));
        let seconds = speech.map(|n| n as f64 / f64::from(SAMPLE_RATE));
        assert!(seconds.iter().all(|s| *s > 1.0), "{seconds:?}");
        assert!(e.line.back().is_some_and(|p| p.owner == Owner::Next(ahead)));
        println!(
            "kokoro engine: first sound ready after {heard:?}; {seconds:?} s of speech made in {made:?}"
        );
    }

    #[test]
    fn a_sentence_said_before_is_queued_at_once() {
        let mut e = engine();
        let key = e.key("Hello.");
        e.studio.takes().put(key, Arc::from(vec![1i16; 2400]));
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
            e.studio.takes().put(key, Arc::from(vec![1i16; 2400]));
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
        let mut e = engine();
        assert_eq!(e.say(4, "Not made yet."), Ok(()));
        assert!(e.making.is_some());
        // No model in the folder: the sentence fails and ends.
        let mut heard = Vec::new();
        for _ in 0..1000 {
            heard.extend(e.events());
            if heard.contains(&Event::Finished(4)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(heard, [Event::Started(4), Event::Finished(4)]);
        assert!(matches!(e.say(5, "Next."), Err(Fault::Failed(_))));
        assert_eq!(e.say(5, "Next."), Ok(()), "told once");
    }

    #[test]
    fn samples_are_clamped_into_sixteen_bits() {
        assert_eq!(sample(0.0), 0);
        assert_eq!(sample(1.5), i16::MAX);
        assert_eq!(sample(-1.5), -i16::MAX);
    }
}
