//! The building blocks: convolutions, style-led norms and a recurrent layer.

use candle_core::{D, DType, Device, Module, Result, Tensor};
use candle_nn::{Linear, VarBuilder};

use crate::fast::{self, ConvShape, Then};

/// `x` times `v`, any dtype.
pub(crate) fn scale(x: &Tensor, v: f32) -> Result<Tensor> {
    x.affine(f64::from(v), 0.0)
}

pub(crate) fn leaky_relu(x: &Tensor, slope: f32) -> Result<Tensor> {
    x.maximum(&scale(x, slope)?)
}

/// Each step repeated to reach `target` steps along the last axis.
pub(crate) fn upsample(x: &Tensor, target: usize) -> Result<Tensor> {
    let (batch, channels, length) = x.dims3()?;
    if target == length || length == 0 {
        return Ok(x.clone());
    }
    let times = target / length;
    if times * length == target {
        let x = x.unsqueeze(3)?.repeat(&[1, 1, 1, times])?;
        return x.reshape((batch, channels, target));
    }
    x.upsample_nearest1d(target)
}

/// Mean and spread taken along `dim`, then each value centred and scaled.
pub(crate) fn normalised(x: &Tensor, dim: D, eps: f64) -> Result<Tensor> {
    let mean = x.mean_keepdim(dim)?;
    let centred = x.broadcast_sub(&mean)?;
    let var = centred.sqr()?.mean_keepdim(dim)?;
    centred.broadcast_div(&(var + eps)?.sqrt()?)
}

/// A weight-normed weight: its direction `v`, rescaled to length `g`.
fn weight_norm(v: &Tensor, g: &Tensor) -> Result<Tensor> {
    let norm = v
        .sqr()?
        .sum_keepdim(D::Minus1)?
        .sum_keepdim(D::Minus2)?
        .sqrt()?;
    let g = match g.rank() {
        1 => g.unsqueeze(1)?.unsqueeze(2)?,
        _ => g.clone(),
    };
    v.broadcast_div(&norm)?.broadcast_mul(&g)
}

/// A plain weight, else one stored weight-normed.
fn conv_weight(vb: &VarBuilder, shape: (usize, usize, usize)) -> Result<Tensor> {
    if let Ok(w) = vb.get(shape, "weight") {
        return Ok(w);
    }
    let v = vb.get(shape, "weight_v")?;
    let g = vb
        .get((shape.0, 1, 1), "weight_g")
        .or_else(|_| vb.get(shape.0, "weight_g"))?;
    weight_norm(&v, &g)
}

fn add_bias(out: Tensor, bias: Option<&Tensor>) -> Result<Tensor> {
    match bias {
        Some(b) => out.broadcast_add(&b.unsqueeze(0)?.unsqueeze(2)?),
        None => Ok(out),
    }
}

/// A convolution's shape; built up, then loaded.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Conv {
    inputs: usize,
    outputs: usize,
    kernel: usize,
    stride: usize,
    padding: usize,
    dilation: usize,
    groups: usize,
    bias: bool,
    /// Transposed only: extra steps at the end.
    output_padding: usize,
}

impl Conv {
    pub(crate) fn new(inputs: usize, outputs: usize, kernel: usize) -> Self {
        Self {
            inputs,
            outputs,
            kernel,
            stride: 1,
            padding: 0,
            dilation: 1,
            groups: 1,
            bias: true,
            output_padding: 0,
        }
    }

    pub(crate) fn stride(self, stride: usize) -> Self {
        Self { stride, ..self }
    }

    pub(crate) fn padding(self, padding: usize) -> Self {
        Self { padding, ..self }
    }

    pub(crate) fn dilation(self, dilation: usize) -> Self {
        Self { dilation, ..self }
    }

    pub(crate) fn groups(self, groups: usize) -> Self {
        Self { groups, ..self }
    }

    pub(crate) fn output_padding(self, output_padding: usize) -> Self {
        Self {
            output_padding,
            ..self
        }
    }

    pub(crate) fn no_bias(self) -> Self {
        Self {
            bias: false,
            ..self
        }
    }

