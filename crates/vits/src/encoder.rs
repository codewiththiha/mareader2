//! The text encoder: phoneme ids to hidden states and the prior's stats.

use candle_core::{Device, Tensor};

use crate::dense::each_row;
use crate::layers::{ChannelNorm, Conv, Weights};
use crate::{Error, Result};

/// Self-attention that also sees each pair's distance, up to a window.
struct Attention {
    q: Conv,
    k: Conv,
    v: Conv,
    o: Conv,
    rel_k: Tensor,
    rel_v: Tensor,
    heads: usize,
    head_dim: usize,
    window: usize,
}

impl Attention {
    fn load(w: &Weights, module: &str) -> Result<Self> {
        let rel_k = w.get(&format!("{module}.emb_rel_k"))?;
        let rel_v = w.get(&format!("{module}.emb_rel_v"))?;
        let (_, span, head_dim) = rel_k.dims3()?;
        let q = Conv::load(w, &format!("{module}.conv_q"))?;
        let heads = q.outputs().checked_div(head_dim).unwrap_or(0);
        if heads == 0 || span % 2 == 0 {
            return Err(Error::Load(format!("{module} has an odd shape")));
        }
        Ok(Self {
            q,
            k: Conv::load(w, &format!("{module}.conv_k"))?,
            v: Conv::load(w, &format!("{module}.conv_v"))?,
            o: Conv::load(w, &format!("{module}.conv_o"))?,
            rel_k,
            rel_v,
            heads,
            head_dim,
            window: span / 2,
        })
    }

    /// `[1, H, T]` to `[heads, T, head_dim]`.
    fn split(&self, x: &Tensor, len: usize) -> Result<Tensor> {
        Ok(x.reshape((self.heads, self.head_dim, len))?
            .transpose(1, 2)?
            .contiguous()?)
    }

    /// The window's embeddings stretched or cut to `2·len − 1` offsets.
    #[cfg(test)]
    fn offsets(&self, table: &Tensor, len: usize) -> Result<Tensor> {
        let pad = len.saturating_sub(self.window + 1);
        let start = (self.window + 1).saturating_sub(len);
        let table = if pad > 0 {
            table.pad_with_zeros(1, pad, pad)?
        } else {
            table.clone()
        };
        Ok(table.narrow(1, start, 2 * len - 1)?)
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let (_, hidden, len) = x.dims3()?;
        let scale = 1.0 / (self.head_dim as f64).sqrt();
        let q = self.split(&self.q.forward(x)?, len)?.affine(scale, 0.0)?;
        let k = self.split(&self.k.forward(x)?, len)?;
        let v = self.split(&self.v.forward(x)?, len)?;
        let band = Band {
            heads: self.heads,
            len,
            dim: self.head_dim,
            window: self.window,
        };
        let rel_k = self.rel_k.flatten_all()?.to_vec1::<f32>()?;
        let rel_v = self.rel_v.flatten_all()?.to_vec1::<f32>()?;
        let mut scores = q.matmul(&k.t()?.contiguous()?)?.flatten_all()?.to_vec1()?;
        band.add_nearby(&mut scores, &q.flatten_all()?.to_vec1()?, &rel_k);
        softmax_rows(&mut scores, len);
        let weights = Tensor::from_vec(scores, (self.heads, len, len), x.device())?;
        let mut out = weights.matmul(&v)?.flatten_all()?.to_vec1()?;
        band.add_by_offset(&mut out, &weights.flatten_all()?.to_vec1()?, &rel_v);
        let out = Tensor::from_vec(out, (self.heads, len, self.head_dim), x.device())?
            .transpose(1, 2)?
            .contiguous()?
            .reshape((1, hidden, len))?;
        self.o.forward(&out)
    }

    /// The same through whole-matrix shifts, kept to check [`Band`].
    #[cfg(test)]
    fn forward_shifted(&self, x: &Tensor) -> Result<Tensor> {
        let (_, hidden, len) = x.dims3()?;
        let scale = 1.0 / (self.head_dim as f64).sqrt();
        let q = self.split(&self.q.forward(x)?, len)?.affine(scale, 0.0)?;
        let k = self.split(&self.k.forward(x)?, len)?;
        let v = self.split(&self.v.forward(x)?, len)?;
        let rel_k = self.offsets(&self.rel_k, len)?.transpose(1, 2)?;
        let near = q.broadcast_matmul(&rel_k.contiguous()?)?;
        let scores = (q.matmul(&k.t()?.contiguous()?)? + relative_to_absolute(&near)?)?;
        let weights = crate::layers::softmax(&scores)?;
        let rel_v = self.offsets(&self.rel_v, len)?.contiguous()?;
        let by_offset = absolute_to_relative(&weights)?.broadcast_matmul(&rel_v)?;
        let out = (weights.matmul(&v)? + by_offset)?;
        let out = out
            .transpose(1, 2)?
            .contiguous()?
            .reshape((1, hidden, len))?;
        self.o.forward(&out)
    }
}

