//! Linear maps, layer norms and the streaming transformer both halves share.

use candle_core::{D, Tensor};

use crate::Result;
use crate::config::Gelu;
use crate::weights::Weights;

/// `x · wᵀ + b`, over rows.
pub(crate) struct Linear {
    w: Tensor,
    b: Option<Tensor>,
}

impl Linear {
    /// `name.weight`, `[out, inp]`, and its bias if `bias`.
    pub(crate) fn load(
        w: &Weights,
        name: &str,
        (inp, out): (usize, usize),
        bias: bool,
    ) -> Result<Self> {
        let weight = w.get(&format!("{name}.weight"), &[out, inp])?;
        let b = match bias {
            true => Some(w.get(&format!("{name}.bias"), &[out])?),
            false => None,
        };
        Ok(Self { w: weight, b })
    }

    pub(crate) fn new(w: Tensor) -> Self {
        Self { w, b: None }
    }

    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let y = x.matmul(&self.w.t()?)?;
        Ok(match &self.b {
            Some(b) => y.broadcast_add(b)?,
            None => y,
        })
    }
}

/// A layer norm over the last axis; no affine part when `w` is None.
pub(crate) struct Norm {
    affine: Option<(Tensor, Tensor)>,
    eps: f64,
}

impl Norm {
    pub(crate) fn load(w: &Weights, name: &str, dim: usize, eps: f64) -> Result<Self> {
        let weight = w.get(&format!("{name}.weight"), &[dim])?;
        let bias = w.get(&format!("{name}.bias"), &[dim])?;
        Ok(Self {
            affine: Some((weight, bias)),
            eps,
        })
    }

    pub(crate) fn plain(eps: f64) -> Self {
        Self { affine: None, eps }
    }

    pub(crate) fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let mean = x.mean_keepdim(D::Minus1)?;
        let centred = x.broadcast_sub(&mean)?;
        let var = centred.sqr()?.mean_keepdim(D::Minus1)?;
        let y = centred.broadcast_div(&(var + self.eps)?.sqrt()?)?;
        Ok(match &self.affine {
            Some((w, b)) => y.broadcast_mul(w)?.broadcast_add(b)?,
            None => y,
        })
    }
}

/// One pre-norm transformer layer.
struct Layer {
    norm1: Norm,
    norm2: Norm,
    in_proj: Linear,
    out_proj: Linear,
    linear1: Linear,
    linear2: Linear,
    /// Per-channel gains on each branch, as Mimi's layers have.
    scales: Option<(Tensor, Tensor)>,
}

/// Rows past which attention is split over threads.
const PARALLEL_ROWS: usize = 64;

/// Keys and values seen so far, one flat `[position, dim]` list a layer.
#[derive(Clone)]
pub(crate) struct Cache {
    keys: Vec<Vec<f32>>,
    values: Vec<Vec<f32>>,
    len: usize,
}

impl Cache {
    /// Positions held.
    pub(crate) fn len(&self) -> usize {
        self.len
    }
}

/// How a stack is shaped.
pub(crate) struct Shape {
    pub dim: usize,
    pub heads: usize,
    pub layers: usize,
    pub ff: usize,
    pub max_period: f32,
    /// How far back a position looks; None for all the way.
    pub context: Option<usize>,
    pub scaled: bool,
    pub gelu: Gelu,
}

/// Causal self-attention layers with rotary positions, run a few at a time.
pub(crate) struct Stack {
    layers: Vec<Layer>,
    dim: usize,
    heads: usize,
    context: Option<usize>,
    gelu: Gelu,
    /// Rotary frequencies for one head's pairs.
    freqs: Vec<f32>,
}

impl Stack {
    pub(crate) fn load(w: &Weights, prefix: &str, shape: &Shape) -> Result<Self> {
        let d = shape.dim;
        let mut layers = Vec::with_capacity(shape.layers);
        for i in 0..shape.layers {
            let p = format!("{prefix}.layers.{i}");
            let scales = match shape.scaled {
                true => Some((
                    w.get(&format!("{p}.layer_scale_1.scale"), &[d])?,
                    w.get(&format!("{p}.layer_scale_2.scale"), &[d])?,
                )),
                false => None,
            };
            layers.push(Layer {
                norm1: Norm::load(w, &format!("{p}.norm1"), d, 1e-5)?,
                norm2: Norm::load(w, &format!("{p}.norm2"), d, 1e-5)?,
                in_proj: Linear::load(w, &format!("{p}.self_attn.in_proj"), (d, 3 * d), false)?,
                out_proj: Linear::load(w, &format!("{p}.self_attn.out_proj"), (d, d), false)?,
                linear1: Linear::load(w, &format!("{p}.linear1"), (d, shape.ff), false)?,
                linear2: Linear::load(w, &format!("{p}.linear2"), (shape.ff, d), false)?,
                scales,
            });
        }
        let half = d / shape.heads / 2;
        let freqs = (0..half)
            .map(|i| 1.0 / shape.max_period.powf(i as f32 / half as f32))
            .collect();
        Ok(Self {
            layers,
            dim: d,
            heads: shape.heads,
            context: shape.context,
            gelu: shape.gelu,
            freqs,
        })
    }

    pub(crate) fn cache(&self) -> Cache {
        Cache {
            keys: vec![Vec::new(); self.layers.len()],
            values: vec![Vec::new(); self.layers.len()],
            len: 0,
        }
    }

