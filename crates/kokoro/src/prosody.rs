//! How long each phoneme lasts, the pitch and energy curves, and text features.

use candle_core::{Module, Result, Tensor};
use candle_nn::{Embedding, VarBuilder};

use crate::fast::Then;
use crate::layers::{
    AdaIn1d, AdaLayerNorm, BiLstm, ChannelNorm, Conv, Conv1d, ConvTranspose1d, LinearNorm,
    leaky_relu, scale, upsample,
};

/// A residual block whose norms follow the style; may double the length.
pub(crate) struct AdainResBlk1d {
    conv1: Conv1d,
    conv2: Conv1d,
    norm1: AdaIn1d,
    norm2: AdaIn1d,
    shortcut: Option<Conv1d>,
    /// Upsampling only: a learned stride-2 spread of each channel.
    pool: Option<ConvTranspose1d>,
}

impl AdainResBlk1d {
    pub(crate) fn load(
        dims: (usize, usize),
        style: usize,
        upsampling: bool,
        vb: &VarBuilder,
    ) -> Result<Self> {
        let (dim_in, dim_out) = dims;
        let shortcut = (dim_in != dim_out)
            .then(|| {
                Conv::new(dim_in, dim_out, 1)
                    .no_bias()
                    .load(&vb.pp("conv1x1"))
            })
            .transpose()?;
        let pool = upsampling
            .then(|| {
                Conv::new(dim_in, dim_in, 3)
                    .stride(2)
                    .padding(1)
                    .output_padding(1)
                    .groups(dim_in)
                    .load_transposed(&vb.pp("pool"))
            })
            .transpose()?;
        Ok(Self {
            conv1: Conv::new(dim_in, dim_out, 3)
                .padding(1)
                .load(&vb.pp("conv1"))?,
            conv2: Conv::new(dim_out, dim_out, 3)
                .padding(1)
                .load(&vb.pp("conv2"))?,
            norm1: AdaIn1d::load(style, dim_in, &vb.pp("norm1"))?,
            norm2: AdaIn1d::load(style, dim_out, &vb.pp("norm2"))?,
            shortcut,
            pool,
        })
    }

    pub(crate) fn upsamples(&self) -> bool {
        self.pool.is_some()
    }

    pub(crate) fn forward(&self, x: &Tensor, s: &Tensor) -> Result<Tensor> {
        let mut short = x.clone();
        if self.upsamples() {
            short = upsample(&short, short.dim(2)? * 2)?;
        }
        if let Some(conv) = &self.shortcut {
            short = conv.forward(&short)?;
        }
        let mut out = self.norm1.forward_then(x, s, Then::Leaky(0.2))?;
        if let Some(pool) = &self.pool {
            out = pool.forward(&out)?;
        }
        let out = self.conv1.forward(&out)?;
        let out = self.norm2.forward_then(&out, s, Then::Leaky(0.2))?;
        let out = self.conv2.forward(&out)?;
        scale(&(out + short)?, std::f32::consts::FRAC_1_SQRT_2)
    }
}

/// The style appended to every step: `[1, n, c]` to `[1, n, c + style]`.
fn with_style(x: &Tensor, s: &Tensor) -> Result<Tensor> {
    let (batch, n, _) = x.dims3()?;
    let style = s.unsqueeze(1)?.broadcast_as((batch, n, s.dim(1)?))?;
    Tensor::cat(&[x, &style.contiguous()?], 2)
}

/// Phoneme features read with the style, ready for timing.
struct DurationEncoder {
    layers: Vec<(BiLstm, AdaLayerNorm)>,
}

impl DurationEncoder {
    fn load(style: usize, hidden: usize, depth: usize, vb: &VarBuilder) -> Result<Self> {
        let lstms = vb.pp("lstms");
        let layers = (0..depth)
            .map(|i| {
                let lstm = BiLstm::load(hidden + style, hidden / 2, &lstms.pp(i * 2))?;
                let norm = AdaLayerNorm::load(style, hidden, &lstms.pp(i * 2 + 1))?;
                Ok((lstm, norm))
            })
            .collect::<Result<_>>()?;
        Ok(Self { layers })
    }

    /// `[1, hidden, n]` to `[1, n, hidden + style]`.
    fn forward(&self, x: &Tensor, s: &Tensor) -> Result<Tensor> {
        let mut x = with_style(&x.transpose(1, 2)?, s)?;
        for (lstm, norm) in &self.layers {
            x = norm.forward(&lstm.forward(&x)?, s)?;
            x = with_style(&x, s)?;
        }
        Ok(x)
    }
}

