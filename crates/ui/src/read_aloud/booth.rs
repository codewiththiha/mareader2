//! The settings tab's voices: an engine opened to list and try them.

use std::sync::Arc;

use egui::Context;
use mareader_settings::ReadAloudSettings;
use mareader_speech::{Caps, Event, Shelf, Voice, Voicer, Waker};

use super::TICK;

/// Opened while the Read aloud tab shows; let go once it closes.
#[derive(Default)]
pub struct Booth {
    engine: String,
    /// The model build opened, for a model's engine.
    build: Option<&'static str>,
    voicer: Option<Box<dyn Voicer>>,
    voices: Vec<Voice>,
    /// The engine's count of changes to its voices when last listed.
    seen: u64,
    caps: Caps,
    fault: Option<String>,
    /// The sample being said, by its id; ids count every sample.
    trying: Option<u64>,
    said: u64,
}

impl Booth {
    /// Opens the engine and build picked unless open, or failed, already.
    pub fn open(&mut self, ctx: &Context, s: &ReadAloudSettings, shelf: &Shelf) {
        let tried = self.voicer.is_some() || self.fault.is_some();
        let build = mareader_speech::build_of(&s.engine, s.build(), shelf);
        if self.engine == s.engine && self.build == build && tried {
            return;
        }
        self.close();
        self.engine.clone_from(&s.engine);
        self.build = build;
        let ctx = ctx.clone();
        let wake: Waker = Arc::new(move || ctx.request_repaint());
        match mareader_speech::open(&s.engine, build, shelf, wake) {
            Ok(voicer) => {
                self.caps = voicer.caps();
                self.voicer = Some(voicer);
                self.relist();
            }
            Err(fault) => self.fault = Some(fault.to_string()),
        }
    }

    /// Lists the engine's voices again, by language then name.
    fn relist(&mut self) {
        let Some(v) = self.voicer.as_ref() else {
            return;
        };
        self.seen = v.voices_seen();
        self.voices = v.voices();
        (self.voices)
            .sort_by(|a, b| (a.language.as_str(), a.name.as_str()).cmp(&(&b.language, &b.name)));
    }

    /// Stops any sample and lets the engine go.
    pub fn close(&mut self) {
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        let said = self.said;
        *self = Self::default();
        self.said = said;
    }

    pub fn is_open(&self) -> bool {
        self.voicer.is_some() || self.fault.is_some()
    }

    /// Open and able to speak.
    pub fn ready(&self) -> bool {
        self.voicer.is_some()
    }

    /// Says a line in the voice and tone `settings` pick.
    pub fn try_voice(&mut self, settings: &ReadAloudSettings) {
        let picked = settings.voices.get(&self.engine).map(String::as_str);
        let language = picked
            .and_then(|id| self.voices.iter().find(|v| v.id == id))
            .map_or("en", |v| v.language.as_str());
        let line = mareader_speech::sample(language);
        let Some(v) = self.voicer.as_mut() else {
            return;
        };
        v.set_voice(picked);
        v.set_tone(super::spoken::tone(settings));
        v.set_knobs(
            &settings
                .knobs
                .get(&self.engine)
                .cloned()
                .unwrap_or_default(),
        );
        self.said = self.said.wrapping_add(1);
        match v.say(self.said, line) {
            Ok(()) => {
                self.trying = Some(self.said);
                self.fault = None;
            }
            Err(fault) => {
                self.trying = None;
                self.fault = Some(fault.to_string());
            }
        }
    }

    pub fn stop_trying(&mut self) {
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.trying = None;
    }

    /// Each frame while open: hears the sample end.
    pub fn tick(&mut self, ctx: &Context) {
        // A voice found mute, here or by the book's reading, leaves the list.
        if self
            .voicer
            .as_ref()
            .is_some_and(|v| v.voices_seen() != self.seen)
        {
            self.relist();
        }
        let (Some(v), Some(id)) = (self.voicer.as_mut(), self.trying) else {
            return;
        };
        if v.events().contains(&Event::Finished(id)) {
            self.trying = None;
        } else {
            // Some systems only tell when asked.
            ctx.request_repaint_after(TICK);
        }
    }

    /// The engine open, or last tried.
    pub fn engine(&self) -> &str {
        &self.engine
    }

    /// Its voices, by language then name.
    pub fn voices(&self) -> &[Voice] {
        &self.voices
    }

    pub fn caps(&self) -> Caps {
        self.caps
    }

    /// Why the engine won't open, or won't speak.
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }

    /// True while the engine reads its model, before the sample sounds.
    pub fn loading(&self) -> bool {
        self.voicer.as_ref().is_some_and(|v| v.loading())
    }

    /// True while a sample is being said.
    pub fn trying(&self) -> bool {
        self.trying.is_some()
    }

    /// Samples said since the window opened.
    pub fn samples(&self) -> u64 {
        self.said
    }
}
