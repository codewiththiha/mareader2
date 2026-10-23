//! The phoneme encoder: an ALBERT, one layer's weights used at every depth.

use candle_core::{D, DType, Module, Result, Tensor};
use candle_nn::{Embedding, Linear, VarBuilder};

use crate::config::Plbert;
use crate::layers::normalised;

/// ALBERT's own epsilon, as the original model sets it.
const EPS: f64 = 1e-12;

/// Layer norm with a two-pass spread, never negative at so small an epsilon.
struct LayerNorm {
    weight: Tensor,
    bias: Tensor,
}

impl LayerNorm {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        normalised(x, D::Minus1, EPS)?
            .broadcast_mul(&self.weight)?
            .broadcast_add(&self.bias)
    }
}

fn norm(size: usize, vb: VarBuilder) -> Result<LayerNorm> {
    Ok(LayerNorm {
        weight: vb.get(size, "weight")?,
        bias: vb.get(size, "bias")?,
    })
}

pub(crate) struct Albert {
    words: Embedding,
    positions: Embedding,
    types: Embedding,
    embed_norm: LayerNorm,
    project: Option<Linear>,
    layer: Layer,
    depth: usize,
}

struct Layer {
    attention: Attention,
    ffn: Linear,
    ffn_output: Linear,
    attention_norm: LayerNorm,
    output_norm: LayerNorm,
}

struct Attention {
    query: Linear,
    key: Linear,
    value: Linear,
    dense: Linear,
    heads: usize,
    head_dim: usize,
}

impl Albert {
    pub(crate) fn load(c: &Plbert, vb: &VarBuilder) -> Result<Self> {
        let e = vb.pp("embeddings");
        let size = c.embedding_size;
        let words = candle_nn::embedding(c.vocab_size, size, e.pp("word_embeddings"))?;
        let positions =
            candle_nn::embedding(c.max_position_embeddings, size, e.pp("position_embeddings"))?;
        let types = candle_nn::embedding(c.type_vocab_size, size, e.pp("token_type_embeddings"))?;
        let embed_norm = norm(size, e.pp("LayerNorm"))?;
        let enc = vb.pp("encoder");
        let project = (size != c.hidden_size)
            .then(|| candle_nn::linear(size, c.hidden_size, enc.pp("embedding_hidden_mapping_in")))
            .transpose()?;
        let layer_vb = enc.pp("albert_layer_groups.0.albert_layers.0");
        Ok(Self {
            words,
            positions,
            types,
            embed_norm,
            project,
            layer: Layer::load(c, &layer_vb)?,
            depth: c.num_hidden_layers,
        })
    }

    /// Token ids `[1, n]` to features `[1, n, hidden]`.
    pub(crate) fn forward(&self, ids: &Tensor) -> Result<Tensor> {
        let (_, n) = ids.dims2()?;
        let most = self.positions.embeddings().dim(0)?;
        if n == 0 || n > most {
            candle_core::bail!("{n} phonemes is outside 1..={most}");
        }
        let count = u32::try_from(n).map_err(candle_core::Error::wrap)?;
        let positions = Tensor::arange(0u32, count, ids.device())?.unsqueeze(0)?;
        let types = Tensor::zeros(ids.shape(), DType::U32, ids.device())?;
        let x = (self.words.forward(ids)?
            + self.positions.forward(&positions)?
            + self.types.forward(&types)?)?;
        let x = self.embed_norm.forward(&x)?;
        let mut x = match &self.project {
            Some(p) => p.forward(&x)?,
            None => x,
        };
        for _ in 0..self.depth {
            x = self.layer.forward(&x)?;
        }
        Ok(x)
    }
}

impl Layer {
    fn load(c: &Plbert, vb: &VarBuilder) -> Result<Self> {
        let h = c.hidden_size;
        Ok(Self {
            attention: Attention::load(c, &vb.pp("attention"))?,
            ffn: candle_nn::linear(h, c.intermediate_size, vb.pp("ffn"))?,
            ffn_output: candle_nn::linear(c.intermediate_size, h, vb.pp("ffn_output"))?,
            attention_norm: norm(h, vb.pp("attention.LayerNorm"))?,
            output_norm: norm(h, vb.pp("full_layer_layer_norm"))?,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let x = self
            .attention_norm
            .forward(&(x + self.attention.forward(x)?)?)?;
        let ffn = self.ffn.forward(&x)?.gelu_erf()?;
        let ffn = self.ffn_output.forward(&ffn)?;
        self.output_norm.forward(&(x + ffn)?)
    }
}

impl Attention {
    fn load(c: &Plbert, vb: &VarBuilder) -> Result<Self> {
        let h = c.hidden_size;
        let heads = c.num_attention_heads.max(1);
        Ok(Self {
            query: candle_nn::linear(h, h, vb.pp("query"))?,
            key: candle_nn::linear(h, h, vb.pp("key"))?,
            value: candle_nn::linear(h, h, vb.pp("value"))?,
            dense: candle_nn::linear(h, h, vb.pp("dense"))?,
            heads,
            head_dim: h / heads,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let (batch, n, _) = x.dims3()?;
        let split = |t: Tensor| -> Result<Tensor> {
            t.reshape((batch, n, self.heads, self.head_dim))?
                .transpose(1, 2)?
                .contiguous()
        };
        let q = split(self.query.forward(x)?)?;
        let k = split(self.key.forward(x)?)?;
        let v = split(self.value.forward(x)?)?;
        let scale = 1.0 / (self.head_dim as f64).sqrt();
        let weights = (q.matmul(&k.transpose(2, 3)?.contiguous()?)? * scale)?;
        let weights = candle_nn::ops::softmax_last_dim(&weights)?;
        let out =
            weights
                .matmul(&v)?
                .transpose(1, 2)?
                .reshape((batch, n, self.heads * self.head_dim))?;
        self.dense.forward(&out)
    }
}