    fn bias(&self, vb: &VarBuilder) -> Result<Option<Tensor>> {
        self.bias.then(|| vb.get(self.outputs, "bias")).transpose()
    }

    pub(crate) fn load(self, vb: &VarBuilder) -> Result<Conv1d> {
        let groups = self.groups.max(1);
        let weight = conv_weight(vb, (self.outputs, self.inputs / groups, self.kernel))?;
        let bias = self.bias(vb)?;
        let flat = weight.reshape((self.outputs, self.inputs / groups * self.kernel))?;
        Ok(Conv1d {
            flat: flat.contiguous()?,
            bias_values: values(bias.as_ref())?,
            bias,
            spec: self,
        })
    }

    pub(crate) fn load_transposed(self, vb: &VarBuilder) -> Result<ConvTranspose1d> {
        let groups = self.groups.max(1);
        let weight = conv_weight(vb, (self.inputs, self.outputs / groups, self.kernel))?;
        let bias = self.bias(vb)?;
        let flat = weight.reshape((self.inputs, self.outputs / groups * self.kernel))?;
        Ok(ConvTranspose1d {
            flat: flat.contiguous()?,
            bias_values: values(bias.as_ref())?,
            bias,
            spec: self,
        })
    }
}

/// A tensor's values as a plain list, where there is one.
fn values(t: Option<&Tensor>) -> Result<Option<Vec<f32>>> {
    t.map(|t| t.flatten_all()?.to_dtype(DType::F32)?.to_vec1())
        .transpose()
}

/// `x`'s channels and steps, if it is one f32 `[1, channels, time]`.
fn single(x: &Tensor) -> Result<Option<(usize, usize)>> {
    let (batch, channels, len) = x.dims3()?;
    Ok((batch == 1 && x.dtype() == DType::F32 && len > 0).then_some((channels, len)))
}

pub(crate) struct Conv1d {
    /// The weight as `[outputs, inputs · kernel]`, for the gathered product.
    flat: Tensor,
    bias: Option<Tensor>,
    bias_values: Option<Vec<f32>>,
    spec: Conv,
}

impl Conv1d {
    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let s = &self.spec;
        if let (Some((inputs, len)), 1, 1) = (single(x)?, s.stride, s.groups.max(1)) {
            let shape = ConvShape {
                inputs,
                kernel: s.kernel,
                padding: s.padding,
                dilation: s.dilation,
            };
            let bias = self.bias_values.as_deref();
            let (out, out_len) =
                fast::with_values(x, |x| fast::conv(x, len, &shape, &self.flat, bias))?;
            return Tensor::from_vec(out, (1, s.outputs, out_len), &Device::Cpu);
        }
        let groups = s.groups.max(1);
        let weight = (self.flat).reshape((s.outputs, s.inputs / groups, s.kernel))?;
        let out = x.conv1d(&weight, s.padding, s.stride, s.dilation, s.groups)?;
        add_bias(out, self.bias.as_ref())
    }
}

pub(crate) struct ConvTranspose1d {
    /// The weight as `[inputs, outputs · kernel]`.
    flat: Tensor,
    bias: Option<Tensor>,
    bias_values: Option<Vec<f32>>,
    spec: Conv,
}

impl ConvTranspose1d {
    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let s = &self.spec;
        if let (Some((_, len)), 1) = (single(x)?, s.groups.max(1)) {
            // One product for every step and tap, then summed into place.
            let products = x.squeeze(0)?.t()?.matmul(&self.flat)?;
            let full = (len - 1) * s.stride + s.kernel;
            let Some(out_len) = (full + s.output_padding).checked_sub(2 * s.padding) else {
                candle_core::bail!("{len} steps is too short for the transposed conv");
            };
            let bias = self.bias_values.as_deref();
            let out = fast::with_values(&products, |p| {
                let layout = (s.stride, s.padding, out_len);
                Ok(fast::overlap_add(p, (s.outputs, s.kernel), layout, bias))
            })?;
            return Tensor::from_vec(out, (1, s.outputs, out_len), &Device::Cpu);
        }
        let groups = s.groups.max(1);
        let weight = (self.flat).reshape((s.inputs, s.outputs / groups, s.kernel))?;
        let out =
            x.conv_transpose1d(&weight, s.padding, s.output_padding, s.stride, 1, s.groups)?;
        add_bias(out, self.bias.as_ref())
    }
}

