//! The coupling flow, run from the prior's sample back to the decoder's input.

use candle_core::Tensor;

use crate::dense::{Prep, Signal};
use crate::fused::gated;
use crate::layers::{Conv, Weights};
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

    fn forward(&self, x: &Signal, speaker: Option<&Tensor>) -> Result<Signal> {
        let hidden = x.channels;
        let cond = match (&self.cond, speaker) {
            (Some(c), Some(g)) => Some(c.forward(g)?.flatten_all()?.to_vec1::<f32>()?),
            _ => None,
        };
        let mut x = x.clone();
        let mut out = Signal {
            data: vec![0.0; x.data.len()],
            ..x
        };
        let last = self.inputs.len().saturating_sub(1);
        for (i, (input, skip)) in self.inputs.iter().zip(&self.skips).enumerate() {
            let gate = input.forward_signal(&x, Prep::NONE, None)?;
            let bias = match &cond {
                Some(c) => Some(
                    c.get(i * 2 * hidden..(i + 1) * 2 * hidden)
                        .ok_or_else(|| Error::Compute("the speaker's input is short".into()))?,
                ),
                None => None,
            };
            let res_skip = skip.forward_signal(&gated(&gate, bias)?, Prep::NONE, None)?;
            if i < last {
                x.add_rows(&res_skip, 0)?;
                out.add_rows(&res_skip, hidden)?;
            } else {
                out.add(&res_skip)?;
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
    /// The reverse step, on channels flipped first as `Flip` leaves them.
    fn inverse(&self, z: &Signal, speaker: Option<&Tensor>) -> Result<Signal> {
        let z = flipped(z);
        let half = z.channels / 2;
        let x0 = z.rows(0, half)?;
        let h = self
            .net
            .forward(&self.pre.forward_signal(&x0, Prep::NONE, None)?, speaker)?;
        let mean = self.post.forward_signal(&h, Prep::NONE, None)?;
        let mut out = z;
        let len = out.len;
        let second = out.data.get_mut(half * len..).unwrap_or_default();
        if second.len() != mean.data.len() {
            return Err(Error::Compute("a coupling's halves differ".into()));
        }
        second.iter_mut().zip(&mean.data).for_each(|(v, m)| *v -= m);
        Ok(out)
    }
}

/// Channels in reverse order.
fn flipped(x: &Signal) -> Signal {
    let mut data = Vec::with_capacity(x.data.len());
    for row in x.data.chunks(x.len.max(1)).rev() {
        data.extend_from_slice(row);
    }
    Signal { data, ..*x }
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
        let mut z = Signal::from_tensor(z)?;
        for coupling in &self.couplings {
            z = coupling.inverse(&z, speaker)?;
        }
        z.into_tensor()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flip_reverses_channels() {
        let x = Signal {
            data: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            channels: 3,
            len: 2,
        };
        assert_eq!(flipped(&x).data, [5.0, 6.0, 3.0, 4.0, 1.0, 2.0]);
    }
}
