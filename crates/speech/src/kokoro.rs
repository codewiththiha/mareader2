//! Kokoro as a local model: its weights, a voice pack, and the G2P.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mareader_g2p::{Dialect, G2p};
use mareader_hanzi::Hanzi;
use mareader_ipa::Language;
use mareader_kokoro::{Kokoro as Model, SAMPLE_RATE, Voice as Pack};

use crate::local::{Line, Out, Synth, trim_join};
use crate::studio::Studio;

pub(crate) use mareader_kokoro::SPEEDS;

/// Tokens in the first piece of a sentence wanted now, about two seconds.
const LEAD: usize = 32;

/// Where a Kokoro build is kept, and its name.
pub(crate) struct Source {
    pub folder: PathBuf,
    pub weights: PathBuf,
    /// The model's name with its build's, as `Kokoro (Small)`.
    pub name: Arc<str>,
}

/// What a voice needs once loaded.
struct Loaded {
    model: Arc<Model>,
    reader: Reader,
    voice: (String, Pack),
}

/// What turns a voice's text into phonemes.
enum Reader {
    /// English, from the downloaded G2P data.
    English(Arc<G2p>),
    /// Another language, by spelling rules alone.
    Rules(Language),
    /// Chinese, from the dictionaries Chinese voices download.
    Chinese(Arc<Hanzi>),
}

/// Kokoro's build from `source`, speaking a voice at a time.
pub(crate) struct Kokoro {
    source: Source,
    studio: Studio,
    loaded: Option<Loaded>,
}

impl Kokoro {
    pub(crate) fn new(source: Source, studio: Studio) -> Self {
        Self {
            source,
            studio,
            loaded: None,
        }
    }

    fn load_fresh(&self, voice: &str) -> Result<Loaded, String> {
        let Source {
            folder,
            weights,
            name,
        } = &self.source;
        let model = (self.studio).model(name, || {
            let config = folder.join("config.json");
            Model::from_files(&config, weights).map_err(|e| e.to_string())
        })?;
        let chinese = voice.starts_with('z');
        let reader = match Language::for_kokoro_voice(voice) {
            Some(language) => Reader::Rules(language),
            None if chinese => Reader::Chinese(
                (self.studio)
                    .hanzi(|| Hanzi::load(&folder.join("hanzi")).map_err(|e| e.to_string()))?,
            ),
            None => {
                let dialect = Dialect::for_kokoro_voice(voice).unwrap_or(Dialect::American);
                Reader::English((self.studio).g2p(dialect, || {
                    G2p::load(folder, dialect).map_err(|e| e.to_string())
                })?)
            }
        };
        let pack = Pack::load(&pack_path(folder, voice)).map_err(|e| e.to_string())?;
        Ok(Loaded {
            model,
            reader,
            voice: (voice.to_owned(), pack),
        })
    }
}

impl Synth for Kokoro {
    fn cold(&self, _voice: &str) -> bool {
        !self.studio.has_model(&self.source.name)
    }

    fn load(&mut self, voice: &str) -> Result<(), String> {
        let fits = |l: &Loaded| l.voice.0 == voice;
        if !self.loaded.as_ref().is_some_and(fits) {
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
        let loaded = self.loaded.as_ref();
        let Loaded {
            model,
            reader,
            voice,
        } = loaded.ok_or("Kokoro was let go")?;
        let phonemes = match reader {
            Reader::English(g2p) => {
                let mut phonemes = String::new();
                for token in g2p.tokens(line.text) {
                    phonemes.push_str(&token.phonemes);
                    if token.whitespace {
                        phonemes.push(' ');
                    }
                }
                phonemes
            }
            Reader::Rules(language) => mareader_ipa::kokoro(*language, line.text),
            Reader::Chinese(hanzi) => hanzi.phonemize(line.text),
        };
        let phonemes = model.normalise(&phonemes);
        let chunks = match line.lead {
            true => model.chunks_led(&phonemes, LEAD),
            false => model.chunks(&phonemes),
        };
        for (i, chunk) in chunks.iter().enumerate() {
            let mut audio =
                (model.render(chunk, &voice.1, line.speed)).map_err(|e| e.to_string())?;
            let owed = f64::from(chunk.pause_ms) / 1000.0 / f64::from(line.speed);
            let last = i + 1 == chunks.len();
            trim_join(&mut audio, SAMPLE_RATE, i > 0, (!last).then_some(owed));
            if !out(&audio, SAMPLE_RATE, last) {
                break;
            }
        }
        Ok(())
    }
}

fn pack_path(folder: &Path, voice: &str) -> PathBuf {
    folder.join("voices").join(format!("{voice}.pt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Voice;
    use crate::local::{Engine, Setup};

    #[test]
    fn chinese_voices_bring_what_the_reader_loads() {
        let paths: Vec<String> = mareader_hanzi::FILES
            .iter()
            .map(|f| format!("hanzi/{f}"))
            .collect();
        let listed: Vec<&str> = crate::models::HANZI.iter().map(|f| f.path).collect();
        assert_eq!(listed, paths);
        let voices = crate::models::KOKORO.voices.iter();
        let chinese: Vec<&str> = voices
            .filter(|v| v.language == "zh-CN")
            .map(|v| v.id)
            .collect();
        assert_eq!(chinese.len(), 8);
        assert!(chinese.iter().all(|id| id.starts_with('z')));
    }

    #[test]
    fn a_downloaded_model_makes_the_sentence_said_then_the_next() {
        // Only where a model folder is given, as the remote check does.
        let Some(dir) = std::env::var_os("MAREADER_KOKORO") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let name: Arc<str> = "Kokoro (Full)".into();
        let source = Source {
            weights: dir.join(mareader_kokoro::WEIGHTS),
            folder: dir,
            name: Arc::clone(&name),
        };
        let setup = Setup {
            name,
            voices: vec![Voice {
                id: "af_heart".into(),
                name: "Heart".into(),
                language: "en-US".into(),
            }],
            speeds: SPEEDS,
            knobs: &[],
        };
        let studio = Studio::default();
        let synth = Kokoro::new(source, studio.clone());
        match Engine::open(synth, setup, studio, std::sync::Arc::new(|| {})) {
            Ok(e) => crate::local::tests::live(e, "kokoro engine"),
            Err(e) => panic!("{e}"),
        }
    }
}
