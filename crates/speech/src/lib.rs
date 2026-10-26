//! Reading aloud: sentences, voices, and swappable engines, each a [`Voicer`].
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

#[cfg(not(target_arch = "wasm32"))]
mod kitten;
#[cfg(not(target_arch = "wasm32"))]
mod kokoro;
#[cfg(not(target_arch = "wasm32"))]
mod local;
#[cfg(target_vendor = "apple")]
mod mac_voices;
mod models;
#[cfg(not(target_arch = "wasm32"))]
mod phonon;
#[cfg(not(target_arch = "wasm32"))]
mod piper;
mod script;
mod sentences;
mod shelf;
mod silent;
#[cfg(not(target_arch = "wasm32"))]
mod studio;
#[cfg(not(target_arch = "wasm32"))]
mod system;
#[cfg(not(target_arch = "wasm32"))]
mod timing;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use models::{
    AIDS, Build, Gender, KITTEN, KOKORO, Knob, MODELS, Model, ModelFile, ModelVoice, PHONON, PIPER,
    PRONUNCIATION,
};
pub use script::{Glyph, Script, Sentence, Spot, Word};
pub use sentences::{LONGEST, split};
#[cfg(not(target_arch = "wasm32"))]
pub use shelf::Lexicons;
pub use shelf::{Item, Shelf, Stage};

/// Wakes the window when an engine has news: a sentence ended, say.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The engine the system itself provides: nothing to download.
pub const SYSTEM: &str = "system";
/// Says nothing, ending each sentence at once: for tests and shots.
pub const SILENT: &str = "silent";

/// A voice an engine offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Voice {
    /// The engine's own name for it, kept in settings.
    pub id: String,
    pub name: String,
    /// A language tag, such as `en-GB`.
    pub language: String,
}

/// How a voice sounds; each is a share of the engine's own normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    /// Speed: 1 is normal, 2 twice as fast.
    pub rate: f32,
    /// 1 is the voice's own pitch.
    pub pitch: f32,
    /// 0 is silent, 1 full.
    pub volume: f32,
}

impl Default for Tone {
    fn default() -> Self {
        Self {
            rate: 1.0,
            pitch: 1.0,
            volume: 1.0,
        }
    }
}

/// What voice models hold in memory now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Names of the models loaded, with their build: `Kokoro (Small)`.
    pub models: Vec<String>,
    /// Bytes of speech kept to say again.
    pub speech: usize,
}

/// What an engine can change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caps {
    pub rate: bool,
    pub pitch: bool,
    pub volume: bool,
    pub voices: bool,
    /// Pauses mid-sentence; else a pause says the sentence again.
    pub pause: bool,
}

/// News from an engine about the sentences it was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Started(u64),
    Finished(u64),
}

/// Where an engine is in the sentence it says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heard {
    pub id: u64,
    /// Seconds of it heard.
    pub seconds: f64,
    /// The byte of the word heard in its text, when the model knows.
    pub word: Option<usize>,
}

/// Why an engine can't speak.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The engine isn't on this system, or wouldn't start.
    Unavailable(String),
    /// It started but failed to say something.
    Failed(String),
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

/// One engine, ready to speak.
pub trait Voicer {
    fn caps(&self) -> Caps;
    fn voices(&self) -> Vec<Voice>;
    /// Moves on whenever [`Self::voices`] changes, as voices turn out mute.
    fn voices_seen(&self) -> u64 {
        0
    }
    /// Picks a voice by its id; none, or an unknown one, is the default.
    fn set_voice(&mut self, id: Option<&str>);
    fn set_tone(&mut self, tone: Tone);
    /// Says `text` now, cutting off whatever was being said.
    fn say(&mut self, id: u64, text: &str) -> Result<(), Fault>;
    fn stop(&mut self);
    /// Pauses mid-sentence where [`Caps::pause`] allows; true if it did.
    fn pause(&mut self) -> bool {
        false
    }
    /// Goes on from a pause; true if it did.
    fn resume(&mut self) -> bool {
        false
    }
    /// True while the engine reads its model from disk, before speaking.
    fn loading(&self) -> bool {
        false
    }
    /// Sentences likely said next, in order, to get ready ahead.
    fn prepare(&mut self, _texts: &[&str]) {}
    /// Sets the model's own [`Knob`]s by id; one not given is 1.
    fn set_knobs(&mut self, _values: &BTreeMap<String, f32>) {}
    /// What happened since the last call, oldest first.
    fn events(&mut self) -> Vec<Event>;
    /// Where it is in the sentence said, as of the last [`Self::events`].
    fn heard(&self) -> Option<Heard> {
        None
    }
}

