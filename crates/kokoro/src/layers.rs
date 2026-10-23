//! The building blocks: convolutions, style-led norms and a recurrent layer.

use candle_core::{D, DType, Module, Result, Tensor};
use candle_nn::{Linear, VarBuilder};

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
        Ok(Conv1d {
            weight,
            bias,
            spec: self,
        })
    }

    pub(crate) fn load_transposed(self, vb: &VarBuilder) -> Result<ConvTranspose1d> {
        let groups = self.groups.max(1);
        let weight = conv_weight(vb, (self.inputs, self.outputs / groups, self.kernel))?;
        let bias = self.bias(vb)?;
        Ok(ConvTranspose1d {
            weight,
            bias,
            spec: self,
        })
    }
}

pub(crate) struct Conv1d {
    weight: Tensor,
    bias: Option<Tensor>,
    spec: Conv,
}

impl Conv1d {
    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let s = &self.spec;
        let out = x.conv1d(&self.weight, s.padding, s.stride, s.dilation, s.groups)?;
        add_bias(out, self.bias.as_ref())
    }
}

pub(crate) struct ConvTranspose1d {
    weight: Tensor,
    bias: Option<Tensor>,
    spec: Conv,
}

impl ConvTranspose1d {
    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let s = &self.spec;
        let out = x.conv_transpose1d(
            &self.weight,
            s.padding,
            s.output_padding,
            s.stride,
            1,
            s.groups,
        )?;
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

    /// `x` is `[batch, channels, time]`, `s` is `[batch, style]`.
    pub(crate) fn forward(&self, x: &Tensor, s: &Tensor) -> Result<Tensor> {
        let (gamma, beta) = gamma_beta(&self.fc.forward(s)?, self.features, 2)?;
        self.norm
            .forward(x)?
            .broadcast_mul(&gamma)?
            .broadcast_add(&beta)
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
    hidden_t: Tensor,
    bias: Tensor,
}

impl Cell {
    fn load(vb: &VarBuilder, inputs: usize, hidden: usize, suffix: &str) -> Result<Self> {
        let w_ih = vb.get((4 * hidden, inputs), &format!("weight_ih_l0{suffix}"))?;
        let w_hh = vb.get((4 * hidden, hidden), &format!("weight_hh_l0{suffix}"))?;
        let b_ih = vb.get(4 * hidden, &format!("bias_ih_l0{suffix}"))?;
        let b_hh = vb.get(4 * hidden, &format!("bias_hh_l0{suffix}"))?;
        Ok(Self {
            input_t: w_ih.t()?.contiguous()?,
            hidden_t: w_hh.t()?.contiguous()?,
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
        let mut h = Tensor::zeros((batch, n), x.dtype(), x.device())?;
        let mut c = h.clone();
        let mut outputs = Vec::with_capacity(steps);
        for step in 0..steps {
            let t = if reverse { steps - 1 - step } else { step };
            let gates = (gates_in.narrow(1, t, 1)?.squeeze(1)? + h.matmul(&cell.hidden_t)?)?;
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

    #[test]
    fn weight_norm_rescales_each_output_to_its_gain() -> Result<()> {
        let v = Tensor::new(&[[[3f32, 4.0]], [[0.0, 2.0]]], &Device::Cpu)?;
        let g = Tensor::new(&[10f32, 1.0], &Device::Cpu)?;
        let w = weight_norm(&v, &g)?.flatten_all()?.to_vec1::<f32>()?;
        assert_eq!(w, [6.0, 8.0, 0.0, 1.0]);
        Ok(())
    }
}