/// Each channel normalised over time, with an optional learned scale.
struct InstanceNorm {
    weight: Option<Tensor>,
    bias: Option<Tensor>,
}

impl InstanceNorm {
    fn load(features: usize, vb: &VarBuilder) -> Self {
        Self {
            weight: vb.get(features, "weight").ok(),
            bias: vb.get(features, "bias").ok(),
        }
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let out = normalised(&x.to_dtype(DType::F32)?, D::Minus1, 1e-5)?.to_dtype(x.dtype())?;
        let out = match &self.weight {
            Some(w) => out.broadcast_mul(&w.unsqueeze(0)?.unsqueeze(2)?)?,
            None => out,
        };
        add_bias(out, self.bias.as_ref())
    }
}

/// Splits a style projection into a scale and a shift, `(1 + γ, β)`.
fn gamma_beta(h: &Tensor, features: usize, axis: usize) -> Result<(Tensor, Tensor)> {
    let gamma = (h.narrow(1, 0, features)?.unsqueeze(axis)? + 1.0)?;
    let beta = h.narrow(1, features, features)?.unsqueeze(axis)?;
    Ok((gamma, beta))
}

/// Instance norm whose scale and shift come from the voice's style.
pub(crate) struct AdaIn1d {
    norm: InstanceNorm,
    fc: Linear,
    features: usize,
}

impl AdaIn1d {
    pub(crate) fn load(style: usize, features: usize, vb: &VarBuilder) -> Result<Self> {
        Ok(Self {
            norm: InstanceNorm::load(features, &vb.pp("norm")),
            fc: candle_nn::linear(style, features * 2, vb.pp("fc"))?,
            features,
        })
    }

    /// `x` `[batch, channels, time]` normed in style `s`, then `then`.
    pub(crate) fn forward_then(&self, x: &Tensor, s: &Tensor, then: Then<'_>) -> Result<Tensor> {
        let h = self.fc.forward(s)?;
        let Some((channels, len)) = single(x)?.filter(|_| h.dim(0).ok() == Some(1)) else {
            let (gamma, beta) = gamma_beta(&h, self.features, 2)?;
            let out = self.norm.forward(x)?.broadcast_mul(&gamma)?;
            return then_slowly(&out.broadcast_add(&beta)?, then);
        };
        // The norm's own scale and shift, then the style's, as one of each.
        let h: Vec<f32> = h.flatten_all()?.to_vec1()?;
        let own = |t: &Option<Tensor>| values(t.as_ref());
        let (weight, bias) = (own(&self.norm.weight)?, own(&self.norm.bias)?);
        let (mut scale, mut shift) = (vec![0f32; channels], vec![0f32; channels]);
        for (c, (a, b)) in scale.iter_mut().zip(&mut shift).enumerate() {
            let gamma = 1.0 + h.get(c).copied().unwrap_or(0.0);
            let beta = h.get(self.features + c).copied().unwrap_or(0.0);
            let w = weight
                .as_ref()
                .and_then(|w| w.get(c))
                .copied()
                .unwrap_or(1.0);
            let bias = bias.as_ref().and_then(|b| b.get(c)).copied().unwrap_or(0.0);
            (*a, *b) = (w * gamma, bias * gamma + beta);
        }
        let out = fast::with_values(x, |x| {
            Ok(fast::norm_rows(x, len, (&scale, &shift), 1e-5, then))
        })?;
        Tensor::from_vec(out, (1, channels, len), &Device::Cpu)
    }
}

/// `then` on a whole tensor, where the hand-written loops don't apply.
fn then_slowly(x: &Tensor, then: Then<'_>) -> Result<Tensor> {
    match then {
        Then::Leaky(slope) => leaky_relu(x, slope),
        Then::Snake(alphas) => {
            let alpha = Tensor::from_slice(alphas, (1, alphas.len(), 1), x.device())?;
            let wave = x.broadcast_mul(&alpha)?.sin()?.sqr()?;
            x + wave.broadcast_mul(&alpha.recip()?)?
        }
    }
}