    /// Runs `x`, `[n, dim]`, after what `cache` holds, and keeps it there.
    pub(crate) fn forward(&self, x: &Tensor, cache: &mut Cache) -> Result<Tensor> {
        let (n, _) = x.dims2()?;
        let start = cache.len;
        let mut x = x.clone();
        let tables = (cache.keys.iter_mut()).zip(cache.values.iter_mut());
        for (layer, (keys, values)) in self.layers.iter().zip(tables) {
            let h = layer.norm1.forward(&x)?;
            let mut qkv: Vec<f32> = layer.in_proj.forward(&h)?.flatten_all()?.to_vec1()?;
            let mut queries = Vec::with_capacity(n * self.dim);
            for (i, row) in qkv.chunks_exact_mut(3 * self.dim).enumerate() {
                let (q, rest) = row.split_at_mut(self.dim);
                let (k, v) = rest.split_at_mut(self.dim);
                self.rotate(q, start + i);
                self.rotate(k, start + i);
                queries.extend_from_slice(q);
                keys.extend_from_slice(k);
                values.extend_from_slice(v);
            }
            let seen = Seen { keys, values };
            let out = self.attend(&queries, &seen, start);
            let mut a =
                layer
                    .out_proj
                    .forward(&Tensor::from_vec(out, (n, self.dim), x.device())?)?;
            if let Some((s, _)) = &layer.scales {
                a = a.broadcast_mul(s)?;
            }
            x = (x + a)?;
            let h = layer.norm2.forward(&x)?;
            let h = layer.linear1.forward(&h)?;
            let h = match self.gelu {
                Gelu::Erf => h.gelu_erf()?,
                Gelu::Tanh => h.gelu()?,
            };
            let mut f = layer.linear2.forward(&h)?;
            if let Some((_, s)) = &layer.scales {
                f = f.broadcast_mul(s)?;
            }
            x = (x + f)?;
        }
        cache.len += n;
        Ok(x)
    }

    /// Turns each head's pairs by `pos` times their frequency.
    fn rotate(&self, row: &mut [f32], pos: usize) {
        let head = self.dim / self.heads;
        for h in row.chunks_exact_mut(head) {
            for ([a, b], f) in h.as_chunks_mut::<2>().0.iter_mut().zip(&self.freqs) {
                let (sin, cos) = (pos as f32 * f).sin_cos();
                let (x, y) = (*a, *b);
                *a = x * cos - y * sin;
                *b = x * sin + y * cos;
            }
        }
    }

    /// Each query's heads over the keys it may see; rows of `dim`.
    fn attend(&self, queries: &[f32], seen: &Seen<'_>, start: usize) -> Vec<f32> {
        let mut out = vec![0.0; queries.len()];
        let n = queries.len() / self.dim;
        let threads = std::thread::available_parallelism().map_or(1, |t| t.get());
        if n < PARALLEL_ROWS || threads < 2 {
            self.attend_rows(queries, &mut out, seen, start);
            return out;
        }
        let rows = n.div_ceil(threads) * self.dim;
        std::thread::scope(|scope| {
            let parts = queries.chunks(rows).zip(out.chunks_mut(rows));
            for (i, (q, o)) in parts.enumerate() {
                let first = start + i * rows / self.dim;
                scope.spawn(move || self.attend_rows(q, o, seen, first));
            }
        });
        out
    }

    /// [`Self::attend`] for the rows from position `start`.
    fn attend_rows(&self, queries: &[f32], out: &mut [f32], seen: &Seen<'_>, start: usize) {
        let head = self.dim / self.heads;
        let scale = 1.0 / (head as f32).sqrt();
        let mut weights = Vec::new();
        let rows = queries
            .chunks_exact(self.dim)
            .zip(out.chunks_exact_mut(self.dim));
        for (i, (q, o)) in rows.enumerate() {
            let pos = start + i;
            let from = self.context.map_or(0, |c| pos.saturating_sub(c));
            for (h, (qh, oh)) in q
                .chunks_exact(head)
                .zip(o.chunks_exact_mut(head))
                .enumerate()
            {
                let slot = |p: usize| p * self.dim + h * head..p * self.dim + (h + 1) * head;
                weights.clear();
                let mut top = f32::NEG_INFINITY;
                for p in from..=pos {
                    let k = seen.keys.get(slot(p)).unwrap_or_default();
                    let s = dot(qh, k) * scale;
                    top = top.max(s);
                    weights.push(s);
                }
                let mut sum = 0.0;
                for s in weights.iter_mut() {
                    *s = (*s - top).exp();
                    sum += *s;
                }
                for (p, w) in (from..=pos).zip(&weights) {
                    let v = seen.values.get(slot(p)).unwrap_or_default();
                    let w = w / sum;
                    for (x, y) in oh.iter_mut().zip(v) {
                        *x += w * y;
                    }
                }
            }
        }
    }
}

/// The keys and values a layer has, this step's included.
struct Seen<'a> {
    keys: &'a [f32],
    values: &'a [f32],
}

/// A dot product kept in eight lanes, so it vectorises.
pub(crate) fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut lanes = [0.0f32; 8];
    let ((ca, ra), (cb, rb)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    for (x, y) in ca.iter().zip(cb) {
        for ((l, p), q) in lanes.iter_mut().zip(x).zip(y) {
            *l += p * q;
        }
    }
    let tail: f32 = ra.iter().zip(rb).map(|(p, q)| p * q).sum();
    lanes.iter().sum::<f32>() + tail
}
