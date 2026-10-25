"""A plain numpy Phonon, to check the port stage by stage."""

import json
import math
import struct

import numpy as np


def read_safetensors(path):
    with open(path, "rb") as f:
        n = struct.unpack("<Q", f.read(8))[0]
        header = json.loads(f.read(n))
        data = f.read()
    out = {}
    for name, info in header.items():
        if name == "__metadata__":
            continue
        a, b = info["data_offsets"]
        raw = data[a:b]
        if info["dtype"] == "BF16":
            u = np.frombuffer(raw, dtype=np.uint16).astype(np.uint32) << 16
            t = u.view(np.float32)
        elif info["dtype"] == "F32":
            t = np.frombuffer(raw, dtype=np.float32)
        else:
            raise ValueError(info["dtype"])
        out[name] = t.reshape(info["shape"]).astype(np.float32)
    return out


def layer_norm(x, w=None, b=None, eps=1e-5):
    m = x.mean(-1, keepdims=True)
    v = ((x - m) ** 2).mean(-1, keepdims=True)
    y = (x - m) / np.sqrt(v + eps)
    if w is not None:
        y = y * w + b
    return y


def gelu_erf(x):
    erf = np.vectorize(math.erf)
    return 0.5 * x * (1.0 + erf(x / math.sqrt(2.0)).astype(np.float32))


def silu(x):
    return x / (1.0 + np.exp(-x))


def elu(x):
    return np.where(x > 0, x, np.exp(np.minimum(x, 0)) - 1.0)


class Stack:
    def __init__(self, w, prefix, layers, heads, max_period, context=None, scaled=False):
        self.w, self.p, self.n, self.h = w, prefix, layers, heads
        self.context, self.scaled, self.max_period = context, scaled, max_period
        self.k = [None] * layers
        self.v = [None] * layers
        self.len = 0

    def rope(self, x, start):
        # x: [n, heads, d]
        n, h, d = x.shape
        half = d // 2
        freqs = 1.0 / (self.max_period ** (np.arange(half, dtype=np.float32) / half))
        pos = np.arange(start, start + n, dtype=np.float32)[:, None] * freqs[None]
        cos, sin = np.cos(pos)[:, None, :], np.sin(pos)[:, None, :]
        a, b = x[..., 0::2], x[..., 1::2]
        out = np.empty_like(x)
        out[..., 0::2] = a * cos - b * sin
        out[..., 1::2] = a * sin + b * cos
        return out

    def forward(self, x):
        n, dim = x.shape
        hd = dim // self.h
        start = self.len
        for i in range(self.n):
            p = f"{self.p}.layers.{i}"
            w = self.w
            h = layer_norm(x, w[f"{p}.norm1.weight"], w[f"{p}.norm1.bias"])
            qkv = h @ w[f"{p}.self_attn.in_proj.weight"].T
            q, k, v = (qkv[:, j * dim:(j + 1) * dim].reshape(n, self.h, hd) for j in range(3))
            q, k = self.rope(q, start), self.rope(k, start)
            self.k[i] = k if self.k[i] is None else np.concatenate([self.k[i], k])
            self.v[i] = v if self.v[i] is None else np.concatenate([self.v[i], v])
            K, V = self.k[i], self.v[i]
            total = K.shape[0]
            s = np.einsum("nhd,thd->hnt", q, K) / math.sqrt(hd)
            qpos = np.arange(start, start + n)[:, None]
            kpos = np.arange(total)[None, :]
            ok = kpos <= qpos
            if self.context is not None:
                ok &= kpos >= qpos - self.context
            s = np.where(ok[None], s, -np.inf)
            s = np.exp(s - s.max(-1, keepdims=True))
            s /= s.sum(-1, keepdims=True)
            o = np.einsum("hnt,thd->nhd", s, V).reshape(n, dim)
            o = o @ w[f"{p}.self_attn.out_proj.weight"].T
            if self.scaled:
                o = o * w[f"{p}.layer_scale_1.scale"]
            x = x + o
            h = layer_norm(x, w[f"{p}.norm2.weight"], w[f"{p}.norm2.bias"])
            f = gelu_erf(h @ w[f"{p}.linear1.weight"].T) @ w[f"{p}.linear2.weight"].T
            if self.scaled:
                f = f * w[f"{p}.layer_scale_2.scale"]
            x = x + f
        self.len += n
        return x


C = "flow_lm.condition_provider.conditioners"


