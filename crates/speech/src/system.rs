//! System voices via `tts`: kept on the window's thread, never asked its voice.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::{Caps, Event, Fault, Tone, Voice, Voicer, Waker, scaled};

/// The speeds and pitches the reader offers, as shares of normal.
const RATES: (f32, f32) = (0.5, 3.0);
const PITCHES: (f32, f32) = (0.5, 2.0);
/// Characters a second at normal speed, when nothing reports an end.
const PACE: f32 = 14.0;

/// An utterance began or ended, by the engine's name for it.
type Heard = Arc<Mutex<Vec<(bool, String)>>>;

pub struct System {
    tts: tts::Tts,
    features: tts::Features,
    /// Its voices, read once: some systems are slow to list them.
    voices: Vec<tts::Voice>,
    heard: Heard,
    wake: Waker,
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
        let tts = tts::Tts::default()
            .map_err(|e| Fault::Unavailable(format!("System voices aren't available: {e}")))?;
        let features = tts.supported_features();
        let voices = match features.voice {
            true => tts.voices().unwrap_or_default(),
            false => Vec::new(),
        };
        let mut system = Self {
            tts,
            features,
            voices,
            heard: Arc::new(Mutex::new(Vec::new())),
            wake,
            current: None,
            said_at: Instant::now(),
            expect: Duration::ZERO,
            voice: None,
            tone: Tone::default(),
        };
        system.listen();
        Ok(system)
    }

    /// Hears each utterance begin and end, waking the window for it.
    fn listen(&mut self) {
        if !self.features.utterance_callbacks {
            return;
        }
        let hear = |begun: bool| {
            let heard = Arc::clone(&self.heard);
            let wake = Arc::clone(&self.wake);
            Box::new(move |id: tts::UtteranceId| {
                let mut heard = heard.lock().unwrap_or_else(PoisonError::into_inner);
                heard.push((begun, format!("{id:?}")));
                drop(heard);
                wake();
            }) as Box<dyn FnMut(tts::UtteranceId)>
        };
        // An engine that refuses callbacks falls back to watching.
        let begin = self.tts.on_utterance_begin(Some(hear(true)));
        let end = self.tts.on_utterance_end(Some(hear(false)));
        if begin.is_err() || end.is_err() {
            self.features.utterance_callbacks = false;
        }
    }

    /// Starts afresh, back on the system's default voice.
    fn reopen(&mut self) {
        let Ok(tts) = tts::Tts::default() else {
            return;
        };
        self.tts = tts;
        self.listen();
        let tone = self.tone;
        self.set_tone(tone);
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
}

impl Voicer for System {
    fn caps(&self) -> Caps {
        Caps {
            rate: self.features.rate,
            pitch: self.features.pitch,
            volume: self.features.volume,
            voices: self.features.voice && !self.voices.is_empty(),
            pause: false,
        }
    }

    fn voices(&self) -> Vec<Voice> {
        let voice = |v: &tts::Voice| Voice {
            id: v.id(),
            name: v.name(),
            language: v.language().to_string(),
        };
        self.voices.iter().map(voice).collect()
    }

    fn set_voice(&mut self, id: Option<&str>) {
        if self.voice.as_deref() == id || !self.features.voice {
            return;
        }
        let was = self.voice.take();
        match id.and_then(|id| self.voices.iter().find(|v| v.id() == id)) {
            Some(v) => {
                if self.tts.set_voice(v).is_ok() {
                    self.voice = Some(v.id());
                }
            }
            // Only a fresh start finds the default again.
            None if was.is_some() => self.reopen(),
            None => {}
        }
    }

    fn set_tone(&mut self, tone: Tone) {
        self.tone = tone;
        // A setting the system lacks is left as it is.
        if self.features.rate {
            let _ = self.tts.set_rate(self.rate(tone.rate));
        }
        if self.features.pitch {
            let range = (
                self.tts.min_pitch(),
                self.tts.normal_pitch(),
                self.tts.max_pitch(),
            );
            let _ = self.tts.set_pitch(scaled(tone.pitch, range, PITCHES));
        }
        if self.features.volume {
            let (min, max) = (self.tts.min_volume(), self.tts.max_volume());
            let _ = self
                .tts
                .set_volume(min + (max - min) * tone.volume.clamp(0.0, 1.0));
        }
    }

    fn say(&mut self, id: u64, text: &str) -> Result<(), Fault> {
        let said = self.tts.speak(text, true);
        let said = said.map_err(|e| Fault::Failed(format!("The system voice failed: {e}")))?;
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
        let _ = self.tts.stop();
    }

    fn events(&mut self) -> Vec<Event> {
        let heard = {
            let mut heard = self.heard.lock().unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut *heard)
        };
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
        if !self.features.utterance_callbacks
            && let Some((_, id)) = self.current.clone()
        {
            // Some time to start, then silence means it ended.
            let elapsed = self.said_at.elapsed();
            let quiet = match self.features.is_speaking {
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
