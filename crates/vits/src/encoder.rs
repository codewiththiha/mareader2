//! The text encoder: phoneme ids to hidden states and the prior's stats.

use candle_core::{Device, Tensor};

use crate::layers::{ChannelNorm, Conv, Weights, softmax};
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
        let rel_k = self.offsets(&self.rel_k, len)?.transpose(1, 2)?;
        let near = q.broadcast_matmul(&rel_k.contiguous()?)?;
        let scores = (q.matmul(&k.t()?.contiguous()?)? + relative_to_absolute(&near)?)?;
        let weights = softmax(&scores)?;
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

/// `[h, T, 2T−1]` scores by offset to `[h, T, T]` by position.
pub(crate) fn relative_to_absolute(x: &Tensor) -> Result<Tensor> {
    let (heads, len, _) = x.dims3()?;
    let x = x.pad_with_zeros(2, 0, 1)?.reshape((heads, len * 2 * len))?;
    let x = x.pad_with_zeros(1, 0, len - 1)?;
    let x = x.reshape((heads, len + 1, 2 * len - 1))?;
    Ok(x.narrow(1, 0, len)?.narrow(2, len - 1, len)?)
}

/// `[h, T, T]` weights by position to `[h, T, 2T−1]` by offset.
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