/// Self-attention's nearby pairs: offsets within the window, each head's
/// rows `T` long.
struct Band {
    heads: usize,
    len: usize,
    dim: usize,
    window: usize,
}

impl Band {
    /// Pairs `(i, j, offset row)` for row `i`: `j` within the window of `i`.
    fn near(&self, i: usize) -> impl Iterator<Item = (usize, usize)> {
        let lo = i.saturating_sub(self.window);
        let hi = (i + self.window + 1).min(self.len);
        (lo..hi).map(move |j| (j, j + self.window - i))
    }

    /// `scores[h, i, j] += q[h, i] · rel_k[j − i + window]`.
    fn add_nearby(&self, scores: &mut [f32], q: &[f32], rel_k: &[f32]) {
        let (len, dim) = (self.len, self.dim);
        for hi in 0..self.heads * len {
            let q = q.get(hi * dim..(hi + 1) * dim).unwrap_or_default();
            let row = scores.get_mut(hi * len..(hi + 1) * len).unwrap_or_default();
            for (j, o) in self.near(hi % len) {
                let e = rel_k.get(o * dim..(o + 1) * dim).unwrap_or_default();
                if let Some(s) = row.get_mut(j) {
                    *s += q.iter().zip(e).map(|(a, b)| a * b).sum::<f32>();
                }
            }
        }
    }

    /// `out[h, i] += Σ_j weights[h, i, j] · rel_v[j − i + window]`.
    fn add_by_offset(&self, out: &mut [f32], weights: &[f32], rel_v: &[f32]) {
        let (len, dim) = (self.len, self.dim);
        for hi in 0..self.heads * len {
            let w = weights.get(hi * len..(hi + 1) * len).unwrap_or_default();
            let dst = out.get_mut(hi * dim..(hi + 1) * dim).unwrap_or_default();
            for (j, o) in self.near(hi % len) {
                let e = rel_v.get(o * dim..(o + 1) * dim).unwrap_or_default();
                let wj = w.get(j).copied().unwrap_or(0.0);
                dst.iter_mut().zip(e).for_each(|(d, v)| *d += wj * v);
            }
        }
    }
}

/// Each `width`-long row softmaxed in place, rows shared across cores.
fn softmax_rows(x: &mut [f32], width: usize) {
    each_row(x, width, |_, row| {
        let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        row.iter_mut().for_each(|v| *v = (*v - max).exp());
        let sum: f32 = row.iter().sum();
        row.iter_mut().for_each(|v| *v /= sum);
    });
}

/// `[h, T, 2T−1]` scores by offset to `[h, T, T]` by position.
#[cfg(test)]
pub(crate) fn relative_to_absolute(x: &Tensor) -> Result<Tensor> {
    let (heads, len, _) = x.dims3()?;
    let x = x.pad_with_zeros(2, 0, 1)?.reshape((heads, len * 2 * len))?;
    let x = x.pad_with_zeros(1, 0, len - 1)?;
    let x = x.reshape((heads, len + 1, 2 * len - 1))?;
    Ok(x.narrow(1, 0, len)?.narrow(2, len - 1, len)?)
}

/// `[h, T, T]` weights by position to `[h, T, 2T−1]` by offset.
#[cfg(test)]
pub(crate) fn absolute_to_relative(x: &Tensor) -> Result<Tensor> {
    let (heads, len, _) = x.dims3()?;
    let x = x.pad_with_zeros(2, 0, len - 1)?;
    let x = x.reshape((heads, len * (2 * len - 1)))?;
    let x = x
        .pad_with_zeros(1, len, 0)?
        .reshape((heads, len, 2 * len))?;
    Ok(x.narrow(2, 1, 2 * len - 1)?)
}

/// Two convolutions around a ReLU, padded so the length holds.
struct FeedForward {
    first: Conv,
    second: Conv,
    pads: (usize, usize),
}

