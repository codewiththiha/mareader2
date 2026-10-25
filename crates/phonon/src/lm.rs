//! The flow language model and the head that writes each latent.

use candle_core::{Device, Tensor};

use crate::config::{Conditioner, Config, Continuous, TimeNorm};
use crate::layers::{Cache, Linear, Norm, Shape, Stack};
use crate::weights::Weights;
use crate::{Error, Result};

const LM: &str = "flow_lm";
const CONDITIONS: &str = "flow_lm.condition_provider.conditioners";
/// Width of the flow head's time features.
const TIME_FEATURES: usize = 256;

/// The flow language model and its head.
pub(crate) struct FlowLm {
    stack: Stack,
    /// Each text token's row.
    text: Tensor,
    /// Maps a voice's latents to the model's width.
    speaker: Linear,
    input: Linear,
    out_norm: Norm,
    eos: Linear,
    head: Head,
    /// The conditions picked from tables, summed, `[1, d]`.
    fixed: Tensor,
    /// The conditions read at a value.
    scalars: Vec<Scalar>,
    pub bos: Vec<f32>,
    pub mean: Vec<f32>,
    pub std: Vec<f32>,
    pub ldim: usize,
}

impl FlowLm {
    pub(crate) fn load(w: &Weights, config: &Config) -> Result<Self> {
        let lm = &config.flow_lm;
        let (d, ldim) = (lm.d_model, lm.ldim);
        let shape = Shape {
            dim: d,
            heads: lm.num_heads,
            layers: lm.num_layers,
            ff: lm.dim_feedforward,
            max_period: lm.max_period,
            context: None,
            scaled: false,
            gelu: config.gelu(lm.gelu),
        };
        let text_name = format!("{CONDITIONS}.transcript_in_segment.embed.weight");
        let speaker_name = format!("{CONDITIONS}.speaker_wavs.output_proj.weight");
        Ok(Self {
            stack: Stack::load(w, &format!("{LM}.transformer"), &shape)?,
            text: w.get(&text_name, &[lm.n_bins + 1, d])?,
            speaker: Linear::new(w.get(&speaker_name, &[d, config.mimi.dimension])?),
            input: Linear::load(w, &format!("{LM}.input_linear"), (ldim, d), false)?,
            out_norm: Norm::load(w, &format!("{LM}.out_norm"), d, 1e-5)?,
            eos: Linear::load(w, &format!("{LM}.out_eos"), (d, 1), true)?,
            head: Head::load(w, config)?,
            fixed: fixed(w, config)?,
            scalars: scalars(w, config)?,
            bos: w.vec(&format!("{LM}.bos_emb"), &[ldim])?,
            mean: w.vec(&format!("{LM}.emb_mean"), &[ldim])?,
            std: w.vec(&format!("{LM}.emb_std"), &[ldim])?,
            ldim,
        })
    }

    pub(crate) fn cache(&self) -> Cache {
        self.stack.cache()
    }

    /// A voice's latents, `[frames, 512]`, as the model's prompt rows.
    pub(crate) fn speaker(&self, latents: &Tensor) -> Result<Tensor> {
        self.speaker.forward(latents)
    }

    /// Feeds prompt rows, a voice's or the text's, into `cache`.
    pub(crate) fn prompt(&self, rows: &Tensor, cache: &mut Cache) -> Result<()> {
        self.stack.forward(rows, cache).map(drop)
    }

    /// The rows of `tokens`; an id past the table is refused.
    pub(crate) fn text(&self, tokens: &[u32]) -> Result<Tensor> {
        let rows = self.text.dim(0)?;
        if let Some(id) = tokens.iter().find(|&&t| t as usize >= rows) {
            return Err(Error::Input(format!(
                "token {id} is past the model's table"
            )));
        }
        let ids = Tensor::new(tokens, &Device::Cpu)?;
        Ok(self.text.index_select(&ids, 0)?)
    }

    /// What every frame's input has added at `delta`, `[1, d]`.
    pub(crate) fn conditions(&self, delta: f32) -> Result<Tensor> {
        let mut sum = self.fixed.clone();
        for s in &self.scalars {
            // Others stay at 0, as the reference reads them by default.
            let value = if s.name == DURATION { delta } else { 0.0 };
            let row = features(&s.shape, value)?;
            let row = match &s.proj {
                Some(proj) => proj.forward(&row)?,
                None => row,
            };
            sum = (sum + row)?;
        }
        Ok(sum)
    }

