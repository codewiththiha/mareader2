//! System voices via `tts`: kept on the window's thread, never asked its voice.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use crate::{Caps, Event, Fault, Tone, Voice, Voicer, Waker, scaled};

/// The speeds and pitches the reader offers, as shares of normal.
const RATES: (f32, f32) = (0.5, 3.0);
const PITCHES: (f32, f32) = (0.5, 2.0);
/// Characters a second at normal speed, when nothing reports an end.
const PACE: f32 = 14.0;

/// Apple's speech opens once a run: a second open fails, so voicers share it.
const ONCE: bool = cfg!(target_vendor = "apple");

thread_local! {
    /// The one speech opened on this thread, where it opens only once.
    static SHARED: RefCell<Option<Rc<Speech>>> = const { RefCell::new(None) };
}

/// Numbers voicers, so a shared speech knows who spoke and who tuned it.
static NEXT: AtomicU64 = AtomicU64::new(1);

/// What one voicer heard begin and end, by the engine's name for it.
struct Ear {
    heard: Mutex<Vec<(bool, String)>>,
    wake: Waker,
}

/// Every voicer's ear on one speech; each picks out its own utterances.
type Ears = Arc<Mutex<Vec<Weak<Ear>>>>;

/// The system's speech and what it offers, as one or more voicers share it.
struct Speech {
    tts: tts::Tts,
    features: tts::Features,
    /// Its voices, read once: some systems are slow to list them.
    voices: Vec<tts::Voice>,
    ears: Ears,
    /// The voicer whose voice and tone it has, and the one that spoke last.
    tuned: Cell<u64>,
    speaker: Cell<u64>,
    /// A voice was set, so the default must be found again.
    voiced: Cell<bool>,
}

impl Speech {
    fn open() -> Result<Self, tts::Error> {
        let tts = tts::Tts::default()?;
        let mut features = tts.supported_features();
        let voices = match features.voice {
            true => tts.voices().unwrap_or_default(),
            false => Vec::new(),
        };
        let ears: Ears = Arc::default();
        if features.utterance_callbacks {
            let hear = |begun: bool| {
                let ears = Arc::clone(&ears);
                Box::new(move |id: tts::UtteranceId| {
                    let name = format!("{id:?}");
                    let ears = ears.lock().unwrap_or_else(PoisonError::into_inner);
                    for ear in ears.iter().filter_map(Weak::upgrade) {
                        let mut heard = ear.heard.lock().unwrap_or_else(PoisonError::into_inner);
                        heard.push((begun, name.clone()));
                        drop(heard);
                        (ear.wake)();
                    }
                }) as Box<dyn FnMut(tts::UtteranceId)>
            };
            // An engine that refuses callbacks falls back to watching.
            let begin = tts.on_utterance_begin(Some(hear(true)));
            let end = tts.on_utterance_end(Some(hear(false)));
            if begin.is_err() || end.is_err() {
                features.utterance_callbacks = false;
            }
        }
        Ok(Self {
            tts,
            features,
            voices,
            ears,
            tuned: Cell::new(0),
            speaker: Cell::new(0),
            voiced: Cell::new(false),
        })
    }

    /// This thread's speech: the shared one where it opens only once.
    fn get() -> Result<Rc<Self>, tts::Error> {
        if !ONCE {
            return Self::open().map(Rc::new);
        }
        SHARED.with(|shared| {
            if let Some(speech) = shared.borrow().as_ref() {
                return Ok(Rc::clone(speech));
            }
            // Only an open that worked is kept; a failed one is tried again.
            let speech = Rc::new(Self::open()?);
            *shared.borrow_mut() = Some(Rc::clone(&speech));
            Ok(speech)
        })
    }

    /// Lets `ear` hear its utterances, forgetting voicers gone.
    fn listen(&self, ear: &Arc<Ear>) {
        let mut ears = self.ears.lock().unwrap_or_else(PoisonError::into_inner);
        ears.retain(|e| e.strong_count() > 0);
        ears.push(Arc::downgrade(ear));
    }
}

pub struct System {
    speech: Rc<Speech>,
    /// Its own handle on the speech, which setting needs.
    tts: tts::Tts,
    me: u64,
    ear: Arc<Ear>,
    /// The engine's name for what is being said, and ours.
    current: Option<(String, u64)>,
    said_at: Instant,
    /// How long what is being said should take, when nothing tells.
    expect: Duration,
    voice: Option<String>,
    tone: Tone,
}

impl System {
    pub fn new(wake: Waker) -> Result<Self, Fault> {
        let speech = Speech::get()
            .map_err(|e| Fault::Unavailable(format!("System voices aren't available: {e}")))?;
        let ear = Arc::new(Ear {
            heard: Mutex::new(Vec::new()),
            wake,
        });
        speech.listen(&ear);
        Ok(Self {
            tts: speech.tts.clone(),
            speech,
            me: NEXT.fetch_add(1, Ordering::Relaxed),
            ear,
            current: None,
            said_at: Instant::now(),
            expect: Duration::ZERO,
            voice: None,
            tone: Tone::default(),
        })
    }