pub(crate) struct ProsodyPredictor {
    encoder: DurationEncoder,
    lstm: BiLstm,
    duration: LinearNorm,
    shared: BiLstm,
    f0: Vec<AdainResBlk1d>,
    f0_proj: Conv1d,
    noise: Vec<AdainResBlk1d>,
    noise_proj: Conv1d,
}

impl ProsodyPredictor {
    pub(crate) fn load(
        style: usize,
        hidden: usize,
        depth: usize,
        max_dur: usize,
        vb: &VarBuilder,
    ) -> Result<Self> {
        let half = hidden / 2;
        let blocks = |name: &str| -> Result<Vec<AdainResBlk1d>> {
            let vb = vb.pp(name);
            Ok(vec![
                AdainResBlk1d::load((hidden, hidden), style, false, &vb.pp(0))?,
                AdainResBlk1d::load((hidden, half), style, true, &vb.pp(1))?,
                AdainResBlk1d::load((half, half), style, false, &vb.pp(2))?,
            ])
        };
        Ok(Self {
            encoder: DurationEncoder::load(style, hidden, depth, &vb.pp("text_encoder"))?,
            lstm: BiLstm::load(hidden + style, half, &vb.pp("lstm"))?,
            duration: LinearNorm::load(hidden, max_dur, &vb.pp("duration_proj.linear_layer"))?,
            shared: BiLstm::load(hidden + style, half, &vb.pp("shared"))?,
            f0: blocks("F0")?,
            f0_proj: Conv::new(half, 1, 1).load(&vb.pp("F0_proj"))?,
            noise: blocks("N")?,
            noise_proj: Conv::new(half, 1, 1).load(&vb.pp("N_proj"))?,
        })
    }

    /// Phoneme features `[1, hidden, n]` with the style, for timing.
    pub(crate) fn encode(&self, x: &Tensor, s: &Tensor) -> Result<Tensor> {
        self.encoder.forward(x, s)
    }

    /// Frames each phoneme lasts, unrounded: `[1, n]`.
    pub(crate) fn durations(&self, encoded: &Tensor) -> Result<Tensor> {
        let x = self.duration.forward(&self.lstm.forward(encoded)?)?;
        candle_nn::ops::sigmoid(&x)?.sum(2)
    }

    /// Pitch and energy, two per frame, from frame features `[1, c, frames]`.
    pub(crate) fn curves(&self, x: &Tensor, s: &Tensor) -> Result<(Tensor, Tensor)> {
        let shared = self.shared.forward(&x.transpose(1, 2)?)?.transpose(1, 2)?;
        let run = |blocks: &[AdainResBlk1d], proj: &Conv1d| -> Result<Tensor> {
            let mut x = shared.clone();
            for block in blocks {
                x = block.forward(&x, s)?;
            }
            proj.forward(&x)?.squeeze(1)
        };
        Ok((
            run(&self.f0, &self.f0_proj)?,
            run(&self.noise, &self.noise_proj)?,
        ))
    }
}

/// The phonemes' own features, aligned later for the decoder.
pub(crate) struct TextEncoder {
    embedding: Embedding,
    cnn: Vec<(Conv1d, ChannelNorm)>,
    lstm: BiLstm,
}

impl TextEncoder {
    pub(crate) fn load(
        channels: usize,
        kernel: usize,
        depth: usize,
        symbols: usize,
        vb: &VarBuilder,
    ) -> Result<Self> {
        let cnn = (0..depth)
            .map(|i| {
                let vb = vb.pp("cnn").pp(i);
                let conv = Conv::new(channels, channels, kernel)
                    .padding(kernel / 2)
                    .load(&vb.pp(0))?;
                Ok((conv, ChannelNorm::load(channels, &vb.pp(1))?))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            embedding: candle_nn::embedding(symbols, channels, vb.pp("embedding"))?,
            cnn,
            lstm: BiLstm::load(channels, channels / 2, &vb.pp("lstm"))?,
        })
    }

    /// Token ids `[1, n]` to `[1, channels, n]`.
    pub(crate) fn forward(&self, ids: &Tensor) -> Result<Tensor> {
        let mut x = self.embedding.forward(ids)?.transpose(1, 2)?;
        for (conv, norm) in &self.cnn {
            let y = conv.forward(&x)?;
            let y = norm.forward(&y.transpose(1, 2)?)?.transpose(1, 2)?;
            x = leaky_relu(&y, 0.2)?;
        }
        self.lstm.forward(&x.transpose(1, 2)?)?.transpose(1, 2)
    }
}