    /// One step: the latent after `latent` from `noise`, and the end logit.
    pub(crate) fn step(
        &self,
        latent: &[f32],
        noise: &[f32],
        conditions: &Tensor,
        cache: &mut Cache,
    ) -> Result<(Vec<f32>, f32)> {
        let x = Tensor::from_slice(latent, (1, self.ldim), &Device::Cpu)?;
        let x = self.input.forward(&x)?.broadcast_add(conditions)?;
        let out = self.out_norm.forward(&self.stack.forward(&x, cache)?)?;
        let eos = self.eos.forward(&out)?.flatten_all()?.to_vec1::<f32>()?;
        let next = self.head.sample(&out, noise)?;
        Ok((next, eos.first().copied().unwrap_or(f32::NEG_INFINITY)))
    }
}

/// The condition that sets the pace: below 0 is faster.
const DURATION: &str = "duration_delta";

/// A condition read at a value, and its projection.
struct Scalar {
    name: String,
    shape: Continuous,
    proj: Option<Linear>,
}

/// The table conditions at their defaults, summed, `[1, d]`.
fn fixed(w: &Weights, config: &Config) -> Result<Tensor> {
    let d = config.flow_lm.d_model;
    let mut sum = Tensor::zeros((1, d), candle_core::DType::F32, &Device::Cpu)?;
    for c in &config.conditioners {
        let Conditioner::Lut { name, lut } = c else {
            continue;
        };
        // One speaker, the default every voice was made with.
        let id = (lut.possible_values.iter())
            .position(|v| v == "1")
            .ok_or_else(|| Error::Load(format!("{name} has no default")))?;
        let table = w.any(&format!("{CONDITIONS}.{name}.embed.weight"))?;
        let table = match table.dims() {
            [_, _] => table,
            _ => return Err(Error::Load(format!("{name} is no table"))),
        };
        let row = table.get(id)?.unsqueeze(0)?;
        let (_, inp) = row.dims2()?;
        let row = match projection(w, name, inp, d)? {
            Some(proj) => proj.forward(&row)?,
            None => row,
        };
        sum = (sum + row)?;
    }
    Ok(sum)
}

/// The conditions read at a value, ready to embed.
fn scalars(w: &Weights, config: &Config) -> Result<Vec<Scalar>> {
    let d = config.flow_lm.d_model;
    let mut out = Vec::new();
    for c in &config.conditioners {
        if let Conditioner::Continuous { name, continuous } = c {
            out.push(Scalar {
                name: name.clone(),
                shape: continuous.clone(),
                proj: projection(w, name, continuous.dim, d)?,
            });
        }
    }
    Ok(out)
}

/// `value`'s sinusoidal features, `[1, dim]`, as the reference embeds it.
fn features(shape: &Continuous, value: f32) -> Result<Tensor> {
    let half = shape.dim / 2;
    let position = shape.scale_factor * value;
    let phases: Vec<f32> = (0..half)
        .map(|i| {
            let e = i as f32 / half.saturating_sub(1).max(1) as f32;
            position / shape.max_period.powf(e)
        })
        .collect();
    let cos = phases.iter().map(|p| p.cos());
    let sin = phases.iter().map(|p| p.sin());
    let feats: Vec<f32> = cos.chain(sin).collect();
    Ok(Tensor::from_vec(feats, (1, shape.dim), &Device::Cpu)?)
}

/// Condition `name`'s projection from `inp` to `d`, when it has one.
fn projection(w: &Weights, name: &str, inp: usize, d: usize) -> Result<Option<Linear>> {
    let key = format!("{CONDITIONS}.{name}.output_proj.weight");
    match w.has(&key) {
        true => Ok(Some(Linear::new(w.get(&key, &[d, inp])?))),
        false => Ok(None),
    }
}

/// One residual block of the head, modulated by the condition.
struct Block {
    norm: Norm,
    fc1: Linear,
    fc2: Linear,
    ada: Linear,
}

/// The flow head: noise and a state in, the velocity towards a latent out.
struct Head {
    /// Each Euler step's two time embeddings, averaged, `[1, w]`.
    times: Vec<Tensor>,
    cond: Linear,
    input: Linear,
    blocks: Vec<Block>,
    final_ada: Linear,
    final_norm: Norm,
    out: Linear,
    width: usize,
    ldim: usize,
}