class Model:
    def __init__(self, weights, config):
        self.w = read_safetensors(weights)
        self.cfg = json.load(open(config))

    def lin(self, x, name, bias=True):
        y = x @ self.w[f"{name}.weight"].T
        return y + self.w[f"{name}.bias"] if bias else y

    def conditions(self):
        w = self.w
        row = w[f"{C}.num_speakers.embed.weight"][1]
        c = row @ w[f"{C}.num_speakers.output_proj.weight"].T
        feats = np.concatenate([np.ones(64, np.float32), np.zeros(64, np.float32)])
        return c + feats @ w[f"{C}.duration_delta.output_proj.weight"].T

    def time(self, p, t):
        w = self.w
        args = t * w[f"{p}.freqs"]
        e = np.concatenate([np.cos(args), np.sin(args)])[None]
        x = self.lin(silu(self.lin(e, f"{p}.mlp.0")), f"{p}.mlp.2")
        ms = (x ** 2).mean(-1, keepdims=True)
        return x / np.sqrt(ms + 1e-5) * w[f"{p}.mlp.3.alpha"]

    def head(self, state, noise):
        p = "flow_lm.flow_net"
        t = (self.time(f"{p}.time_embed.0", 0.0) + self.time(f"{p}.time_embed.1", 1.0)) / 2
        y = self.lin(state, f"{p}.cond_embed") + t
        ya = silu(y)
        x = self.lin(noise, f"{p}.input_proj")
        for i in range(6):
            b = f"{p}.res_blocks.{i}"
            ada = self.lin(ya, f"{b}.adaLN_modulation.1")
            sh, sc, g = ada[:, :512], ada[:, 512:1024], ada[:, 1024:]
            h = layer_norm(x, self.w[f"{b}.in_ln.weight"], self.w[f"{b}.in_ln.bias"], 1e-6)
            h = h * (1 + sc) + sh
            h = self.lin(silu(self.lin(h, f"{b}.mlp.0")), f"{b}.mlp.2")
            x = x + g * h
        ada = self.lin(ya, f"{p}.final_layer.adaLN_modulation.1")
        sh, sc = ada[:, :512], ada[:, 512:]
        h = layer_norm(x, eps=1e-6) * (1 + sc) + sh
        v = self.lin(h, f"{p}.final_layer.linear")
        return noise + v

    def run(self, voice_latents, tokens, budget, after):
        w = self.w
        stack = Stack(w, "flow_lm.transformer", 6, 8, 10000.0)
        emb = voice_latents @ w[f"{C}.speaker_wavs.output_proj.weight"].T
        stack.forward(emb)
        stack.forward(w[f"{C}.transcript_in_segment.embed.weight"][tokens])
        cond = self.conditions()
        latent = w["flow_lm.bos_emb"][None]
        out, left, ended = [], None, None
        for frame in range(budget):
            x = latent @ w["flow_lm.input_linear.weight"].T + cond
            s = stack.forward(x)
            s = layer_norm(s, w["flow_lm.out_norm.weight"], w["flow_lm.out_norm.bias"])
            eos = float(self.lin(s, "flow_lm.out_eos")[0, 0])
            nxt = self.head(s, np.zeros((1, 32), np.float32))
            out.append(nxt[0])
            if eos > 4.0 and left is None:
                left, ended = after, frame
            if left is not None:
                if left == 0:
                    break
                left -= 1
            latent = nxt
        return np.array(out), ended

    def decode(self, latents):
        w = self.w
        x = latents * w["flow_lm.emb_std"] + w["flow_lm.emb_mean"]
        x = x @ w["mimi.quantizer.output_proj.weight"][:, :, 0].T  # [T, 512]
        T = x.shape[0]
        taps = w["mimi.upsample.convtr.convtr.weight"][:, 0, :]  # [512, 32]
        up = np.zeros((T * 16 + 16, 512), np.float32)
        for t in range(T):
            up[t * 16:t * 16 + 32] += x[t][None, :] * taps.T
        up = up[:T * 16]
        st = Stack(w, "mimi.decoder_transformer.transformer", 2, 8, 10000.0, 250, True)
        z = st.forward(up).T[None]  # [1, 512, n]

        def conv(z, name, dil=1):
            k = w[f"{name}.conv.weight"]
            b = w[f"{name}.conv.bias"]
            kk = k.shape[2]
            pad = (kk - 1) * dil
            zp = np.pad(z, ((0, 0), (0, 0), (pad, 0)))
            n = z.shape[2]
            out = np.zeros((1, k.shape[0], n), np.float32)
            for j in range(kk):
                out[0] += k[:, :, j] @ zp[0, :, j * dil:j * dil + n]
            return out + b[None, :, None]

        def convtr(z, name, s):
            k = w[f"{name}.convtr.weight"]  # [in, out, 2s]
            b = w[f"{name}.convtr.bias"]
            n = z.shape[2]
            out = np.zeros((1, k.shape[1], n * s + s), np.float32)
            for j in range(k.shape[2]):
                out[0, :, j:j + n * s:s] += k[:, :, j].T @ z[0]
            return out[:, :, :n * s] + b[None, :, None]

        m = "mimi.decoder.model"
        z = conv(z, f"{m}.0")
        for (up_i, res_i, s) in ((2, 3, 6), (5, 6, 5), (8, 9, 4)):
            z = convtr(elu(z), f"{m}.{up_i}", s)
            y = conv(elu(z), f"{m}.{res_i}.block.1")
            y = conv(elu(y), f"{m}.{res_i}.block.3")
            z = z + y
        z = conv(elu(z), f"{m}.11")
        return z[0, 0]
