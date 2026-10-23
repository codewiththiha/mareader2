//! Kitten as a local model: a build's weights, its voices, and the G2P.

use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;

use mareader_g2p::{Dialect, G2p};
use mareader_kokoro::{Kokoro as Model, SAMPLE_RATE, Voice, kitten};
use mareader_vits::{Style, espeak_like};

use crate::local::{Line, Out, Synth, trim_join};
use crate::piper::pieces;
use crate::studio::Studio;

/// The speeds offered, as shares of each voice's own pace.
pub(crate) const SPEEDS: RangeInclusive<f32> = 0.5..=3.0;
/// The pause owed at a clause split, in seconds at normal speed.
const CLAUSE: f64 = 0.12;

/// Where a Kitten build is kept, and how it speaks.
pub(crate) struct Source {
    /// Where the G2P's files are.
    pub folder: PathBuf,
    pub weights: PathBuf,
    pub voices: PathBuf,
    /// The model's name with its build's, as `Kitten (Nano)`.
    pub name: Arc<str>,
    /// Nano was tuned to be slowed a little, as its own settings say.
    pub nano: bool,
}

/// A build's weights and voices, held as one.
struct Build {
    model: Model,
    voices: HashMap<String, Voice>,
}

/// What a voice needs once loaded.
struct Loaded {
    build: Arc<Build>,
    g2p: Arc<G2p>,
    voice: String,
}

/// Kitten's build from `source`, speaking a voice at a time.
pub(crate) struct Kitten {
    source: Source,
    studio: Studio,
    loaded: Option<Loaded>,
}

impl Kitten {
    pub(crate) fn new(source: Source, studio: Studio) -> Self {
        Self {
            source,
            studio,
            loaded: None,
        }
    }

    fn load_fresh(&self, voice: &str) -> Result<Loaded, String> {
        let source = &self.source;
        let build = (self.studio).model(&source.name, || {
            let model = Model::from_onnx(&source.weights).map_err(|e| e.to_string())?;
            let voices = kitten::read_voices(&source.voices).map_err(|e| e.to_string())?;
            Ok(Build {
                model,
                voices: voices.into_iter().collect(),
            })
        })?;
        if !build.voices.contains_key(voice) {
            return Err(format!("{} has no voice {voice}", source.name));
        }
        let g2p = (self.studio).g2p(Dialect::American, || {
            G2p::load(&source.folder, Dialect::American).map_err(|e| e.to_string())
        })?;
        Ok(Loaded {
            build,
            g2p,
            voice: voice.to_owned(),
        })
    }
}

impl Synth for Kitten {
    fn cold(&self, _voice: &str) -> bool {
        !self.studio.has_model(&self.source.name)
    }

    fn load(&mut self, voice: &str) -> Result<(), String> {
        if !self.loaded.as_ref().is_some_and(|l| l.voice == voice) {
            self.loaded = Some(self.load_fresh(voice)?);
        }
        Ok(())
    }

    fn unload(&mut self) {
        self.loaded = None;
    }

    fn loaded(&self) -> bool {
        self.loaded.is_some()
    }

    fn render(&mut self, line: &Line<'_>, out: &mut Out<'_>) -> Result<(), String> {
        let loaded = self.loaded.as_ref().ok_or("Kitten was let go")?;
        let voice = (loaded.build.voices.get(&loaded.voice)).ok_or("Kitten lost its voice")?;
        let speed =
            line.speed.clamp(*SPEEDS.start(), *SPEEDS.end()) * prior(&self.source, &loaded.voice);
        let phonemes = espeak_like(&loaded.g2p.phonemize(line.text), Style::American);
        let pieces = pieces(&phonemes, line.lead);
        for (i, piece) in pieces.iter().enumerate() {
            let piece = closed(piece);
            let style = kitten::style(voice, &piece).map_err(|e| e.to_string())?;
            let ids = kitten::ids(&piece);
            let trace =
                (loaded.build.model.trace(&ids, &style, speed)).map_err(|e| e.to_string())?;
            let mut audio = trace.audio;
            trim(&mut audio);
            let last = i + 1 == pieces.len();
            let owed = CLAUSE / f64::from(line.speed);
            trim_join(&mut audio, SAMPLE_RATE, i > 0, (!last).then_some(owed));
            if !out(&audio, SAMPLE_RATE, last) {
                break;
            }
        }
        Ok(())
    }
}

/// The pace a voice was tuned for in this build, as its settings give it.
fn prior(source: &Source, voice: &str) -> f32 {
    match (source.nano, voice) {
        (true, "expr-voice-4-m") => 0.9,
        (true, _) => 0.8,
        (false, _) => 1.0,
    }
}

/// A piece ending in a mark, as Kitten closes every run it is given.
fn closed(piece: &str) -> String {
    let marked = piece.ends_with(['.', ',', '!', '?', ';', ':', '…']);
    match marked {
        true => piece.to_owned(),
        false => format!("{piece},"),
    }
}

/// Cuts the tail Kitten leaves after each run; a short run keeps half.
fn trim(audio: &mut Vec<f32>) {
    let keep = audio
        .len()
        .saturating_sub(kitten::TRIM)
        .max(audio.len() / 2);
    audio.truncate(keep);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Voice as Listed;
    use crate::local::{Engine, Setup};

    fn source(nano: bool) -> Source {
        Source {
            folder: PathBuf::new(),
            weights: PathBuf::new(),
            voices: PathBuf::new(),
            name: "Kitten (Nano)".into(),
            nano,
        }
    }

    #[test]
    fn kitten_slows_nano_as_tuned_and_closes_each_piece() {
        assert_eq!(prior(&source(true), "expr-voice-2-f"), 0.8);
        assert_eq!(prior(&source(true), "expr-voice-4-m"), 0.9);
        assert_eq!(prior(&source(false), "expr-voice-4-m"), 1.0);
        assert_eq!(closed("həlˈoʊ"), "həlˈoʊ,");
        assert_eq!(closed("həlˈoʊ."), "həlˈoʊ.");
        let mut long = vec![0.5; kitten::TRIM + 100];
        trim(&mut long);
        assert_eq!(long.len(), 100.max((kitten::TRIM + 100) / 2));
        let mut short = vec![0.5; 10];
        trim(&mut short);
        assert_eq!(short.len(), 5);
    }

    #[test]
    fn a_downloaded_kitten_makes_the_sentence_said_then_the_next() {
        // Only where a model folder is given, as the remote check does.
        let Some(dir) = std::env::var_os("MAREADER_KITTEN") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let name: Arc<str> = "Kitten (Nano)".into();
        let source = Source {
            weights: dir.join("nano.onnx"),
            voices: dir.join("nano-voices.npz"),
            folder: dir,
            name: Arc::clone(&name),
            nano: true,
        };
        let setup = Setup {
            name,
            voices: vec![Listed {
                id: "expr-voice-2-f".into(),
                name: "Bella".into(),
                language: "en-US".into(),
            }],
            speeds: SPEEDS,
            knobs: &[],
        };
        let studio = Studio::default();
        let synth = Kitten::new(source, studio.clone());
        match Engine::open(synth, setup, studio, Arc::new(|| {})) {
            Ok(e) => crate::local::tests::live(e, "kitten engine"),
            Err(e) => panic!("{e}"),
        }
    }
}
