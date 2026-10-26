//! A voice's `.onnx.json`: its sample rate, phoneme table, speakers and scales.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

use crate::phonemes::Style;
use crate::{Error, Result, Scales};

const PAD: &str = "_";
const BOS: &str = "^";
const EOS: &str = "$";

#[derive(Debug, Clone, Deserialize)]
pub struct Audio {
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Espeak {
    pub voice: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub audio: Audio,
    pub espeak: Espeak,
    pub inference: Scales,
    pub phoneme_id_map: HashMap<String, Vec<u32>>,
    #[serde(default)]
    pub num_speakers: u32,
    #[serde(default)]
    pub speaker_id_map: BTreeMap<String, u32>,
}

impl Config {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(|e| Error::Load(format!("the voice's settings: {e}")))
    }

    /// How its phonemes are spelled, if this crate can write them.
    pub fn style(&self) -> Option<Style> {
        Style::from_espeak_voice(&self.espeak.voice)
    }

    fn id(&self, symbol: &str) -> Result<&[u32]> {
        self.phoneme_id_map
            .get(symbol)
            .map(Vec::as_slice)
            .ok_or_else(|| Error::Load(format!("the phoneme table has no {symbol:?}")))
    }

    /// Ids for one sentence: `^`, then each known phoneme and a pad, then `$`.
    pub fn ids(&self, phonemes: &str) -> Result<Vec<u32>> {
        Ok(self.ids_at(phonemes)?.0)
    }

    /// As [`Self::ids`], with the index each character's ids start at.
    pub fn ids_at(&self, phonemes: &str) -> Result<(Vec<u32>, Vec<usize>)> {
        let pad = self.id(PAD)?;
        let mut ids = self.id(BOS)?.to_vec();
        ids.extend_from_slice(pad);
        let mut at = Vec::with_capacity(phonemes.len());
        let mut buf = [0u8; 4];
        for c in phonemes.chars() {
            at.push(ids.len());
            if let Some(found) = self.phoneme_id_map.get(&*c.encode_utf8(&mut buf)) {
                ids.extend_from_slice(found);
                ids.extend_from_slice(pad);
            }
        }
        ids.extend_from_slice(self.id(EOS)?);
        Ok((ids, at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
        "audio": {"sample_rate": 22050, "quality": "medium"},
        "espeak": {"voice": "en-us"},
        "inference": {"noise_scale": 0.667, "length_scale": 1, "noise_w": 0.8},
        "phoneme_id_map": {"_": [0], "^": [1], "$": [2], " ": [3], "h": [20], "ə": [59]},
        "num_speakers": 1,
        "speaker_id_map": {}
    }"#;

    #[test]
    fn reads_a_voice_config_and_spells_ids_with_pads() {
        let c = Config::parse(JSON.as_bytes()).unwrap();
        assert_eq!(c.audio.sample_rate, 22050);
        assert_eq!(c.style(), Some(Style::American));
        assert_eq!(c.inference.noise_w, 0.8);
        assert_eq!(
            c.ids("hə ❓h").unwrap(),
            [1, 0, 20, 0, 59, 0, 3, 0, 20, 0, 2]
        );
        // Each character's ids start here; the unknown at the next.
        let (_, at) = c.ids_at("hə ❓h").unwrap();
        assert_eq!(at, [2, 4, 6, 8, 8]);
    }
}