    fn features(&self) -> &tts::Features {
        &self.speech.features
    }

    /// Gives the speech this voicer's voice and tone, once another had it.
    fn tune(&mut self) {
        let speech = Rc::clone(&self.speech);
        let picked = self.voice.as_deref();
        match picked.and_then(|id| speech.voices.iter().find(|v| v.id() == id)) {
            Some(v) => {
                if self.tts.set_voice(v).is_ok() {
                    speech.voiced.set(true);
                }
            }
            None if speech.voiced.get() => self.unvoice(),
            None => {}
        }
        let tone = self.tone;
        self.apply(tone);
        self.speech.tuned.set(self.me);
    }

    /// Back to the system's default voice, which no call names.
    fn unvoice(&mut self) {
        if ONCE {
            // Apple's speech can't open again: its usual voice stands in.
            let voices = &self.speech.voices;
            let pairs = voices.iter().map(|v| (v.id(), v.language().to_string()));
            let usual = locale().and_then(|l| usual(pairs, &l));
            if let Some(v) = usual.and_then(|i| voices.get(i)) {
                let _ = self.tts.set_voice(v);
            }
            return;
        }
        // Only a fresh start finds the default again.
        let Ok(speech) = Speech::open() else {
            return;
        };
        speech.listen(&self.ear);
        self.tts = speech.tts.clone();
        self.speech = Rc::new(speech);
    }

    /// Sets what the system offers of `tone`; the rest is left as it is.
    fn apply(&mut self, tone: Tone) {
        let features = self.features();
        let (rate, pitch, volume) = (features.rate, features.pitch, features.volume);
        if rate {
            let _ = self.tts.set_rate(self.rate(tone.rate));
        }
        if pitch {
            let range = (
                self.tts.min_pitch(),
                self.tts.normal_pitch(),
                self.tts.max_pitch(),
            );
            let _ = self.tts.set_pitch(scaled(tone.pitch, range, PITCHES));
        }
        if volume {
            let (min, max) = (self.tts.min_volume(), self.tts.max_volume());
            let _ = self
                .tts
                .set_volume(min + (max - min) * tone.volume.clamp(0.0, 1.0));
        }
    }

    fn rate(&self, share: f32) -> f32 {
        let (min, normal, mut max) = (
            self.tts.min_rate(),
            self.tts.normal_rate(),
            self.tts.max_rate(),
        );
        // Windows takes a plain multiple of normal speed.
        if cfg!(windows) {
            return share.clamp(min, max);
        }
        // Past 1 the Mac's synthesizer goes no faster.
        if cfg!(target_os = "macos") {
            max = max.min(1.0);
        }
        scaled(share, (min, normal, max), RATES)
    }

    /// The speech says what this voicer said last, not another's.
    fn speaking(&self) -> bool {
        self.speech.speaker.get() == self.me
    }
}

/// The user's language tag, as `en-US`.
#[cfg(target_vendor = "apple")]
fn locale() -> Option<String> {
    sys_locale::get_locale()
}

#[cfg(not(target_vendor = "apple"))]
fn locale() -> Option<String> {
    None
}

/// Which of `voices` (id, language) the system would pick for `locale`.
fn usual(voices: impl Iterator<Item = (String, String)>, locale: &str) -> Option<usize> {
    let locale = locale.replace('_', "-");
    let language = locale.split('-').next().unwrap_or_default().to_owned();
    // Novelty voices share it: compact ones, like Samantha, come first.
    let rank = |(id, tag): &(String, String)| {
        let same = tag.eq_ignore_ascii_case(&locale);
        let kin = tag.split('-').next().unwrap_or_default();
        let kin = kin.eq_ignore_ascii_case(&language);
        (!kin, !same, !id.contains(".voice.compact."))
    };
    let (i, best) = voices.enumerate().min_by_key(|(_, v)| rank(v))?;
    (!rank(&best).0).then_some(i)
}

impl Voicer for System {
    fn caps(&self) -> Caps {
        let features = self.features();
        Caps {
            rate: features.rate,
            pitch: features.pitch,
            volume: features.volume,
            voices: features.voice && !self.speech.voices.is_empty(),
            pause: false,
        }
    }

    fn voices(&self) -> Vec<Voice> {
        let voice = |v: &tts::Voice| Voice {
            id: v.id(),
            name: v.name(),
            language: v.language().to_string(),
        };
        self.speech.voices.iter().map(voice).collect()
    }

    fn set_voice(&mut self, id: Option<&str>) {
        if self.voice.as_deref() == id || !self.features().voice {
            return;
        }
        let known = id.filter(|id| self.speech.voices.iter().any(|v| v.id() == *id));
        self.voice = known.map(str::to_owned);
        self.tune();
    }

    fn set_tone(&mut self, tone: Tone) {
        self.tone = tone;
        self.tune();
    }