/// Layer norm over channels, scaled and shifted by the style.
pub(crate) struct AdaLayerNorm {
    fc: Linear,
    features: usize,
}

impl AdaLayerNorm {
    pub(crate) fn load(style: usize, features: usize, vb: &VarBuilder) -> Result<Self> {
        Ok(Self {
            fc: candle_nn::linear(style, features * 2, vb.pp("fc"))?,
            features,
        })
    }

    /// `x` is `[batch, time, channels]`.
    pub(crate) fn forward(&self, x: &Tensor, s: &Tensor) -> Result<Tensor> {
        let (gamma, beta) = gamma_beta(&self.fc.forward(s)?, self.features, 1)?;
        normalised(&x.to_dtype(DType::F32)?, D::Minus1, 1e-5)?
            .to_dtype(x.dtype())?
            .broadcast_mul(&gamma)?
            .broadcast_add(&beta)
    }
}

/// Layer norm over the last axis with stored `gamma` and `beta`.
pub(crate) struct ChannelNorm {
    gamma: Tensor,
    beta: Tensor,
}

impl ChannelNorm {
    pub(crate) fn load(features: usize, vb: &VarBuilder) -> Result<Self> {
        Ok(Self {
            gamma: vb.get(features, "gamma")?,
            beta: vb.get(features, "beta")?,
        })
    }

    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        normalised(&x.to_dtype(DType::F32)?, D::Minus1, 1e-5)?
            .to_dtype(x.dtype())?
            .broadcast_mul(&self.gamma)?
            .broadcast_add(&self.beta)
    }
}

/// One direction's weights, input and hidden side pre-transposed.
struct Cell {
    input_t: Tensor,
    bias: Tensor,
    rows: fast::CellRows,
}

impl Cell {
    fn load(vb: &VarBuilder, inputs: usize, hidden: usize, suffix: &str) -> Result<Self> {
        let w_ih = vb.get((4 * hidden, inputs), &format!("weight_ih_l0{suffix}"))?;
        let w_hh = vb.get((4 * hidden, hidden), &format!("weight_hh_l0{suffix}"))?;
        let b_ih = vb.get(4 * hidden, &format!("bias_ih_l0{suffix}"))?;
        let b_hh = vb.get(4 * hidden, &format!("bias_hh_l0{suffix}"))?;
        Ok(Self {
            input_t: w_ih.t()?.contiguous()?,
            rows: fast::CellRows {
                hidden_t: w_hh.t()?.flatten_all()?.to_vec1()?,
            },
            bias: (b_ih + b_hh)?,
        })
    }
}

/// A one-layer LSTM read both ways, the two outputs side by side.
pub(crate) struct BiLstm {
    forward: Cell,
    backward: Cell,
    hidden: usize,
}

impl BiLstm {
    pub(crate) fn load(inputs: usize, hidden: usize, vb: &VarBuilder) -> Result<Self> {
        Ok(Self {
            forward: Cell::load(vb, inputs, hidden, "")?,
            backward: Cell::load(vb, inputs, hidden, "_reverse")?,
            hidden,
        })
    }

