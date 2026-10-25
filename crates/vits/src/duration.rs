//! The stochastic duration predictor, run from noise back to log durations.

use candle_core::Tensor;

use crate::dense::{Prep, Signal};
use crate::fused::gelu;
use crate::layers::{ChannelNorm, Conv, Weights};
use crate::{Error, Result};

/// Dilated depth-wise convolutions, each followed by a point-wise one.
struct Separable {
    layers: Vec<(Conv, ChannelNorm, Conv, ChannelNorm)>,
}

impl Separable {
    fn load(w: &Weights, module: &str) -> Result<Self> {
        let layers = (0..w.count(&format!("{module}.convs_sep")))
            .map(|i| {
                Ok((
                    Conv::load(w, &format!("{module}.convs_sep.{i}"))?,
                    ChannelNorm::load(w, &format!("{module}.norms_1.{i}"))?,
                    Conv::load(w, &format!("{module}.convs_1x1.{i}"))?,
                    ChannelNorm::load(w, &format!("{module}.norms_2.{i}"))?,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self { layers })
    }

    fn forward(&self, x: &Signal) -> Result<Signal> {
        let mut x = x.clone();
        for (sep, norm_1, point, norm_2) in &self.layers {
            let mut y = norm_1.forward_signal(sep.forward_signal(&x, Prep::NONE, None)?)?;
            gelu(&mut y);
            let mut y = norm_2.forward_signal(point.forward_signal(&y, Prep::NONE, None)?)?;
            gelu(&mut y);
            x.add(&y)?;
        }
        Ok(x)
    }
}

/// A coupling step whose second channel moves along a learned spline.
struct SplineFlow {
    pre: Conv,
    convs: Separable,
    proj: Conv,
    bins: usize,
    filters: usize,
}

impl SplineFlow {
    fn load(w: &Weights, module: &str) -> Result<Self> {
        let pre = Conv::load(w, &format!("{module}.pre"))?;
        let proj = Conv::load(w, &format!("{module}.proj"))?;
        let bins = (proj.outputs() + 1) / 3;
        if bins < 2 || bins * 3 - 1 != proj.outputs() {
            return Err(Error::Load(format!("{module} has an odd shape")));
        }
        Ok(Self {
            filters: pre.outputs(),
            pre,
            convs: Separable::load(w, &format!("{module}.convs"))?,
            proj,
            bins,
        })
    }

    /// One inverse step on `[first, second]`, each one value per phoneme.
    fn inverse(&self, first: &[f32], second: &mut [f32], context: &Signal) -> Result<()> {
        let x0 = Signal {
            data: first.to_vec(),
            channels: 1,
            len: first.len(),
        };
        let mut h = self.pre.forward_signal(&x0, Prep::NONE, None)?;
        h.add(context)?;
        let h = self.convs.forward(&h)?;
        let params = self
            .proj
            .forward_signal(&h, Prep::NONE, None)?
            .into_tensor()?
            .squeeze(0)?
            .t()?
            .contiguous()?
            .to_vec2::<f32>()?;
        let scale = 1.0 / (self.filters as f32).sqrt();
        for (x, p) in second.iter_mut().zip(params) {
            let (w, rest) = p.split_at(self.bins.min(p.len()));
            let (h, d) = rest.split_at(self.bins.min(rest.len()));
            let w: Vec<f32> = w.iter().map(|v| v * scale).collect();
            let h: Vec<f32> = h.iter().map(|v| v * scale).collect();
            *x = spline_inverse(*x, &w, &h, d);
        }
        Ok(())
    }
}

const TAIL: f32 = 5.0;
const MIN_BIN: f32 = 1e-3;
const MIN_SLOPE: f32 = 1e-3;

fn softmax(v: &[f32]) -> Vec<f32> {
    let max = v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let e: Vec<f32> = v.iter().map(|x| (x - max).exp()).collect();
    let sum: f32 = e.iter().sum();
    e.iter().map(|x| x / sum).collect()
}

fn softplus(x: f32) -> f32 {
    (-x.abs()).exp().ln_1p() + x.max(0.0)
}

/// Knot positions from unnormalised bin sizes, pinned to `±TAIL`.
fn knots(raw: &[f32]) -> Vec<f32> {
    let bins = raw.len() as f32;
    let mut out = Vec::with_capacity(raw.len() + 1);
    let mut acc = 0.0f32;
    out.push(-TAIL);
    for size in softmax(raw) {
        acc += MIN_BIN + (1.0 - MIN_BIN * bins) * size;
        out.push(2.0 * TAIL * acc - TAIL);
    }
    if let Some(last) = out.last_mut() {
        *last = TAIL;
    }
    out
}

/// The inverse of a monotone rational-quadratic spline with linear tails.
pub(crate) fn spline_inverse(y: f32, widths: &[f32], heights: &[f32], slopes: &[f32]) -> f32 {
    if !(-TAIL..=TAIL).contains(&y) || widths.len() != heights.len() {
        return y;
    }
    let xs = knots(widths);
    let ys = knots(heights);
    let edge = ((1.0f64 - f64::from(MIN_SLOPE)).exp() - 1.0).ln() as f32;
    let slope = |i: usize| {
        let raw = match i.checked_sub(1) {
            Some(j) => slopes.get(j).copied().unwrap_or(edge),
            None => edge,
        };
        MIN_SLOPE + softplus(raw)
    };
    let last = ys.len().saturating_sub(1);
    let bin = ys
        .iter()
        .enumerate()
        .filter(|&(i, &k)| y >= if i == last { k + 1e-6 } else { k })
        .count()
        .saturating_sub(1)
        .min(widths.len().saturating_sub(1));
    let (Some(&x0), Some(&x1), Some(&y0), Some(&y1)) =
        (xs.get(bin), xs.get(bin + 1), ys.get(bin), ys.get(bin + 1))
    else {
        return y;
    };
    let (width, height) = (x1 - x0, y1 - y0);
    let delta = height / width;
    let (d0, d1) = (slope(bin), slope(bin + 1));
    let rise = y - y0;
    let a = rise * (d0 + d1 - 2.0 * delta) + height * (delta - d0);
    let b = height * d0 - rise * (d0 + d1 - 2.0 * delta);
    let c = -delta * rise;
    let disc = (b * b - 4.0 * a * c).max(0.0);
    let root = (2.0 * c) / (-b - disc.sqrt());
    root * width + x0
}

pub(crate) struct DurationPredictor {
    pre: Conv,
    cond: Option<Conv>,
    convs: Separable,
    proj: Conv,
    /// Used last to first, as the reverse pass runs them.
    flows: Vec<SplineFlow>,
    mean: [f32; 2],
    scale: [f32; 2],
}

fn pair(t: &Tensor) -> Result<[f32; 2]> {
    let v = t.flatten_all()?.to_vec1::<f32>()?;
    match v.as_slice() {
        [a, b] => Ok([*a, *b]),
        _ => Err(Error::Load(
            "the duration flow's affine step is not 2 wide".into(),
        )),
    }
}

impl DurationPredictor {
    pub(crate) fn load(w: &Weights) -> Result<Self> {
        if w.has("dp.conv_1.weight") {
            return Err(Error::Load(
                "deterministic duration models aren't supported".into(),
            ));
        }
        let scale = if w.has("dp.flows.0.logs") {
            w.get("dp.flows.0.logs")?.neg()?.exp()?
        } else {
            w.get("dp.flows.0.scale")?
        };
        let mut flows = w
            .indices("dp.flows")
            .into_iter()
            .filter(|&i| i > 1 && w.has(&format!("dp.flows.{i}.pre.weight")))
            .map(|i| SplineFlow::load(w, &format!("dp.flows.{i}")))
            .collect::<Result<Vec<_>>>()?;
        flows.reverse();
        if flows.is_empty() {
            return Err(Error::Load("the duration predictor has no flows".into()));
        }
        Ok(Self {
            pre: Conv::load(w, "dp.pre")?,
            cond: w
                .has("dp.cond.weight")
                .then(|| Conv::load(w, "dp.cond"))
                .transpose()?,
            convs: Separable::load(w, "dp.convs")?,
            proj: Conv::load(w, "dp.proj")?,
            flows,
            mean: pair(&w.get("dp.flows.0.m")?)?,
            scale: pair(&scale)?,
        })
    }

    /// Log frames per phoneme from encoder states `[1, H, T]` and noise.
    pub(crate) fn forward(
        &self,
        hidden: &Tensor,
        speaker: Option<&Tensor>,
        noise: [Vec<f32>; 2],
    ) -> Result<Vec<f32>> {
        let mut x = self.pre.forward(hidden)?;
        if let (Some(cond), Some(g)) = (&self.cond, speaker) {
            x = x.broadcast_add(&cond.forward(g)?)?;
        }
        let x = self.convs.forward(&Signal::from_tensor(&x)?)?;
        let context = self.proj.forward_signal(&x, Prep::NONE, None)?;
        let [mut z0, mut z1] = noise;
        for flow in &self.flows {
            std::mem::swap(&mut z0, &mut z1);
            flow.inverse(&z0, &mut z1, &context)?;
        }
        std::mem::swap(&mut z0, &mut z1);
        let ([m, _], [s, _]) = (self.mean, self.scale);
        Ok(z0.iter().map(|z| (z - m) * s).collect())
    }
}
