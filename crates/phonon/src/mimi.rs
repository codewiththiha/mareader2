//! Mimi's decoder, a whole run at once; equal to streaming it.

use candle_core::{Device, Tensor};

use crate::config::Config;
use crate::layers::{Shape, Stack};
use crate::weights::Weights;
use crate::{Error, Result};

const MIMI: &str = "mimi";

/// A causal convolution: zeros in front, so no output sees ahead.
struct Conv {
    w: Tensor,
    b: Tensor,
    dilation: usize,
    /// Zeros owed in front.
    pad: usize,
}

impl Conv {
    fn load(
        w: &Weights,
        name: &str,
        (inp, out, k): (usize, usize, usize),
        dilation: usize,
    ) -> Result<Self> {
        Ok(Self {
            w: w.get(&format!("{name}.conv.weight"), &[out, inp, k])?,
            b: w.get(&format!("{name}.conv.bias"), &[out])?,
            dilation,
            pad: (k - 1) * dilation,
        })
    }

    /// `x` is `[1, channels, time]`.
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let x = x.pad_with_zeros(2, self.pad, 0)?;
        let y = x.conv1d(&self.w, 0, 1, self.dilation, 1)?;
        Ok(y.broadcast_add(&self.b.reshape((1, (), 1))?)?)
    }
}

/// A transposed convolution with its tail cut, as the stream drops it.
struct ConvTr {
    w: Tensor,
    b: Tensor,
    stride: usize,
}

impl ConvTr {
    fn load(w: &Weights, name: &str, (inp, out, stride): (usize, usize, usize)) -> Result<Self> {
        Ok(Self {
            w: w.get(&format!("{name}.convtr.weight"), &[inp, out, 2 * stride])?,
            b: w.get(&format!("{name}.convtr.bias"), &[out])?,
            stride,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let (_, _, t) = x.dims3()?;
        let y = x.conv_transpose1d(&self.w, 0, 0, self.stride, 1, 1)?;
        let y = y.narrow(2, 0, t * self.stride)?;
        Ok(y.broadcast_add(&self.b.reshape((1, (), 1))?)?)
    }
}

/// A residual block: two convolutions, each after an ELU.
struct Residual {
    a: Conv,
    b: Conv,
}

impl Residual {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let y = self.a.forward(&x.elu(1.0)?)?;
        let y = self.b.forward(&y.elu(1.0)?)?;
        Ok((x + y)?)
    }
}

/// One upsampling stage and the blocks after it.
struct Stage {
    up: ConvTr,
    blocks: Vec<Residual>,
}

/// The decoder: projection, frame upsampling, transformer and SEANet.
pub(crate) struct Mimi {
    /// The quantizer's output map, `[dim, ldim]`.
    project: Tensor,
    /// The depthwise upsampler's taps, one row of `2 · stride` a channel.
    taps: Vec<f32>,
    stride: usize,
    stack: Stack,
    first: Conv,
    stages: Vec<Stage>,
    last: Conv,
    dim: usize,
}

impl Mimi {
    pub(crate) fn load(w: &Weights, config: &Config) -> Result<Self> {
        let m = &config.mimi;
        let d = m.dimension;
        let hop: usize = m.ratios.iter().product();
        let stride = config.frame_samples() / hop;
        let shape = Shape {
            dim: d,
            heads: m.transformer_num_heads,
            layers: m.transformer_num_layers,
            ff: m.transformer_dim_feedforward,
            max_period: m.transformer_max_period,
            context: Some(m.transformer_context),
            scaled: true,
            gelu: config.gelu(m.gelu),
        };
        let model = format!("{MIMI}.decoder.model");
        let mut mult = 1 << m.ratios.len();
        let first = Conv::load(
            w,
            &format!("{model}.0"),
            (d, mult * m.n_filters, m.kernel_size),
            1,
        )?;
        let mut stages = Vec::with_capacity(m.ratios.len());
        let mut at = 1;
        for &ratio in &m.ratios {
            let (inp, out) = (mult * m.n_filters, mult * m.n_filters / 2);
            let up = ConvTr::load(w, &format!("{model}.{}", at + 1), (inp, out, ratio))?;
            at += 2;
            let mut blocks = Vec::with_capacity(m.n_residual_layers);
            for j in 0..m.n_residual_layers {
                let hidden = out / m.compress;
                let dilation = m.dilation_base.pow(j as u32);
                let k = m.residual_kernel_size;
                let p = format!("{model}.{at}.block");
                blocks.push(Residual {
                    a: Conv::load(w, &format!("{p}.1"), (out, hidden, k), dilation)?,
                    b: Conv::load(w, &format!("{p}.3"), (hidden, out, 1), 1)?,
                });
                at += 1;
            }
            stages.push(Stage { up, blocks });
            mult /= 2;
        }
        let last = Conv::load(
            w,
            &format!("{model}.{}", at + 1),
            (m.n_filters, 1, m.last_kernel_size),
            1,
        )?;
        let project = w.get(
            &format!("{MIMI}.quantizer.output_proj.weight"),
            &[d, m.quantizer_dimension, 1],
        )?;
        let taps = w.vec(
            &format!("{MIMI}.upsample.convtr.convtr.weight"),
            &[d, 1, 2 * stride],
        )?;
        let stack = Stack::load(
            w,
            &format!("{MIMI}.decoder_transformer.transformer"),
            &shape,
        )?;
        Ok(Self {
            project: project.squeeze(2)?,
            taps,
            stride,
            stack,
            first,
            stages,
            last,
            dim: d,
        })
    }

    /// Audio for `latents`, `[frames, ldim]` in the codec's own scale.
    pub(crate) fn decode(&self, latents: &Tensor) -> Result<Vec<f32>> {
        let (frames, _) = latents.dims2()?;
        if frames == 0 {
            return Ok(Vec::new());
        }
        let rows: Vec<f32> = latents
            .matmul(&self.project.t()?)?
            .flatten_all()?
            .to_vec1()?;
        let up = self.upsample(&rows, frames)?;
        let n = frames * self.stride;
        let x = Tensor::from_vec(up, (n, self.dim), &Device::Cpu)?;
        let x = self.stack.forward(&x, &mut self.stack.cache())?;
        let mut z = self.first.forward(&x.t()?.unsqueeze(0)?.contiguous()?)?;
        for stage in &self.stages {
            z = stage.up.forward(&z.elu(1.0)?)?;
            for b in &stage.blocks {
                z = b.forward(&z)?;
            }
        }
        let z = self.last.forward(&z.elu(1.0)?)?;
        Ok(z.flatten_all()?.to_vec1()?)
    }

    /// Each channel spread over `stride` steps a frame, tails overlapping.
    fn upsample(&self, rows: &[f32], frames: usize) -> Result<Vec<f32>> {
        let (d, s) = (self.dim, self.stride);
        let n = frames * s;
        let mut out = vec![0.0f32; n * d];
        for (t, row) in rows.chunks_exact(d).enumerate() {
            for (c, (&x, taps)) in row.iter().zip(self.taps.chunks_exact(2 * s)).enumerate() {
                for (k, &tap) in taps.iter().enumerate() {
                    let at = t * s + k;
                    if at < n {
                        let cell = out
                            .get_mut(at * d + c)
                            .ok_or(Error::Compute("upsample".into()))?;
                        *cell += x * tap;
                    }
                }
            }
        }
        Ok(out)
    }
}