    /// `[batch, time, inputs]` to `[batch, time, 2 × hidden]`.
    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let (batch, steps, inputs) = x.dims3()?;
        if batch == 1 && steps > 0 && x.dtype() == DType::F32 {
            let n = self.hidden;
            let gates = |cell: &Cell| -> Result<Vec<f32>> {
                let rows = x.reshape((steps, inputs))?.matmul(&cell.input_t)?;
                rows.broadcast_add(&cell.bias)?.flatten_all()?.to_vec1()
            };
            let (ahead, back) = (gates(&self.forward)?, gates(&self.backward)?);
            // The two directions share nothing, so each takes a core.
            let (ahead, back) = std::thread::scope(|s| {
                let run = || fast::lstm_run(&self.backward.rows, &back, n, true);
                let named = std::thread::Builder::new().name("kokoro lstm".into());
                let beside = named.spawn_scoped(s, run).ok();
                let ahead = fast::lstm_run(&self.forward.rows, &ahead, n, false);
                let back = beside.and_then(|b| b.join().ok());
                (ahead, back.unwrap_or_else(run))
            });
            let mut out = Vec::with_capacity(steps * 2 * n);
            for (a, b) in ahead.chunks_exact(n).zip(back.chunks_exact(n)) {
                out.extend_from_slice(a);
                out.extend_from_slice(b);
            }
            return Tensor::from_vec(out, (1, steps, 2 * n), &Device::Cpu);
        }
        self.forward_slowly(x)
    }

    /// Step by step in tensors, for any batch.
    fn forward_slowly(&self, x: &Tensor) -> Result<Tensor> {
        let ahead = self.run(x, &self.forward, false)?;
        let back = self.run(x, &self.backward, true)?;
        Tensor::cat(&[&ahead, &back], 2)
    }

    fn run(&self, x: &Tensor, cell: &Cell, reverse: bool) -> Result<Tensor> {
        let (batch, steps, inputs) = x.dims3()?;
        let n = self.hidden;
        if steps == 0 {
            return Tensor::zeros((batch, 0, n), x.dtype(), x.device());
        }
        let gates_in = x
            .reshape((batch * steps, inputs))?
            .matmul(&cell.input_t)?
            .broadcast_add(&cell.bias)?
            .reshape((batch, steps, 4 * n))?;
        let hidden_t = Tensor::from_slice(&cell.rows.hidden_t, (n, 4 * n), x.device())?;
        let hidden_t = hidden_t.to_dtype(x.dtype())?;
        let mut h = Tensor::zeros((batch, n), x.dtype(), x.device())?;
        let mut c = h.clone();
        let mut outputs = Vec::with_capacity(steps);
        for step in 0..steps {
            let t = if reverse { steps - 1 - step } else { step };
            let gates = (gates_in.narrow(1, t, 1)?.squeeze(1)? + h.matmul(&hidden_t)?)?;
            let i = candle_nn::ops::sigmoid(&gates.narrow(1, 0, n)?)?;
            let f = candle_nn::ops::sigmoid(&gates.narrow(1, n, n)?)?;
            let g = gates.narrow(1, 2 * n, n)?.tanh()?;
            let o = candle_nn::ops::sigmoid(&gates.narrow(1, 3 * n, n)?)?;
            c = ((f * &c)? + (i * g)?)?;
            h = (o * c.tanh()?)?;
            outputs.push(h.unsqueeze(1)?);
        }
        if reverse {
            outputs.reverse();
        }
        Tensor::cat(&outputs, 1)
    }
}

/// A linear layer: `x · wᵀ + b`, broadcast over leading axes.
pub(crate) struct LinearNorm {
    weight_t: Tensor,
    bias: Option<Tensor>,
}

