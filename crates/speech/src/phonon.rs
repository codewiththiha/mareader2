//! Phonon as a local model: one build's weights, and a file per voice.

use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;

use mareader_phonon::{Noise, Pace, Phonon as Model, SAMPLE_RATE, TEMPERATURE, Voice};

use crate::local::{Line, Out, Synth};
use crate::studio::Studio;

/// The speeds offered: the pace condition keeps speech steady only near 1.
pub(crate) const SPEEDS: RangeInclusive<f32> = 0.9..=1.3;

/// Speed against its pace: the reference's medians over 12 lines.
const PACES: [(f32, f32); 9] = [
    (0.89, 0.3),
    (0.91, 0.2),
    (0.946, 0.1),
    (1.0, 0.0),
    (1.066, -0.1),
    (1.126, -0.2),
    (1.183, -0.3),
    (1.244, -0.4),
    (1.297, -0.5),
];

/// Where Phonon's build is kept, and its voices.
#[derive(Clone)]
pub(crate) struct Source {
    pub config: PathBuf,
    pub weights: PathBuf,
    pub tokenizer: PathBuf,
    /// Each downloaded voice's file, by id.
    pub voices: HashMap<String, PathBuf>,
    /// The model's name with its build's, as `Phonon (8-bit)`.
    pub name: Arc<str>,
}

/// What a voice needs once loaded.
struct Loaded {
    model: Arc<Model>,
    id: String,
    voice: Voice,
}

/// Phonon from `source`, speaking a voice at a time.
pub(crate) struct Phonon {
    source: Source,
    studio: Studio,
    loaded: Option<Loaded>,
}

impl Phonon {
    pub(crate) fn new(source: Source, studio: Studio) -> Self {
        Self {
            source,
            studio,
            loaded: None,
        }
    }

    fn load_fresh(&self, id: &str) -> Result<Loaded, String> {
        let source = &self.source;
        let path =
            (source.voices.get(id)).ok_or_else(|| format!("{} has no voice {id}", source.name))?;
        let model = (self.studio).model(&source.name, || {
            Model::load(&source.config, &source.weights, &source.tokenizer)
                .map_err(|e| e.to_string())
        })?;
        let voice = model.voice(path).map_err(|e| e.to_string())?;
        Ok(Loaded {
            model,
            id: id.to_owned(),
            voice,
        })
    }
}

impl Synth for Phonon {
    fn cold(&self, _voice: &str) -> bool {
        !self.studio.has_model(&self.source.name)
    }

    fn load(&mut self, voice: &str) -> Result<(), String> {
        if !self.loaded.as_ref().is_some_and(|l| l.id == voice) {
            // The old voice goes first, so two are never held.
            self.loaded = None;
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
        let loaded = self.loaded.as_ref().ok_or("Phonon was let go")?;
        let pace = pace(line.speed);
        let mut going = true;
        for chunk in loaded.model.chunks(line.text) {
            // Seeded, so the same line always sounds the same.
            let mut noise = Noise::new(TEMPERATURE, 0);
            let said = loaded
                .model
                .say(&loaded.voice, &chunk, pace, &mut noise, |block| {
                    going = out(&block, SAMPLE_RATE, false);
                    going
                });
            said.map_err(|e| e.to_string())?;
            if !going {
                break;
            }
        }
        Ok(())
    }
}

/// The pace for `speed`, read off [`PACES`] between its points.
fn pace(speed: f32) -> Pace {
    let speed = match speed.is_finite() {
        true => speed.clamp(*SPEEDS.start(), *SPEEDS.end()),
        false => 1.0,
    };
    let above = PACES.iter().position(|&(s, _)| s >= speed);
    let delta = match above.map(|i| (i.checked_sub(1).and_then(|j| PACES.get(j)), PACES.get(i))) {
        Some((Some(&(s0, d0)), Some(&(s1, d1)))) => d0 + (d1 - d0) * (speed - s0) / (s1 - s0),
        Some((None, Some(&(_, d)))) => d,
        _ => PACES.last().map_or(0.0, |&(_, d)| d),
    };
    Pace(delta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Voice as Listed;
    use crate::local::{Engine, Setup};

    #[test]
    fn phonon_reads_each_speed_as_a_pace_between_the_measured_ones() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(near(pace(1.0).0, 0.0));
        assert!(near(pace(1.126).0, -0.2));
        assert!(near(pace(1.096).0, -0.15));
        assert!(near(pace(0.9).0, 0.25));
        // Held to the speeds offered, and 1 when no number.
        assert_eq!(pace(3.0), pace(1.3));
        assert_eq!(pace(0.5), pace(0.9));
        assert_eq!(pace(f32::NAN), Pace(0.0));
        assert!(
            PACES
                .windows(2)
                .all(|w| matches!(w, [a, b] if a.0 < b.0 && a.1 > b.1))
        );
    }

    #[test]
    fn a_downloaded_phonon_makes_the_sentence_said_then_the_next() {
        // Only where a model folder is given, as the remote check does.
        let Some(dir) = std::env::var_os("MAREADER_PHONON") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let name: Arc<str> = "Phonon (8-bit)".into();
        let voices = [("default".to_owned(), dir.join("default-voice.safetensors"))];
        let source = Source {
            config: dir.join("config.json"),
            weights: dir.join("model.q8.gguf"),
            tokenizer: dir.join("tokenizer.json"),
            voices: voices.into_iter().collect(),
            name: Arc::clone(&name),
        };
        let setup = Setup {
            name,
            voices: vec![Listed {
                id: "default".into(),
                name: "Default".into(),
                language: "en-US".into(),
            }],
            speeds: SPEEDS,
            knobs: &[],
        };
        let alone = Phonon::new(source.clone(), Studio::default());
        crate::local::tests::time_load(alone, "default", "phonon engine");
        let studio = Studio::default();
        let synth = Phonon::new(source, studio.clone());
        match Engine::open(synth, setup, studio, Arc::new(|| {})) {
            Ok(e) => crate::local::tests::live(e, "phonon engine"),
            Err(e) => panic!("{e}"),
        }
    }
}