/// A line for trying a voice, in its language where we have one.
pub fn sample(language: &str) -> &'static str {
    let primary = language.split(['-', '_']).next().unwrap_or_default();
    let found = SAMPLES
        .iter()
        .find(|(tag, _)| primary.eq_ignore_ascii_case(tag));
    found.map_or(SAMPLE, |(_, line)| line)
}

const SAMPLE: &str = "This is how your books will sound when read aloud.";
const SAMPLES: [(&str, &str); 13] = [
    ("en", SAMPLE),
    ("de", "So klingen Ihre Bücher, wenn sie vorgelesen werden."),
    ("es", "Así sonarán tus libros cuando se lean en voz alta."),
    (
        "fr",
        "Voici comment vos livres sonneront, lus à voix haute.",
    ),
    ("hi", "ज़ोर से पढ़ी जाने पर आपकी किताबें ऐसी सुनाई देंगी।"),
    (
        "it",
        "Ecco come suoneranno i tuoi libri letti ad alta voce.",
    ),
    ("ja", "本を読み上げると、このように聞こえます。"),
    ("ko", "책을 소리 내어 읽으면 이렇게 들립니다."),
    ("my", "သင့်စာအုပ်များကို အသံထွက်ဖတ်သောအခါ ဤသို့ ကြားရပါမည်။"),
    ("nl", "Zo klinken je boeken als ze worden voorgelezen."),
    (
        "pt",
        "É assim que os seus livros vão soar lidos em voz alta.",
    ),
    ("ru", "Так будут звучать ваши книги при чтении вслух."),
    ("zh", "这就是您的书被朗读时的声音。"),
];

/// The models with an engine, by id.
const RUNNABLE: [&str; 4] = [KOKORO.id, PIPER.id, KITTEN.id, PHONON.id];

/// The engines to pick from, by id and name: the system's, then models ready.
pub fn engines(shelf: &Shelf) -> Vec<(&'static str, &'static str)> {
    // A browser build runs no model yet.
    let runs = |m: &&Model| cfg!(not(target_arch = "wasm32")) && RUNNABLE.contains(&m.id);
    let models = shelf
        .ready()
        .into_iter()
        .filter(runs)
        .map(|m| (m.id, m.name));
    std::iter::once((SYSTEM, "System voices"))
        .chain(models)
        .collect()
}

