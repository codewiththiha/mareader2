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
mod kokoro;
mod models;
mod script;
mod sentences;
mod shelf;
mod silent;
#[cfg(not(target_arch = "wasm32"))]
mod studio;
#[cfg(not(target_arch = "wasm32"))]
mod system;

use std::sync::Arc;

pub use models::{KOKORO, MODELS, Model, ModelFile, ModelVoice};
pub use script::{Glyph, Script, Sentence, Spot};
pub use sentences::{LONGEST, split};
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
    /// Sentences likely said next, in order, to get ready ahead.
    fn prepare(&mut self, _texts: &[&str]) {}
    /// What happened since the last call, oldest first.
    fn events(&mut self) -> Vec<Event>;
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
const SAMPLES: [(&str, &str); 12] = [
    ("en", SAMPLE),
    ("de", "So klingen Ihre Bücher, wenn sie vorgelesen werden."),
    ("es", "Así sonarán tus libros cuando se lean en voz alta."),
    (
        "fr",
        "Voici comment vos livres sonneront, lus à voix haute.",
    ),
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

/// The engines to pick from, by id and name: the system's, then models ready.
pub fn engines(shelf: &Shelf) -> Vec<(&'static str, &'static str)> {
    // A browser build runs no model yet.
    let runs = |m: &&Model| cfg!(not(target_arch = "wasm32")) && m.id == KOKORO.id;
    let models = shelf
        .ready()
        .into_iter()
        .filter(runs)
        .map(|m| (m.id, m.name));
    std::iter::once((SYSTEM, "System voices"))
        .chain(models)
        .collect()
}

/// Starts engine `id`; `wake` is called whenever it has events.
pub fn open(id: &str, shelf: &Shelf, wake: Waker) -> Result<Box<dyn Voicer>, Fault> {
    match id {
        #[cfg(not(target_arch = "wasm32"))]
        SYSTEM => Ok(Box::new(system::System::new(wake)?)),
        SILENT => Ok(Box::new(silent::Silent::default())),
        #[cfg(not(target_arch = "wasm32"))]
        id if id == KOKORO.id => {
            let ready = shelf.ready().into_iter().find(|m| m.id == id);
            let folder = ready.and_then(|m| shelf.folder(m));
            let (Some(model), Some(folder)) = (ready, folder) else {
                return Err(Fault::Unavailable(format!(
                    "{} isn't downloaded",
                    KOKORO.name
                )));
            };
            let voices = (model.voices.iter())
                .filter(|v| shelf.has(model, v.id))
                .map(|v| Voice {
                    id: v.id.to_owned(),
                    name: v.name.to_owned(),
                    language: v.language.to_owned(),
                });
            // The voice that came with the model leads, as the default.
            let (mut voices, first): (Vec<Voice>, _) = (voices.collect(), model.first_voice);
            voices.sort_by_key(|v| v.id != first);
            let engine = kokoro::Engine::open(folder, voices, shelf.studio().clone(), wake)?;
            Ok(Box::new(engine))
        }
        _ => {
            let _ = (wake, shelf);
            Err(Fault::Unavailable(format!("No speech engine named {id}")))
        }
    }
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
        let Ok(mut v) = open(SILENT, &Shelf::default(), wake) else {
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
            open("nope", &shelf, wake.clone()),
            Err(Fault::Unavailable(_))
        ));
        assert!(matches!(
            open(KOKORO.id, &shelf, wake),
            Err(Fault::Unavailable(_))
        ));
        assert_eq!(engines(&shelf), [(SYSTEM, "System voices")]);
    }
}
