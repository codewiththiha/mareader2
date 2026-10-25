//! Convolutions shaped by the model file, channel norms and small helpers.

use candle_core::{D, Tensor};

use crate::dense::{DenseConv, Depthwise, Prep, Signal, Transposed};
use crate::onnx::{ConvSpec, Graph};
use crate::{Error, Result};

/// The model's weights, looked up by module path.
pub(crate) struct Weights {
    graph: Graph,
}

impl Weights {
    pub(crate) fn new(graph: Graph) -> Self {
        Self { graph }
    }

    pub(crate) fn get(&self, name: &str) -> Result<Tensor> {
        self.graph
            .tensors
            .get(name)
            .cloned()
            .ok_or_else(|| Error::Load(format!("the model has no {name}")))
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.graph.tensors.contains_key(name)
    }

    /// How many of `{prefix}.0.`, `{prefix}.1.`, … exist, counting up.
    pub(crate) fn count(&self, prefix: &str) -> usize {
        (0..)
            .take_while(|i| {
                let head = format!("{prefix}.{i}.");
                self.graph.tensors.keys().any(|k| k.starts_with(&head))
            })
            .count()
    }

    /// The indices `i` of every `{prefix}.{i}.…` weight, ascending.
    pub(crate) fn indices(&self, prefix: &str) -> Vec<usize> {
        let head = format!("{prefix}.");
        let mut found: Vec<usize> = self
            .graph
            .tensors
            .keys()
            .filter_map(|k| k.strip_prefix(&head)?.split('.').next()?.parse().ok())
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }

    fn spec(&self, module: &str) -> ConvSpec {
        self.graph.convs.get(module).copied().unwrap_or_default()
    }
}

/// A 1-D convolution as the file's node ran it.
pub(crate) enum Conv {
    /// Stride 1 and ungrouped: one wide multiply.
    Dense(DenseConv),
    /// Stride 1, one group per channel: a few multiply-adds per step.
    Depthwise(Depthwise),
    /// Transposed: one wide multiply, then the taps summed into place.
    Transposed(Transposed),
    /// Strided, grouped or transposed: through candle.
    Candle(Strided),
}

pub(crate) struct Strided {
    pub(crate) weight: Tensor,
    pub(crate) bias: Option<Tensor>,
    pub(crate) spec: ConvSpec,
    pub(crate) transposed: bool,
}

impl Conv {
    pub(crate) fn load(w: &Weights, module: &str) -> Result<Self> {
        Self::build(w, module, false)
    }

    pub(crate) fn load_transposed(w: &Weights, module: &str) -> Result<Self> {
        Self::build(w, module, true)
    }

    fn build(w: &Weights, module: &str, transposed: bool) -> Result<Self> {
        let weight = w.get(&format!("{module}.weight"))?;
        if weight.rank() != 3 {
            return Err(Error::Load(format!("{module} is not a 1-D convolution")));
        }
        let bias = w.get(&format!("{module}.bias")).ok();
        let spec = w.spec(module);
        if transposed {
            return Ok(Self::Transposed(Transposed::new(
                &weight,
                bias.as_ref(),
                spec,
            )?));
        }
        if spec.stride == 1 && spec.groups == 1 {
            return Ok(Self::Dense(DenseConv::new(&weight, bias.as_ref(), spec)?));
        }
        if let Some(d) = Depthwise::new(&weight, bias.as_ref(), spec)? {
            return Ok(Self::Depthwise(d));
        }
        Ok(Self::Candle(Strided {
            weight,
            bias,
            spec,
            transposed: false,
        }))
    }

    /// Output channels.
    pub(crate) fn outputs(&self) -> usize {
        match self {
            Self::Dense(d) => d.outputs(),
            Self::Depthwise(d) => d.channels(),
            Self::Transposed(t) => t.outputs(),
            Self::Candle(c) => {
                let at = usize::from(c.transposed);
                c.weight.dims().get(at).copied().unwrap_or(0)
            }
        }
    }

