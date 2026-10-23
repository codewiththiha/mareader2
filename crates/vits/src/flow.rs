//! The coupling flow, run from the prior's sample back to the decoder's input.

use candle_core::Tensor;
use candle_nn::ops::sigmoid;

use crate::layers::{Conv, Weights, flip_channels};
use crate::{Error, Result};

/// WaveNet-style gated convolutions with residual and skip outputs.
struct WaveNet {
    inputs: Vec<Conv>,
    skips: Vec<Conv>,
    cond: Option<Conv>,
}

impl WaveNet {
    fn load(w: &Weights, module: &str) -> Result<Self> {
        let layers = w.count(&format!("{module}.in_layers"));
        let conv = |kind: &str, i: usize| Conv::load(w, &format!("{module}.{kind}.{i}"));
        Ok(Self {
            inputs: (0..layers)
                .map(|i| conv("in_layers", i))
                .collect::<Result<_>>()?,
            skips: (0..layers)
                .map(|i| conv("res_skip_layers", i))
                .collect::<Result<_>>()?,
            cond: w
                .has(&format!("{module}.cond_layer.weight"))
                .then(|| Conv::load(w, &format!("{module}.cond_layer")))
                .transpose()?,
        })
    }

    fn forward(&self, x: &Tensor, speaker: Option<&Tensor>) -> Result<Tensor> {
        let hidden = x.dim(1)?;
        let cond = match (&self.cond, speaker) {
            (Some(c), Some(g)) => Some(c.forward(g)?),
            _ => None,
        };
        let mut x = x.clone();
        let mut out = x.zeros_like()?;
        let last = self.inputs.len().saturating_sub(1);
        for (i, (input, skip)) in self.inputs.iter().zip(&self.skips).enumerate() {
            let mut gate = input.forward(&x)?;
            if let Some(c) = &cond {
                gate = gate.broadcast_add(&c.narrow(1, i * 2 * hidden, 2 * hidden)?)?;
            }
            let acts =
                (gate.narrow(1, 0, hidden)?.tanh()? * sigmoid(&gate.narrow(1, hidden, hidden)?)?)?;
            let res_skip = skip.forward(&acts)?;
            if i < last {
                x = (x + res_skip.narrow(1, 0, hidden)?)?;
                out = (out + res_skip.narrow(1, hidden, hidden)?)?;
            } else {
                out = (out + res_skip)?;
            }
        }
        Ok(out)
    }
}

/// Shifts the second half of the channels by a mean the first half predicts.
struct Coupling {
    pre: Conv,
    net: WaveNet,
    post: Conv,
}

impl Coupling {
    fn inverse(&self, z: &Tensor, speaker: Option<&Tensor>) -> Result<Tensor> {
        let half = z.dim(1)? / 2;
        let x0 = z.narrow(1, 0, half)?;
        let x1 = z.narrow(1, half, half)?;
        let h = self.net.forward(&self.pre.forward(&x0)?, speaker)?;
        let mean = self.post.forward(&h)?;
        Ok(Tensor::cat(&[&x0, &(x1 - mean)?], 1)?)
    }
}

pub(crate) struct Flow {
    /// Used last to first, as the reverse pass runs them.
    couplings: Vec<Coupling>,
}

impl Flow {
    pub(crate) fn load(w: &Weights) -> Result<Self> {
        let mut couplings = w
            .indices("flow.flows")
            .into_iter()
            .map(|i| {
                let module = format!("flow.flows.{i}");
                Ok(Coupling {
                    pre: Conv::load(w, &format!("{module}.pre"))?,
                    net: WaveNet::load(w, &format!("{module}.enc"))?,
                    post: Conv::load(w, &format!("{module}.post"))?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        couplings.reverse();
        if couplings.is_empty() {
            return Err(Error::Load("the model has no flow".into()));
        }
        Ok(Self { couplings })
    }

    pub(crate) fn inverse(&self, z: &Tensor, speaker: Option<&Tensor>) -> Result<Tensor> {
        let mut z = z.clone();
        for coupling in &self.couplings {
            z = coupling.inverse(&flip_channels(&z)?, speaker)?;
        }
        Ok(z)
    }
}
