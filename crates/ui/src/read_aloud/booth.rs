//! The voice tabs' booth: an engine opened to list and try voices.

use std::sync::Arc;

use egui::Context;
use mareader_settings::ReadAloudSettings;
use mareader_speech::{Caps, Event, Shelf, Voice, Voicer, Waker};

use super::TICK;

/// Opened while a voice tab shows; let go once it closes.
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
    /// The voice asked for by name, not the one picked.
    tried: Option<String>,
    said: u64,
}

impl Booth {
    /// Opens `engine` in its picked build unless open, or failed, already.
    pub fn open(&mut self, ctx: &Context, engine: &str, s: &ReadAloudSettings, shelf: &Shelf) {
        let tried = self.voicer.is_some() || self.fault.is_some();
        let wanted = s.build_for(engine);
        let build = mareader_speech::build_of(engine, wanted, shelf);
        if self.engine == engine && self.build == build && tried {
            return;
        }
        self.start(ctx, engine.to_owned(), build, shelf);
    }

    fn start(&mut self, ctx: &Context, engine: String, build: Option<&'static str>, shelf: &Shelf) {
        self.close();
        self.engine = engine;
        self.build = build;
        let ctx = ctx.clone();
        let wake: Waker = Arc::new(move || ctx.request_repaint());
        match mareader_speech::open(&self.engine, build, shelf, wake) {
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
        let picked = settings.voice_for(&self.engine);
        self.say_sample(settings, picked);
        self.tried = None;
    }

    /// Says a line in the voice `id`, in the tone `settings` pick.
    pub fn try_voice_of(
        &mut self,
        ctx: &Context,
        settings: &ReadAloudSettings,
        shelf: &Shelf,
        id: &str,
    ) {
        // Downloaded since it opened: the engine lists it once reopened.
        if self.ready() && !self.voices.iter().any(|v| v.id == id) {
            self.start(ctx, self.engine.clone(), self.build, shelf);
        }
        self.say_sample(settings, Some(id));
        self.tried = self.trying.map(|_| id.to_owned());
    }

    /// Says `text` in the voice and tone `settings` pick.
    pub fn say(&mut self, settings: &ReadAloudSettings, text: &str) {
        let picked = settings.voice_for(&self.engine);
        self.speak(settings, picked, text);
        self.tried = None;
    }

    fn say_sample(&mut self, settings: &ReadAloudSettings, picked: Option<&str>) {
        let language = picked
            .and_then(|id| self.voices.iter().find(|v| v.id == id))
            .map_or("en", |v| v.language.as_str());
        let line = mareader_speech::sample(language);
        self.speak(settings, picked, line);
    }

    fn speak(&mut self, settings: &ReadAloudSettings, picked: Option<&str>, line: &str) {
        let Some(v) = self.voicer.as_mut() else {
            return;
        };
        v.set_voice(picked);
        v.set_tone(super::spoken::tone(settings));
        v.set_knobs(&settings.knobs_for(&self.engine));
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
        self.tried = None;
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
            self.tried = None;
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

    /// The voice being said, when asked for by name.
    pub fn trying_voice(&self) -> Option<&str> {
        self.trying.and(self.tried.as_deref())
    }

    /// Samples said since the window opened.
    pub fn samples(&self) -> u64 {
        self.said
    }
}