    pub(crate) fn kernel(&self) -> usize {
        match self {
            Self::Dense(d) => d.kernel(),
            Self::Depthwise(d) => d.kernel(),
            Self::Transposed(t) => t.kernel(),
            Self::Candle(c) => c.weight.dims().get(2).copied().unwrap_or(0),
        }
    }

    /// `[1, C, T]` in and out.
    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        match self {
            Self::Dense(d) => d
                .forward(&Signal::from_tensor(x)?, Prep::NONE, None)?
                .into_tensor(),
            Self::Depthwise(d) => d.forward(&Signal::from_tensor(x)?)?.into_tensor(),
            Self::Transposed(t) => t
                .forward(&Signal::from_tensor(x)?, Prep::NONE)?
                .into_tensor(),
            Self::Candle(c) => c.forward(x),
        }
    }

    /// `conv(prep(x)) + residual`, staying on plain buffers where it can.
    pub(crate) fn forward_signal(
        &self,
        x: &Signal,
        prep: Prep,
        residual: Option<&Signal>,
    ) -> Result<Signal> {
        match self {
            Self::Dense(d) => d.forward(x, prep, residual),
            Self::Transposed(t) if residual.is_none() => t.forward(x, prep),
            _ => {
                let y = self.forward(&x.prepped(prep).into_tensor()?)?;
                let mut y = Signal::from_tensor(&y)?;
                if let Some(r) = residual {
                    y.add(r)?;
                }
                Ok(y)
            }
        }
    }
}

impl Strided {
    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let s = self.spec;
        let out = if self.transposed {
            let full = x.conv_transpose1d(&self.weight, 0, 0, s.stride, 1, 1)?;
            let len = full.dim(2)?;
            let keep = len
                .checked_sub(s.pads.0 + s.pads.1)
                .ok_or_else(|| Error::Input("too short to upsample".into()))?;
            full.narrow(2, s.pads.0, keep)?
        } else if s.pads.0 == s.pads.1 {
            x.conv1d(&self.weight, s.pads.0, s.stride, s.dilation, s.groups)?
        } else {
            let x = x.pad_with_zeros(2, s.pads.0, s.pads.1)?;
            x.conv1d(&self.weight, 0, s.stride, s.dilation, s.groups)?
        };
        Ok(match &self.bias {
            Some(b) => out.broadcast_add(&b.reshape((1, (), 1))?)?,
            None => out,
        })
    }
}

/// Normalises each step across channels: `[1, C, T]` in and out.
pub(crate) struct ChannelNorm {
    gamma: Tensor,
    beta: Tensor,
}

impl ChannelNorm {
    const EPS: f64 = 1e-5;

    pub(crate) fn load(w: &Weights, module: &str) -> Result<Self> {
        let gamma = w.get(&format!("{module}.gamma"))?.reshape((1, (), 1))?;
        let beta = w.get(&format!("{module}.beta"))?.reshape((1, (), 1))?;
        Ok(Self { gamma, beta })
    }

    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let mean = x.mean_keepdim(1)?;
        let centred = x.broadcast_sub(&mean)?;
        let var = centred.sqr()?.mean_keepdim(1)?;
        let normed = centred.broadcast_div(&(var + Self::EPS)?.sqrt()?)?;
        Ok(normed
            .broadcast_mul(&self.gamma)?
            .broadcast_add(&self.beta)?)
    }
}

/// Channels in reverse order, as the flows' `Flip` does.
pub(crate) fn flip_channels(x: &Tensor) -> Result<Tensor> {
    let channels = x.dim(1)?;
    let order: Vec<u32> = (0..channels as u32).rev().collect();
    let order = Tensor::new(order.as_slice(), x.device())?;
    Ok(x.index_select(&order, 1)?)
}

/// The last axis softmaxed.
pub(crate) fn softmax(x: &Tensor) -> Result<Tensor> {
    let max = x.max_keepdim(D::Minus1)?;
    let e = x.broadcast_sub(&max)?.exp()?;
    let sum = e.sum_keepdim(D::Minus1)?;
    Ok(e.broadcast_div(&sum)?)
}
