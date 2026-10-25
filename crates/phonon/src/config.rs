//! The parts of a checkpoint's `config.json` the port reads.

use serde::Deserialize;

use crate::{Error, Result};

/// A checkpoint's shape and its sampling settings.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub flow_lm: FlowLm,
    pub mimi: Mimi,
    pub lsd_decode_steps: usize,
    pub eos_threshold: f32,
    /// Voice formats the checkpoint takes, as `sig@epoch`.
    #[serde(default)]
    pub embeddings_ext: Vec<String>,
    #[serde(default)]
    pub conditioners: Vec<Conditioner>,
    /// Present on checkpoints whose layers use the exact GELU by default.
    #[serde(default)]
    model_id: Option<serde_json::Value>,
}

/// Which GELU the feed-forward layers use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gelu {
    Erf,
    Tanh,
}

/// The flow language model.
#[derive(Clone, Debug, Deserialize)]
pub struct FlowLm {
    pub d_model: usize,
    pub num_heads: usize,
    pub num_layers: usize,
    pub dim_feedforward: usize,
    pub max_period: f32,
    pub n_bins: usize,
    pub flow_dim: usize,
    pub flow_depth: usize,
    /// Width of one audio latent.
    pub ldim: usize,
    #[serde(default)]
    pub time_rms_norm: TimeNorm,
    pub gelu: Option<Gelu>,
}

/// How the flow head's time embeddings are scaled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeNorm {
    /// By their root mean square.
    Rms,
    /// By their unbiased standard deviation, uncentred.
    #[default]
    RmsVar,
}

/// The Mimi codec's decoder.
#[derive(Clone, Debug, Deserialize)]
pub struct Mimi {
    pub sample_rate: usize,
    pub frame_rate: f64,
    pub dimension: usize,
    pub quantizer_dimension: usize,
    pub n_filters: usize,
    pub n_residual_layers: usize,
    pub ratios: Vec<usize>,
    pub kernel_size: usize,
    pub last_kernel_size: usize,
    pub residual_kernel_size: usize,
    pub dilation_base: usize,
    pub compress: usize,
    pub transformer_num_heads: usize,
    pub transformer_num_layers: usize,
    pub transformer_context: usize,
    pub transformer_max_period: f32,
    pub transformer_dim_feedforward: usize,
    pub gelu: Option<Gelu>,
}

/// A condition summed into every audio frame.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Conditioner {
    Lut {
        name: String,
        lut: Lut,
    },
    Continuous {
        name: String,
        continuous: Continuous,
    },
}

/// A condition picked from a short list.
#[derive(Clone, Debug, Deserialize)]
pub struct Lut {
    #[serde(default)]
    pub possible_values: Vec<String>,
}

/// A condition given as a number.
#[derive(Clone, Debug, Deserialize)]
pub struct Continuous {
    pub scale_factor: f32,
    pub dim: usize,
    #[serde(default = "max_period")]
    pub max_period: f32,
}

fn max_period() -> f32 {
    10_000.0
}

impl Config {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let config: Self =
            serde_json::from_slice(bytes).map_err(|e| Error::Load(format!("config.json: {e}")))?;
        config.check()?;
        Ok(config)
    }

    /// The GELU for a stack configured with `given`.
    pub fn gelu(&self, given: Option<Gelu>) -> Gelu {
        given.unwrap_or(match self.model_id {
            Some(_) => Gelu::Erf,
            None => Gelu::Tanh,
        })
    }

    /// Frames a second of audio holds; 12.5 for every Phonon.
    pub fn frame_rate(&self) -> f64 {
        self.mimi.frame_rate
    }

    /// Samples one latent frame becomes.
    pub fn frame_samples(&self) -> usize {
        (self.mimi.sample_rate as f64 / self.mimi.frame_rate).round() as usize
    }

    fn check(&self) -> Result<()> {
        let lm = &self.flow_lm;
        let mimi = &self.mimi;
        let hop: usize = mimi.ratios.iter().product();
        let fits = lm.num_heads > 0
            && lm.d_model.is_multiple_of(lm.num_heads)
            && mimi.transformer_num_heads > 0
            && mimi.dimension.is_multiple_of(mimi.transformer_num_heads)
            && hop > 0
            && self.frame_samples().is_multiple_of(hop)
            && self.lsd_decode_steps > 0
            && mimi.compress > 0;
        match fits {
            true => Ok(()),
            false => Err(Error::Load("config.json describes no Phonon".into())),
        }
    }
}
