//! The HiFi-GAN generator: latent frames up to samples.

use candle_core::Tensor;

use crate::layers::{Conv, Weights, leaky_relu};
use crate::{Error, Result};

const SLOPE: f64 = 0.1;

/// A residual block: pairs of convolutions, or single ones in the smaller kind.
enum ResBlock {
    Pairs(Vec<(Conv, Conv)>),
    Singles(Vec<Conv>),
}

impl ResBlock {
    fn load(w: &Weights, module: &str) -> Result<Self> {
        let conv = |kind: &str, i: usize| Conv::load(w, &format!("{module}.{kind}.{i}"));
        let pairs = w.count(&format!("{module}.convs1"));
        if pairs > 0 {
            let pairs = (0..pairs)
                .map(|i| Ok((conv("convs1", i)?, conv("convs2", i)?)))
                .collect::<Result<_>>()?;
            return Ok(Self::Pairs(pairs));
        }
        let singles = (0..w.count(&format!("{module}.convs")))
            .map(|i| conv("convs", i))
            .collect::<Result<Vec<_>>>()?;
        if singles.is_empty() {
            return Err(Error::Load(format!("{module} has no convolutions")));
        }
        Ok(Self::Singles(singles))
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let mut x = x.clone();
        match self {
            Self::Pairs(pairs) => {
                for (a, b) in pairs {
                    let t = a.forward(&leaky_relu(&x, SLOPE)?)?;
                    x = (b.forward(&leaky_relu(&t, SLOPE)?)? + x)?;
                }
            }
            Self::Singles(convs) => {
                for c in convs {
                    x = (c.forward(&leaky_relu(&x, SLOPE)?)? + x)?;
                }
            }
        }
        Ok(x)
    }
}

pub(crate) struct Generator {
    pre: Conv,
    cond: Option<Conv>,
    /// Each upsampling step with the blocks averaged after it.
    stages: Vec<(Conv, Vec<ResBlock>)>,
    post: Conv,
}

impl Generator {
    pub(crate) fn load(w: &Weights) -> Result<Self> {
        let ups = w.count("dec.ups");
        let blocks = w.count("dec.resblocks");
        let per_stage = blocks.checked_div(ups).unwrap_or(0);
        if per_stage == 0 || per_stage * ups != blocks {
            return Err(Error::Load("the generator has an odd shape".into()));
        }
        let stages = (0..ups)
            .map(|i| {
                let up = Conv::load_transposed(w, &format!("dec.ups.{i}"))?;
                let blocks = (0..per_stage)
                    .map(|j| ResBlock::load(w, &format!("dec.resblocks.{}", i * per_stage + j)))
                    .collect::<Result<_>>()?;
                Ok((up, blocks))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            pre: Conv::load(w, "dec.conv_pre")?,
            cond: w
                .has("dec.cond.weight")
                .then(|| Conv::load(w, "dec.cond"))
                .transpose()?,
            stages,
            post: Conv::load(w, "dec.conv_post")?,
        })
    }

    /// Samples in −1..1 from latent frames `[1, C, F]`.
    pub(crate) fn forward(&self, z: &Tensor, speaker: Option<&Tensor>) -> Result<Vec<f32>> {
        let mut x = self.pre.forward(z)?;
        if let (Some(cond), Some(g)) = (&self.cond, speaker) {
            x = x.broadcast_add(&cond.forward(g)?)?;
        }
        for (up, blocks) in &self.stages {
            x = up.forward(&leaky_relu(&x, SLOPE)?)?;
            let mut sum: Option<Tensor> = None;
            for block in blocks {
                let y = block.forward(&x)?;
                sum = Some(match sum {
                    Some(s) => (s + y)?,
                    None => y,
                });
            }
            if let Some(sum) = sum {
                x = sum.affine(1.0 / blocks.len() as f64, 0.0)?;
            }
        }
        let x = self.post.forward(&leaky_relu(&x, 0.01)?)?.tanh()?;
        Ok(x.flatten_all()?.to_vec1::<f32>()?)
    }
}