    fn say(&mut self, id: u64, text: &str) -> Result<(), Fault> {
        // A voicer sharing the speech may have tuned it since.
        if self.speech.tuned.get() != self.me {
            self.tune();
        }
        let said = self.tts.speak(text, true);
        let said = said.map_err(|e| Fault::Failed(format!("The system voice failed: {e}")))?;
        self.speech.speaker.set(self.me);
        let name = said.map(|u| format!("{u:?}")).unwrap_or_default();
        self.current = Some((name, id));
        self.said_at = Instant::now();
        let chars = text.chars().count() as f32;
        let secs = chars / (PACE * self.tone.rate.max(0.1));
        self.expect = Duration::from_secs_f32(secs.clamp(0.3, 120.0));
        Ok(())
    }

    fn stop(&mut self) {
        self.current = None;
        // What another voicer says is not this one's to stop.
        if self.speaking() {
            let _ = self.tts.stop();
        }
    }

    fn events(&mut self) -> Vec<Event> {
        let heard = {
            let mut heard = self
                .ear
                .heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut *heard)
        };
        // Another voicer spoke over this one: what it said was cut off.
        if !self.speaking() {
            self.current = None;
        }
        let mut out = Vec::new();
        for (begun, name) in heard {
            let Some((ours, id)) = self.current.clone() else {
                continue;
            };
            // A late word about something already cut off is dropped.
            if name != ours {
                continue;
            }
            match begun {
                true => out.push(Event::Started(id)),
                false => {
                    out.push(Event::Finished(id));
                    self.current = None;
                }
            }
        }
        if !self.features().utterance_callbacks
            && let Some((_, id)) = self.current.clone()
        {
            // Some time to start, then silence means it ended.
            let elapsed = self.said_at.elapsed();
            let quiet = match self.features().is_speaking {
                true => {
                    elapsed > Duration::from_millis(400)
                        && matches!(self.tts.is_speaking(), Ok(false))
                }
                false => elapsed > self.expect,
            };
            if quiet {
                out.push(Event::Finished(id));
                self.current = None;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> impl Iterator<Item = (String, String)> {
        let owned: Vec<(String, String)> = (list.iter())
            .map(|(id, tag)| ((*id).to_owned(), (*tag).to_owned()))
            .collect();
        owned.into_iter()
    }

    #[test]
    fn the_usual_voice_is_the_compact_one_in_the_users_language() {
        let voices = [
            ("com.apple.speech.synthesis.voice.Albert", "en-US"),
            ("com.apple.voice.compact.en-GB.Daniel", "en-GB"),
            ("com.apple.eloquence.en-US.Eddy", "en-US"),
            ("com.apple.voice.compact.en-US.Samantha", "en-US"),
            ("com.apple.voice.compact.fr-FR.Thomas", "fr-FR"),
        ];
        assert_eq!(usual(pairs(&voices), "en_US"), Some(3));
        assert_eq!(usual(pairs(&voices), "en-GB"), Some(1));
        // A region with no voice of its own takes its language's.
        assert_eq!(usual(pairs(&voices), "fr-CA"), Some(4));
        assert_eq!(usual(pairs(&voices), "de-DE"), None);
        assert_eq!(usual(pairs(&[]), "en-US"), None);
    }

    /// Only where the system's speech runs in CI: the remote `system` task.
    #[cfg(any(target_vendor = "apple", windows))]
    #[test]
    fn system_voices_open_again_and_side_by_side() {
        let wake: Waker = Arc::new(|| {});
        let first = match System::new(Arc::clone(&wake)) {
            Ok(first) => first,
            Err(e) => panic!("{e}"),
        };
        let listed = first.voices().len();
        println!("system voices: {listed} listed");
        drop(first);
        // Read aloud stopped and started, and the settings tab reopened.
        for _ in 0..3 {
            match System::new(Arc::clone(&wake)) {
                Ok(again) => assert_eq!(again.voices().len(), listed),
                Err(e) => panic!("opening again: {e}"),
            }
        }
        // The settings tab open while a book is read.
        let mut tab = System::new(Arc::clone(&wake)).ok().unwrap();
        let mut book = System::new(wake).ok().unwrap();
        let voices = tab.voices();
        tab.set_voice(voices.first().map(|v| v.id.as_str()));
        book.set_voice(None);
        assert!(book.say(1, "A sentence read from the book.").is_ok());
        // Closing the tab leaves the book's sentence alone.
        tab.stop();
        assert!(book.speaking() && !tab.speaking());
        assert!(tab.say(2, "A sample.").is_ok());
        // Shared, a sample cuts the book's sentence off; apart, both speak.
        assert!(tab.speaking() && book.speaking() != ONCE);
        // The book never takes the tab's sample for its own sentence.
        let heard = book.events();
        let theirs = [Event::Started(2), Event::Finished(2)];
        assert!(heard.iter().all(|e| !theirs.contains(e)), "{heard:?}");
        assert_eq!(book.current.is_none(), ONCE);
        tab.stop();
        book.stop();
        println!("system voices: open again and side by side");
    }
}
