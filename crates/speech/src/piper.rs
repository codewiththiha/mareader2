//! Piper as a local model: each voice a small model, with the G2P.

use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;

use mareader_g2p::{Dialect, G2p};
use mareader_vits::{Config, Scales, Style, Vits, espeak_like};

use crate::local::{Line, Out, Synth, both, trim_join};
use crate::studio::Studio;

/// The speeds offered, as shares of each voice's own pace.
pub(crate) const SPEEDS: RangeInclusive<f32> = 0.5..=3.0;
/// Phonemes in the first piece of a sentence wanted now, about two seconds.
const LEAD: usize = 24;
/// Phonemes in a piece at most, well inside what the model takes.
const LONGEST: usize = 400;
/// The pause owed at a clause split, in seconds at normal speed.
const CLAUSE: f64 = 0.12;
/// Peaks below this are left alone rather than made loud.
const FLOOR: f32 = 0.01;

/// Where a voice's files are, and the name it shows as when loaded.
#[derive(Clone)]
pub(crate) struct Files {
    pub weights: PathBuf,
    pub settings: PathBuf,
    /// As `Piper (LJ)`.
    pub name: Arc<str>,
}

/// What a voice needs once loaded.
struct Loaded {
    voice: String,
    vits: Arc<Vits>,
    config: Config,
    style: Style,
    g2p: Arc<G2p>,
}

/// Piper's downloaded voices, one loaded at a time.
pub(crate) struct Piper {
    /// Where the G2P's files are.
    folder: PathBuf,
    voices: HashMap<String, Files>,
    studio: Studio,
    loaded: Option<Loaded>,
}

impl Piper {
    pub(crate) fn new(folder: PathBuf, voices: HashMap<String, Files>, studio: Studio) -> Self {
        Self {
            folder,
            voices,
            studio,
            loaded: None,
        }
    }

    fn load_fresh(&self, voice: &str) -> Result<Loaded, String> {
        let files = (self.voices.get(voice)).ok_or_else(|| format!("No Piper voice {voice}"))?;
        let bytes = std::fs::read(&files.settings).map_err(|e| e.to_string())?;
        let config = Config::parse(&bytes).map_err(|e| e.to_string())?;
        let style = (config.style()).ok_or_else(|| format!("{} speaks no English", files.name))?;
        let dialect = match style.british() {
            true => Dialect::British,
            false => Dialect::American,
        };
        let vits = || {
            (self.studio).model(&files.name, || {
                Vits::load(&files.weights).map_err(|e| e.to_string())
            })
        };
        let g2p = || {
            (self.studio).g2p(dialect, || {
                G2p::load(&self.folder, dialect).map_err(|e| e.to_string())
            })
        };
        let (vits, g2p) = both(vits, g2p)?;
        Ok(Loaded {
            voice: voice.to_owned(),
            vits,
            config,
            style,
            g2p,
        })
    }
}

impl Synth for Piper {
    fn cold(&self, voice: &str) -> bool {
        let files = self.voices.get(voice);
        !files.is_some_and(|f| self.studio.has_model(&f.name))
    }

    fn load(&mut self, voice: &str) -> Result<(), String> {
        if self.loaded.as_ref().is_some_and(|l| l.voice == voice) {
            return Ok(());
        }
        // Each voice is a model: the last goes before the next comes.
        self.loaded = None;
        self.loaded = Some(self.load_fresh(voice)?);
        Ok(())
    }

    fn unload(&mut self) {
        self.loaded = None;
    }

    fn loaded(&self) -> bool {
        self.loaded.is_some()
    }

    fn render(&mut self, line: &Line<'_>, out: &mut Out<'_>) -> Result<(), String> {
        let loaded = self.loaded.as_ref().ok_or("Piper was let go")?;
        let rate = loaded.config.audio.sample_rate;
        let scales = scales(loaded.config.inference, line);
        let phonemes = espeak_like(&loaded.g2p.phonemize(line.text), loaded.style);
        let pieces = pieces(&phonemes, line.lead);
        for (i, piece) in pieces.iter().enumerate() {
            let ids = loaded.config.ids(piece).map_err(|e| e.to_string())?;
            let mut audio = (loaded.vits.speak(&ids, 0, scales)).map_err(|e| e.to_string())?;
            normalise(&mut audio);
            let last = i + 1 == pieces.len();
            let owed = CLAUSE / f64::from(line.speed);
            trim_join(&mut audio, rate, i > 0, (!last).then_some(owed));
            if !out(&audio, rate, last) {
                break;
            }
        }
        Ok(())
    }
}

/// The voice's own scales, shared out by the line's speed and knobs:
/// expression, then rhythm.
fn scales(own: Scales, line: &Line<'_>) -> Scales {
    let share = |i: usize| line.knobs.get(i).copied().unwrap_or(1.0);
    Scales {
        noise: own.noise * share(0),
        noise_w: own.noise_w * share(1),
        length: own.length / line.speed.max(*SPEEDS.start()),
    }
}