/// Starts engine `id`, a model's in `build` if here; `wake` hears events.
pub fn open(
    id: &str,
    build: Option<&str>,
    shelf: &Shelf,
    wake: Waker,
) -> Result<Box<dyn Voicer>, Fault> {
    match id {
        #[cfg(not(target_arch = "wasm32"))]
        SYSTEM => Ok(Box::new(system::System::new(wake)?)),
        SILENT => Ok(Box::new(silent::Silent::default())),
        #[cfg(not(target_arch = "wasm32"))]
        id if RUNNABLE.contains(&id) => {
            let ready = shelf.ready().into_iter().find(|m| m.id == id);
            let folder = ready.and_then(|m| shelf.folder(m));
            let (Some(model), Some(folder)) = (ready, folder) else {
                let name = shelf.model(id).map_or(id, |m| m.name);
                return Err(Fault::Unavailable(format!("{name} isn't downloaded")));
            };
            if model.builds.is_empty() {
                return Ok(Box::new(open_piper(model, folder, shelf, wake)?));
            }
            let build = shelf.build(model, build).ok_or_else(|| {
                Fault::Unavailable(format!("{} has no build downloaded", model.name))
            })?;
            let name: Arc<str> = label(model, build).into();
            let setup = |speeds| local::Setup {
                name: Arc::clone(&name),
                voices: voices_here(model, shelf),
                speeds,
                knobs: model.knobs,
            };
            let studio = shelf.studio().clone();
            let engine = match model.id {
                id if id == PHONON.id => {
                    let voices = (model.voices.iter())
                        .filter(|v| shelf.has(model, v.id))
                        .filter_map(|v| Some((v.id.to_owned(), folder.join(v.file?.path))));
                    let source = phonon::Source {
                        config: folder.join("config.json"),
                        weights: folder.join(build.file.path),
                        tokenizer: folder.join("tokenizer.json"),
                        voices: voices.collect(),
                        name: Arc::clone(&name),
                    };
                    let synth = phonon::Phonon::new(source, studio.clone());
                    local::Engine::open(synth, setup(phonon::SPEEDS), studio, wake)?
                }
                id if id == KITTEN.id => {
                    let source = kitten::Source {
                        weights: folder.join(build.file.path),
                        voices: folder.join(build.extra.map_or("", |f| f.path)),
                        folder,
                        name: Arc::clone(&name),
                        nano: build.id.starts_with("nano"),
                    };
                    let synth = kitten::Kitten::new(source, studio.clone());
                    local::Engine::open(synth, setup(kitten::SPEEDS), studio, wake)?
                }
                _ => {
                    let source = kokoro::Source {
                        weights: folder.join(build.file.path),
                        folder,
                        name: Arc::clone(&name),
                    };
                    let synth = kokoro::Kokoro::new(source, studio.clone());
                    local::Engine::open(synth, setup(kokoro::SPEEDS), studio, wake)?
                }
            };
            Ok(Box::new(engine))
        }
        _ => {
            let _ = (wake, shelf, build);
            Err(Fault::Unavailable(format!("No speech engine named {id}")))
        }
    }
}

/// Piper in its downloaded voices, each a model of its own.
#[cfg(not(target_arch = "wasm32"))]
fn open_piper(
    model: &'static Model,
    folder: std::path::PathBuf,
    shelf: &Shelf,
    wake: Waker,
) -> Result<local::Engine, Fault> {
    let mut files = std::collections::HashMap::new();
    for v in model.voices.iter().filter(|v| shelf.has(model, v.id)) {
        let (Some(weights), Some(settings)) = (v.file, v.extra) else {
            continue;
        };
        let found = piper::Files {
            weights: folder.join(weights.path),
            settings: folder.join(settings.path),
            name: format!("{} ({})", model.name, v.name).into(),
        };
        files.insert(v.id.to_owned(), found);
    }
    let setup = local::Setup {
        name: model.name.into(),
        voices: voices_here(model, shelf),
        speeds: piper::SPEEDS,
        knobs: model.knobs,
    };
    let studio = shelf.studio().clone();
    let synth = piper::Piper::new(folder, files, studio.clone());
    local::Engine::open(synth, setup, studio, wake)
}

/// The voices of `model` downloaded, the one that came with it first.
#[cfg(not(target_arch = "wasm32"))]
fn voices_here(model: &Model, shelf: &Shelf) -> Vec<Voice> {
    let voices = (model.voices.iter())
        .filter(|v| shelf.has(model, v.id))
        .map(|v| Voice {
            id: v.id.to_owned(),
            name: v.name.to_owned(),
            language: v.language.to_owned(),
        });
    let mut voices: Vec<Voice> = voices.collect();
    voices.sort_by_key(|v| v.id != model.first_voice);
    voices
}

/// A model's name with its build's, as `Kokoro (Small)`.
pub fn label(model: &Model, build: &Build) -> String {
    format!("{} ({})", model.name, build.name)
}