impl LinearNorm {
    pub(crate) fn load(inputs: usize, outputs: usize, vb: &VarBuilder) -> Result<Self> {
        Ok(Self {
            weight_t: vb.get((outputs, inputs), "weight")?.t()?,
            bias: vb.get(outputs, "bias").ok(),
        })
    }

    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let out = x.broadcast_matmul(&self.weight_t)?;
        match &self.bias {
            Some(b) => out.broadcast_add(b),
            None => Ok(out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::Device;

    #[test]
    fn upsampling_repeats_each_step() -> Result<()> {
        let x = Tensor::new(&[[[1f32, 2.0, 3.0]]], &Device::Cpu)?;
        let up = upsample(&x, 6)?.flatten_all()?.to_vec1::<f32>()?;
        assert_eq!(up, [1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
        Ok(())
    }

    #[test]
    fn leaky_relu_scales_only_negatives() -> Result<()> {
        let x = Tensor::new(&[-2f32, 0.0, 3.0], &Device::Cpu)?;
        assert_eq!(leaky_relu(&x, 0.1)?.to_vec1::<f32>()?, [-0.2, 0.0, 3.0]);
        Ok(())
    }

    /// Smooth made-up values, `count` of them, varied by `seed`.
    fn wavy(count: usize, seed: f32) -> Vec<f32> {
        (0..count).map(|i| (i as f32 * seed).sin() * 0.5).collect()
    }

    fn builder(named: Vec<(String, Vec<usize>, f32)>) -> Result<VarBuilder<'static>> {
        let mut all = std::collections::HashMap::new();
        for (name, shape, seed) in named {
            let count = shape.iter().product();
            let t = Tensor::from_vec(wavy(count, seed), shape, &Device::Cpu)?;
            all.insert(name, t);
        }
        Ok(VarBuilder::from_tensors(all, DType::F32, &Device::Cpu))
    }

    #[test]
    fn the_written_out_lstm_matches_the_tensor_one() -> Result<()> {
        let (inputs, hidden, steps) = (6, 5, 40);
        let mut named = Vec::new();
        for (suffix, seed) in [("", 0.3), ("_reverse", 0.7)] {
            let name = |n: &str| format!("{n}{suffix}");
            named.push((name("weight_ih_l0"), vec![4 * hidden, inputs], seed));
            named.push((name("weight_hh_l0"), vec![4 * hidden, hidden], seed + 0.1));
            named.push((name("bias_ih_l0"), vec![4 * hidden], seed + 0.2));
            named.push((name("bias_hh_l0"), vec![4 * hidden], seed + 0.3));
        }
        let lstm = BiLstm::load(inputs, hidden, &builder(named)?)?;
        let x = Tensor::from_vec(wavy(steps * inputs, 0.13), (1, steps, inputs), &Device::Cpu)?;
        let fast = lstm.forward(&x)?.flatten_all()?.to_vec1::<f32>()?;
        let slow = lstm.forward_slowly(&x)?.flatten_all()?.to_vec1::<f32>()?;
        assert_eq!(fast.len(), slow.len());
        let worst = fast
            .iter()
            .zip(&slow)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(worst < 1e-5, "off by {worst}");
        Ok(())
    }

    #[test]
    fn the_written_out_norm_matches_the_tensor_one() -> Result<()> {
        let (style, channels, len) = (4, 3, 300);
        let vb = builder(vec![
            ("norm.weight".to_owned(), vec![channels], 0.4),
            ("norm.bias".to_owned(), vec![channels], 0.9),
            ("fc.weight".to_owned(), vec![2 * channels, style], 0.2),
            ("fc.bias".to_owned(), vec![2 * channels], 0.6),
        ])?;
        let norm = AdaIn1d::load(style, channels, &vb)?;
        let x = Tensor::from_vec(wavy(channels * len, 0.05), (1, channels, len), &Device::Cpu)?;
        let s = Tensor::from_vec(wavy(style, 1.1), (1, style), &Device::Cpu)?;
        let (gamma, beta) = gamma_beta(&norm.fc.forward(&s)?, channels, 2)?;
        let plain = norm
            .norm
            .forward(&x)?
            .broadcast_mul(&gamma)?
            .broadcast_add(&beta)?;
        let alphas = [0.5, 1.0, 2.0];
        for then in [Then::Leaky(0.2), Then::Snake(&alphas)] {
            let want = then_slowly(&plain, then)?.flatten_all()?.to_vec1::<f32>()?;
            let got = norm
                .forward_then(&x, &s, then)?
                .flatten_all()?
                .to_vec1::<f32>()?;
            let worst = got
                .iter()
                .zip(&want)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(worst < 1e-5, "off by {worst}");
        }
        Ok(())
    }

    #[test]
    fn weight_norm_rescales_each_output_to_its_gain() -> Result<()> {
        let v = Tensor::new(&[[[3f32, 4.0]], [[0.0, 2.0]]], &Device::Cpu)?;
        let g = Tensor::new(&[10f32, 1.0], &Device::Cpu)?;
        let w = weight_norm(&v, &g)?.flatten_all()?.to_vec1::<f32>()?;
        assert_eq!(w, [6.0, 8.0, 0.0, 1.0]);
        Ok(())
    }
}