/// Phonemes split at clauses: a short first if `lead`, none past [`LONGEST`].
pub(crate) fn pieces(phonemes: &str, lead: bool) -> Vec<String> {
    let clauses = phonemes.split_inclusive([',', ';', ':']);
    let parts = clauses.flat_map(|clause| {
        // A clause too long alone is split between words.
        let words = clause.split_inclusive(' ');
        let mut runs = vec![String::new()];
        for word in words {
            let full = runs
                .last()
                .is_some_and(|r| length(r) + length(word) > LONGEST);
            if full {
                runs.push(String::new());
            }
            if let Some(run) = runs.last_mut() {
                run.push_str(word);
            }
        }
        runs
    });
    let mut pieces: Vec<String> = Vec::new();
    let mut run = String::new();
    for part in parts {
        if !run.trim().is_empty() && length(&run) + length(&part) > LONGEST {
            pieces.push(std::mem::take(&mut run));
        }
        run.push_str(&part);
        if lead && pieces.is_empty() && length(run.trim()) >= LEAD {
            pieces.push(std::mem::take(&mut run));
        }
    }
    pieces.push(run);
    let pieces = pieces.into_iter().map(|p| p.trim().to_owned());
    pieces.filter(|p| !p.is_empty()).collect()
}

fn length(s: &str) -> usize {
    s.chars().count()
}

/// Scales a piece so its loudest sample is full scale, as Piper does.
fn normalise(audio: &mut [f32]) {
    let peak = audio.iter().fold(0f32, |m, s| m.max(s.abs()));
    let gain = 1.0 / peak.max(FLOOR);
    audio.iter_mut().for_each(|s| *s *= gain);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Voice;
    use crate::local::{Engine, Setup};

    fn line(lead: bool) -> Line<'static> {
        Line {
            text: "",
            speed: 2.0,
            knobs: vec![0.5],
            lead,
        }
    }

    #[test]
    fn a_sentence_said_now_leads_with_a_clause_and_the_rest_stays_whole() {
        let said = "hɛlˈoʊ ðɛɹ, ðɪs ɪz ðə fˈɜːst sˈɛntəns, ɹˈɛd ɐlˈaʊd.";
        let led = pieces(said, true);
        assert_eq!(
            led,
            ["hɛlˈoʊ ðɛɹ, ðɪs ɪz ðə fˈɜːst sˈɛntəns,", "ɹˈɛd ɐlˈaʊd."]
        );
        assert_eq!(
            pieces(said, false),
            [said],
            "ahead, a sentence is one piece"
        );
        assert_eq!(pieces("ɐ, b", true), ["ɐ, b"], "short enough to lead whole");
        assert!(pieces("  ", true).is_empty());
    }

    #[test]
    fn no_piece_is_longer_than_the_model_takes() {
        let word = "wˈɜːd ";
        let long = word.repeat(LONGEST);
        for lead in [false, true] {
            let all = pieces(&long, lead);
            assert!(all.len() > 1);
            assert!(all.iter().all(|p| length(p) <= LONGEST));
            assert_eq!(all.concat().replace(' ', ""), long.replace(' ', ""));
        }
    }

    #[test]
    fn speed_and_knobs_share_out_the_voice_own_scales() {
        let own = Scales {
            noise: 0.6,
            length: 0.8,
            noise_w: 0.4,
        };
        let s = scales(own, &line(true));
        assert_eq!((s.noise, s.length, s.noise_w), (0.3, 0.4, 0.4));
        let slow = Line {
            speed: 0.0,
            knobs: vec![],
            ..line(false)
        };
        assert_eq!(scales(own, &slow).length, 1.6, "speed held in range");
    }

    #[test]
    fn pieces_peak_at_full_scale_and_silence_stays_quiet() {
        let mut loud = vec![0.1, -0.5, 0.25];
        normalise(&mut loud);
        assert_eq!(loud, [0.2, -1.0, 0.5]);
        let mut hush = vec![0.001, -0.002];
        normalise(&mut hush);
        assert!(hush.iter().all(|s| s.abs() < 0.21));
        normalise(&mut []);
    }

    #[test]
    fn a_downloaded_voice_makes_the_sentence_said_then_the_next() {
        // Only where the model's folder is given, as the remote check does.
        let Some(dir) = std::env::var_os("MAREADER_PIPER") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let model = crate::models::PIPER;
        let mut files = HashMap::new();
        let mut voices = Vec::new();
        for v in model.voices.iter().filter(|v| v.note.is_empty()) {
            let (Some(weights), Some(settings)) = (v.file, v.extra) else {
                continue;
            };
            if !dir.join(weights.path).exists() {
                continue;
            }
            let name: Arc<str> = format!("Piper ({})", v.name).into();
            let (weights, settings) = (dir.join(weights.path), dir.join(settings.path));
            files.insert(
                v.id.to_owned(),
                Files {
                    weights,
                    settings,
                    name,
                },
            );
            voices.push(Voice {
                id: v.id.to_owned(),
                name: v.name.to_owned(),
                language: v.language.to_owned(),
            });
        }
        assert!(!voices.is_empty(), "no voice in {}", dir.display());
        let what = format!(
            "piper engine, {}",
            voices
                .iter()
                .map(|v| v.id.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
        let setup = Setup {
            name: model.name.into(),
            voices,
            speeds: SPEEDS,
            knobs: model.knobs,
        };
        let first = (setup.voices.first()).map_or("", |v| v.id.as_str());
        let alone = Piper::new(dir.clone(), files.clone(), Studio::default());
        crate::local::tests::time_load(alone, first, &what);
        let studio = Studio::default();
        let synth = Piper::new(dir, files, studio.clone());
        match Engine::open(synth, setup, studio, Arc::new(|| {})) {
            Ok(e) => crate::local::tests::live(e, &what),
            Err(e) => panic!("{e}"),
        }
    }
}