/// Samples a second `engine` keeps `voice` at;
/// None when not kept or downloaded.
#[cfg(not(target_arch = "wasm32"))]
pub fn speech_rate(engine: &str, voice: Option<&str>, shelf: &Shelf) -> Option<u32> {
    let model = shelf.ready().into_iter().find(|m| m.id == engine)?;
    match model.id {
        id if id == KOKORO.id || id == KITTEN.id => Some(mareader_kokoro::SAMPLE_RATE),
        id if id == PHONON.id => Some(mareader_phonon::SAMPLE_RATE),
        id if id == PIPER.id => {
            // Each Piper voice says its own rate in its settings.
            let picked = voice.filter(|v| shelf.has(model, v));
            let voice = model.voice(picked.unwrap_or(model.first_voice))?;
            let settings = shelf.folder(model)?.join(voice.extra?.path);
            let bytes = std::fs::read(settings).ok()?;
            let config = mareader_vits::Config::parse(&bytes).ok()?;
            Some(config.audio.sample_rate)
        }
        _ => None,
    }
}

/// No speech is kept on the web.
#[cfg(target_arch = "wasm32")]
pub fn speech_rate(_engine: &str, _voice: Option<&str>, _shelf: &Shelf) -> Option<u32> {
    None
}

/// The build `engine` runs: `wanted` if downloaded, else the best one.
pub fn build_of(engine: &str, wanted: Option<&str>, shelf: &Shelf) -> Option<&'static str> {
    let model = shelf.model(engine)?;
    shelf.build(model, wanted).map(|b| b.id)
}

/// Maps `share` (`bottom..1..top`) onto `min..normal..max`.
pub fn scaled(share: f32, (min, normal, max): (f32, f32, f32), (bottom, top): (f32, f32)) -> f32 {
    let value = if share >= 1.0 {
        normal + (max - normal) * ((share - 1.0) / (top - 1.0).max(f32::EPSILON))
    } else {
        normal - (normal - min) * ((1.0 - share) / (1.0 - bottom).max(f32::EPSILON))
    };
    value.clamp(min.min(max), max.max(min))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_map_onto_each_engine_range() {
        // Speech Dispatcher: -100 to 100 around 0.
        assert_eq!(scaled(1.0, (-100.0, 0.0, 100.0), (0.5, 3.0)), 0.0);
        assert_eq!(scaled(3.0, (-100.0, 0.0, 100.0), (0.5, 3.0)), 100.0);
        assert_eq!(scaled(0.5, (-100.0, 0.0, 100.0), (0.5, 3.0)), -100.0);
        assert_eq!(scaled(2.0, (-100.0, 0.0, 100.0), (0.5, 3.0)), 50.0);
        // Out of range stays in range.
        assert_eq!(scaled(9.0, (0.1, 0.5, 1.0), (0.5, 3.0)), 1.0);
    }

    #[test]
    fn the_silent_engine_starts_then_ends_each_sentence() {
        let wake: Waker = Arc::new(|| {});
        let Ok(mut v) = open(SILENT, None, &Shelf::default(), wake) else {
            panic!("the silent engine always opens");
        };
        assert_eq!(v.say(7, "Hello."), Ok(()));
        assert_eq!(v.events(), [Event::Started(7)]);
        assert!(v.pause());
        assert!(v.events().is_empty(), "a pause holds the end back");
        assert!(v.resume());
        assert_eq!(v.events(), [Event::Finished(7)]);
        assert!(v.events().is_empty());
    }

    #[test]
    fn samples_follow_the_voice_language() {
        assert_eq!(sample("fr-FR"), sample("fr"));
        assert_ne!(sample("fr"), sample("en"));
        assert_eq!(
            sample("tlh"),
            sample("en-GB"),
            "unknown falls back to English"
        );
        assert_eq!(sample(""), SAMPLE);
    }

    #[test]
    fn an_unknown_engine_is_unavailable() {
        let wake: Waker = Arc::new(|| {});
        let shelf = Shelf::default();
        assert!(matches!(
            open("nope", None, &shelf, wake.clone()),
            Err(Fault::Unavailable(_))
        ));
        for model in RUNNABLE {
            assert!(matches!(
                open(model, Some("small"), &shelf, wake.clone()),
                Err(Fault::Unavailable(_))
            ));
        }
        assert_eq!(engines(&shelf), [(SYSTEM, "System voices")]);
    }
}
