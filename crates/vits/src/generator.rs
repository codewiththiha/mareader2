//! The HiFi-GAN generator: latent frames up to samples.

use candle_core::Tensor;

use crate::dense::{Prep, Signal, each_row};
use crate::layers::{Conv, Weights};
use crate::{Error, Laps, Result};

const SLOPE: f32 = 0.1;

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

    /// The first convolution's kernel, which names the block.
    fn kernel(&self) -> usize {
        match self {
            Self::Pairs(pairs) => pairs.first().map_or(0, |(a, _)| a.kernel()),
            Self::Singles(convs) => convs.first().map_or(0, Conv::kernel),
        }
    }

    fn forward(&self, x: &Signal) -> Result<Signal> {
        let leaky = Prep::leaky(SLOPE);
        // The input is only read, so the first layer needs no copy of it.
        let mut y: Option<Signal> = None;
        match self {
            Self::Pairs(pairs) => {
                for (a, b) in pairs {
                    let input = y.as_ref().unwrap_or(x);
                    let t = a.forward_signal(input, leaky, None)?;
                    y = Some(b.forward_signal(&t, leaky, Some(input))?);
                }
            }
            Self::Singles(convs) => {
                for c in convs {
                    let input = y.as_ref().unwrap_or(x);
                    y = Some(c.forward_signal(input, leaky, Some(input))?);
                }
            }
        }
        y.ok_or_else(|| Error::Load("a block has no convolutions".into()))
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
    pub(crate) fn forward(
        &self,
        z: &Tensor,
        speaker: Option<&Tensor>,
        laps: &mut Laps,
    ) -> Result<Vec<f32>> {
        let z = Signal::from_tensor(z)?;
        let mut x = self.pre.forward_signal(&z, Prep::NONE, None)?;
        if let (Some(cond), Some(g)) = (&self.cond, speaker) {
            x.add_per_channel(&cond.forward(g)?.flatten_all()?.to_vec1::<f32>()?)?;
        }
        // The blocks' mean is taken inside the next layer's input pass.
        laps.lap("generator: pre");
        let mut scale = 1.0;
        for (i, (up, blocks)) in self.stages.iter().enumerate() {
            let prep = Prep {
                scale,
                slope: Some(SLOPE),
            };
            x = up.forward_signal(&x, prep, None)?;
            laps.lap(&format!("stage{i}.up"));
            let mut sum: Option<Signal> = None;
            for block in blocks {
                let y = block.forward(&x)?;
                laps.lap(&format!("stage{i}.k{}", block.kernel()));
                match &mut sum {
                    Some(s) => s.add(&y)?,
                    None => sum = Some(y),
                }
            }
            x = sum.ok_or_else(|| Error::Load("a stage has no blocks".into()))?;
            scale = 1.0 / blocks.len() as f32;
        }
        let prep = Prep {
            scale,
            slope: Some(0.01),
        };
        let mut out = self.post.forward_signal(&x, prep, None)?.data;
        each_row(&mut out, 1 << 14, |_, row| {
            row.iter_mut().for_each(|v| *v = v.tanh());
        });
        laps.lap("post");
        Ok(out)
    }
}