impl Head {
    fn load(w: &Weights, config: &Config) -> Result<Self> {
        let lm = &config.flow_lm;
        let p = format!("{LM}.flow_net");
        let width = lm.flow_dim;
        let steps = config.lsd_decode_steps;
        let mut times = Vec::with_capacity(steps);
        for i in 0..steps {
            let s = i as f32 / steps as f32;
            let t = (i + 1) as f32 / steps as f32;
            let a = time(w, &format!("{p}.time_embed.0"), s, lm.time_rms_norm, width)?;
            let b = time(w, &format!("{p}.time_embed.1"), t, lm.time_rms_norm, width)?;
            times.push(((a + b)? * 0.5)?);
        }
        let mut blocks = Vec::with_capacity(lm.flow_depth);
        for i in 0..lm.flow_depth {
            let b = format!("{p}.res_blocks.{i}");
            blocks.push(Block {
                norm: Norm::load(w, &format!("{b}.in_ln"), width, 1e-6)?,
                fc1: Linear::load(w, &format!("{b}.mlp.0"), (width, width), true)?,
                fc2: Linear::load(w, &format!("{b}.mlp.2"), (width, width), true)?,
                ada: Linear::load(
                    w,
                    &format!("{b}.adaLN_modulation.1"),
                    (width, 3 * width),
                    true,
                )?,
            });
        }
        let fin = format!("{p}.final_layer");
        Ok(Self {
            times,
            cond: Linear::load(w, &format!("{p}.cond_embed"), (lm.d_model, width), true)?,
            input: Linear::load(w, &format!("{p}.input_proj"), (lm.ldim, width), true)?,
            blocks,
            final_ada: Linear::load(
                w,
                &format!("{fin}.adaLN_modulation.1"),
                (width, 2 * width),
                true,
            )?,
            final_norm: Norm::plain(1e-6),
            out: Linear::load(w, &format!("{fin}.linear"), (width, lm.ldim), true)?,
            width,
            ldim: lm.ldim,
        })
    }

    /// Euler steps from `noise` to a latent, conditioned on `state`.
    fn sample(&self, state: &Tensor, noise: &[f32]) -> Result<Vec<f32>> {
        let cond = self.cond.forward(state)?;
        let mut x = Tensor::from_slice(noise, (1, self.ldim), &Device::Cpu)?;
        let steps = self.times.len() as f64;
        for t in &self.times {
            let y = cond.broadcast_add(t)?;
            let v = self.velocity(&x, &y)?;
            x = (x + (v / steps)?)?;
        }
        Ok(x.flatten_all()?.to_vec1()?)
    }

    fn velocity(&self, x: &Tensor, y: &Tensor) -> Result<Tensor> {
        let w = self.width;
        let y_act = y.silu()?;
        let mut h = self.input.forward(x)?;
        for b in &self.blocks {
            let ada = b.ada.forward(&y_act)?;
            let (shift, scale, gate) = (
                ada.narrow(1, 0, w)?,
                ada.narrow(1, w, w)?,
                ada.narrow(1, 2 * w, w)?,
            );
            let m = modulate(&b.norm.forward(&h)?, &shift, &scale)?;
            let m = b.fc2.forward(&b.fc1.forward(&m)?.silu()?)?;
            h = (h + gate.mul(&m)?)?;
        }
        let ada = self.final_ada.forward(&y_act)?;
        let (shift, scale) = (ada.narrow(1, 0, w)?, ada.narrow(1, w, w)?);
        self.out
            .forward(&modulate(&self.final_norm.forward(&h)?, &shift, &scale)?)
    }
}

/// `x · (1 + scale) + shift`.
fn modulate(x: &Tensor, shift: &Tensor, scale: &Tensor) -> Result<Tensor> {
    Ok(x.mul(&(scale + 1.0)?)?.add(shift)?)
}

/// The embedding of flow time `t` from embedder `p`, `[1, width]`.
fn time(w: &Weights, p: &str, t: f32, norm: TimeNorm, width: usize) -> Result<Tensor> {
    let freqs = w.vec(&format!("{p}.freqs"), &[TIME_FEATURES / 2])?;
    let angles: Vec<f32> = freqs.iter().map(|f| t * f).collect();
    let cos = angles.iter().map(|a| a.cos());
    let feats: Vec<f32> = cos.chain(angles.iter().map(|a| a.sin())).collect();
    let x = Tensor::from_vec(feats, (1, TIME_FEATURES), &Device::Cpu)?;
    let fc1 = Linear::load(w, &format!("{p}.mlp.0"), (TIME_FEATURES, width), true)?;
    let fc2 = Linear::load(w, &format!("{p}.mlp.2"), (width, width), true)?;
    let x = fc2.forward(&fc1.forward(&x)?.silu()?)?;
    let alpha = w.get(&format!("{p}.mlp.3.alpha"), &[width])?;
    let n = width as f64;
    let spread = match norm {
        TimeNorm::Rms => (x.sqr()?.sum_keepdim(1)? / n)?,
        TimeNorm::RmsVar => {
            let mean = (x.sum_keepdim(1)? / n)?;
            (x.broadcast_sub(&mean)?.sqr()?.sum_keepdim(1)? / (n - 1.0))?
        }
    };
    let inv = (spread + 1e-5)?.sqrt()?.recip()?;
    Ok(x.broadcast_mul(&inv)?.broadcast_mul(&alpha)?)
}
