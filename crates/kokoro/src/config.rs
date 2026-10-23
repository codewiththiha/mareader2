//! The model's `config.json`: sizes, and the phoneme alphabet.

use std::collections::HashMap;

use serde::Deserialize;

/// The parts of `config.json` the model is built from.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hidden_dim: usize,
    pub style_dim: usize,
    pub n_token: usize,
    pub n_layer: usize,
    pub max_dur: usize,
    pub text_encoder_kernel_size: usize,
    pub plbert: Plbert,
    pub istftnet: Istft,
    /// Each phoneme symbol and its token id.
    pub vocab: HashMap<String, u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hidden_dim: 512,
            style_dim: 128,
            n_token: 178,
            n_layer: 3,
            max_dur: 50,
            text_encoder_kernel_size: 5,
            plbert: Plbert::default(),
            istftnet: Istft::default(),
            vocab: HashMap::new(),
        }
    }
}

/// The phoneme encoder, an ALBERT.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Plbert {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub num_attention_heads: usize,
    pub num_hidden_layers: usize,
    pub intermediate_size: usize,
    pub max_position_embeddings: usize,
    pub embedding_size: usize,
    pub type_vocab_size: usize,
}

impl Default for Plbert {
    fn default() -> Self {
        Self {
            vocab_size: 178,
            hidden_size: 768,
            num_attention_heads: 12,
            num_hidden_layers: 12,
            intermediate_size: 2048,
            max_position_embeddings: 512,
            embedding_size: 128,
            type_vocab_size: 2,
        }
    }
}

/// The waveform generator.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Istft {
    pub upsample_rates: Vec<usize>,
    pub upsample_kernel_sizes: Vec<usize>,
    pub upsample_initial_channel: usize,
    pub resblock_kernel_sizes: Vec<usize>,
    pub resblock_dilation_sizes: Vec<Vec<usize>>,
    pub gen_istft_n_fft: usize,
    pub gen_istft_hop_size: usize,
}

impl Default for Istft {
    fn default() -> Self {
        Self {
            upsample_rates: vec![10, 6],
            upsample_kernel_sizes: vec![20, 12],
            upsample_initial_channel: 512,
            resblock_kernel_sizes: vec![3, 7, 11],
            resblock_dilation_sizes: vec![vec![1, 3, 5]; 3],
            gen_istft_n_fft: 20,
            gen_istft_hop_size: 5,
        }
    }
}

impl Config {
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_the_published_sizes() {
        let c = Config::parse(br#"{"vocab": {"a": 43}, "plbert": {"hidden_size": 8}}"#);
        let Ok(c) = c else {
            panic!("a partial config parses");
        };
        assert_eq!(c.hidden_dim, 512);
        assert_eq!(c.plbert.hidden_size, 8);
        assert_eq!(c.plbert.embedding_size, 128);
        assert_eq!(c.istftnet.upsample_rates, [10, 6]);
        assert_eq!(c.vocab.get("a"), Some(&43));
    }
}