impl FeedForward {
    fn load(w: &Weights, module: &str) -> Result<Self> {
        let first = Conv::load(w, &format!("{module}.conv_1"))?;
        let kernel = first.kernel();
        Ok(Self {
            pads: (kernel.saturating_sub(1) / 2, kernel / 2),
            first,
            second: Conv::load(w, &format!("{module}.conv_2"))?,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let (l, r) = self.pads;
        let x = self.first.forward(&x.pad_with_zeros(2, l, r)?)?.relu()?;
        self.second.forward(&x.pad_with_zeros(2, l, r)?)
    }
}

struct Layer {
    attention: Attention,
    norm_1: ChannelNorm,
    feed: FeedForward,
    norm_2: ChannelNorm,
}

/// What the encoder makes of one utterance, each `[1, C, T]`.
pub(crate) struct Encoded {
    pub(crate) hidden: Tensor,
    pub(crate) mean: Tensor,
    pub(crate) log_spread: Tensor,
}

pub(crate) struct TextEncoder {
    embedding: Tensor,
    layers: Vec<Layer>,
    proj: Conv,
}

impl TextEncoder {
    pub(crate) fn load(w: &Weights) -> Result<Self> {
        let embedding = w.get("enc_p.emb.weight")?;
        let count = w.count("enc_p.encoder.attn_layers");
        if count == 0 {
            return Err(Error::Load("the text encoder has no layers".into()));
        }
        let layers = (0..count)
            .map(|i| {
                Ok(Layer {
                    attention: Attention::load(w, &format!("enc_p.encoder.attn_layers.{i}"))?,
                    norm_1: ChannelNorm::load(w, &format!("enc_p.encoder.norm_layers_1.{i}"))?,
                    feed: FeedForward::load(w, &format!("enc_p.encoder.ffn_layers.{i}"))?,
                    norm_2: ChannelNorm::load(w, &format!("enc_p.encoder.norm_layers_2.{i}"))?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            embedding,
            layers,
            proj: Conv::load(w, "enc_p.proj")?,
        })
    }

    /// Phoneme ids the embedding has rows for.
    pub(crate) fn vocabulary(&self) -> usize {
        self.embedding.dims().first().copied().unwrap_or(0)
    }

    pub(crate) fn forward(&self, ids: &[u32]) -> Result<Encoded> {
        if let Some(&bad) = ids.iter().find(|&&id| id as usize >= self.vocabulary()) {
            return Err(Error::Input(format!(
                "phoneme id {bad} is past the model's table"
            )));
        }
        let (_, hidden) = self.embedding.dims2()?;
        let ids = Tensor::new(ids, &Device::Cpu)?;
        let x = self
            .embedding
            .index_select(&ids, 0)?
            .affine((hidden as f64).sqrt(), 0.0)?;
        let mut x = x.t()?.unsqueeze(0)?.contiguous()?;
        for layer in &self.layers {
            x = layer
                .norm_1
                .forward(&(&x + layer.attention.forward(&x)?)?)?;
            x = layer.norm_2.forward(&(&x + layer.feed.forward(&x)?)?)?;
        }
        let stats = self.proj.forward(&x)?;
        let half = stats.dim(1)? / 2;
        Ok(Encoded {
            mean: stats.narrow(1, 0, half)?,
            log_spread: stats.narrow(1, half, half)?,
            hidden: x,
        })
    }
}

#[cfg(test)]
mod tests {
    use candle_core::{Device, Tensor};

    use super::*;

    fn wave(n: usize, seed: f32) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.37 + seed).sin()).collect()
    }

    fn conv(channels: usize, seed: f32) -> Conv {
        let w = Tensor::from_vec(
            wave(channels * channels, seed),
            (channels, channels, 1),
            &Device::Cpu,
        );
        let b = Tensor::from_vec(wave(channels, seed + 1.0), channels, &Device::Cpu);
        Conv::from_parts(w.unwrap(), Some(b.unwrap()))
    }

    #[test]
    fn band_matches_whole_matrix_shifts() {
        let (heads, dim, window) = (2, 3, 2);
        let span = 2 * window + 1;
        let table = |seed| Tensor::from_vec(wave(span * dim, seed), (1, span, dim), &Device::Cpu);
        let a = Attention {
            q: conv(heads * dim, 0.1),
            k: conv(heads * dim, 0.2),
            v: conv(heads * dim, 0.3),
            o: conv(heads * dim, 0.4),
            rel_k: table(0.5).unwrap(),
            rel_v: table(0.6).unwrap(),
            heads,
            head_dim: dim,
            window,
        };
        for len in [1, 2, 3, 9] {
            let x = Tensor::from_vec(
                wave(heads * dim * len, 0.7),
                (1, heads * dim, len),
                &Device::Cpu,
            );
            let x = x.unwrap();
            let got = a
                .forward(&x)
                .unwrap()
                .flatten_all()
                .unwrap()
                .to_vec1::<f32>()
                .unwrap();
            let want = a.forward_shifted(&x).unwrap().flatten_all().unwrap();
            for (g, w) in got.iter().zip(want.to_vec1::<f32>().unwrap()) {
                assert!((g - w).abs() < 1e-4, "len {len}: {g} against {w}");
            }
        }
    }
}
