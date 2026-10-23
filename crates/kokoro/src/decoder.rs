//! Frames to sound: a harmonic source shaped by an inverse-STFT generator.

use candle_core::{DType, Device, Result, Tensor};
use candle_nn::VarBuilder;

use crate::SAMPLE_RATE;
use crate::config::Istft;
use crate::layers::{AdaIn1d, Conv, Conv1d, ConvTranspose1d, leaky_relu, scale, upsample};
use crate::noise::Noise;
use crate::prosody::AdainResBlk1d;

/// Named steps kept from a pass, for checking; off unless asked.
#[derive(Default)]
pub(crate) struct Probes(pub(crate) Option<Vec<(String, Tensor)>>);

impl Probes {
    fn keep(&mut self, name: impl FnOnce() -> String, t: &Tensor) {
        if let Some(all) = &mut self.0 {
            all.push((name(), t.clone()));
        }
    }
}

/// Harmonics above the fundamental in the source.
const HARMONICS: usize = 8;
const SINE_AMP: f32 = 0.1;
const NOISE_STD: f32 = 0.003;
/// Pitch below this, in hertz, counts as unvoiced.
const VOICED: f32 = 10.0;

/// `input` stretched to `len` points, as linear interpolation without corners.
fn resample(input: &[f32], len: usize) -> Vec<f32> {
    let (Some(&first), Some(last)) = (input.first(), input.len().checked_sub(1)) else {
        return Vec::new();
    };
    if input.len() == len {
        return input.to_vec();
    }
    if last == 0 {
        return vec![first; len];
    }
    let ratio = input.len() as f32 / len as f32;
    (0..len)
        .map(|i| {
            let src = ((i as f32 + 0.5) * ratio - 0.5).clamp(0.0, last as f32);
            let left = src.floor() as usize;
            let frac = src - left as f32;
            let a = input.get(left).copied().unwrap_or(first);
            let b = input.get((left + 1).min(last)).copied().unwrap_or(a);
            a * (1.0 - frac) + b * frac
        })
        .collect()
}

/// A fixed phase offset per harmonic, as the model's source expects.
fn offset(seed: u64) -> f32 {
    let mut v = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    v = (v ^ (v >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let v = v ^ (v >> 31);
    ((v >> 40) as u32 as f32) / ((1u32 << 24) as f32)
}

/// Sines at the pitch and its harmonics, noise where unvoiced, merged to one.
struct Source {
    upsampling: usize,
    weight_t: Tensor,
    bias: Option<Tensor>,
}

impl Source {
    fn load(upsampling: usize, vb: &VarBuilder) -> Result<Self> {
        Ok(Self {
            upsampling: upsampling.max(1),
            weight_t: vb.get((1, HARMONICS + 1), "l_linear.weight")?.t()?,
            bias: vb.get(1, "l_linear.bias").ok(),
        })
    }

    /// Pitch per sample `[1, len, 1]` to the source `[1, len, 1]`.
    fn forward(&self, f0: &Tensor, noise: &mut Noise) -> Result<Tensor> {
        let (_, len, _) = f0.dims3()?;
        let f0: Vec<f32> = f0.flatten_all()?.to_dtype(DType::F32)?.to_vec1()?;
        let dims = HARMONICS + 1;
        let coarse = (len / self.upsampling).max(1);
        let mut sines = vec![0f32; len * dims];
        for h in 0..dims {
            let times = (h + 1) as f32;
            let mut rad: Vec<f32> = f0
                .iter()
                .map(|v| (v * times / SAMPLE_RATE as f32).rem_euclid(1.0))
                .collect();
            if let (true, Some(first)) = (h > 0, rad.first_mut()) {
                *first += offset(h as u64);
            }
            let mut acc = 0f32;
            let phase: Vec<f32> = resample(&rad, coarse)
                .into_iter()
                .map(|v| {
                    acc += v;
                    acc * std::f32::consts::TAU * self.upsampling as f32
                })
                .collect();
            let phase = resample(&phase, len);
            for (slot, p) in sines.iter_mut().skip(h).step_by(dims).zip(phase) {
                *slot = p.sin() * SINE_AMP;
            }
        }
        // Voiced samples keep sines under faint noise; the rest is noise.
        for (row, v) in sines.chunks_exact_mut(dims).zip(&f0) {
            let voiced = *v > VOICED;
            let amp = if voiced { NOISE_STD } else { SINE_AMP / 3.0 };
            for slot in row {
                let sine = if voiced { *slot } else { 0.0 };
                *slot = sine + noise.normal() * amp;
            }
        }
        let sines = Tensor::from_vec(sines, (1, len, dims), &Device::Cpu)?;
        let merged = sines.broadcast_matmul(&self.weight_t)?;
        let merged = match &self.bias {
            Some(b) => merged.broadcast_add(b)?,
            None => merged,
        };
        merged.tanh()
    }
}

/// A short-time Fourier transform with a Hann window, and its inverse.
struct Stft {
    n_fft: usize,
    hop: usize,
    window: Vec<f32>,
    /// `cos` and `sin` of `2π k n / N`, row `k` after row.
    cos: Vec<f32>,
    sin: Vec<f32>,
}

impl Stft {
    fn new(n_fft: usize, hop: usize) -> Result<Self> {
        if n_fft < 2 || !n_fft.is_multiple_of(2) || hop == 0 {
            candle_core::bail!("an STFT needs an even window of 2 or more, and a hop");
        }
        let n = n_fft as f32;
        let window = (0..n_fft)
            .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / n).cos()))
            .collect();
        // Rounded as the reference rounds it: near-empty bins keep its phase.
        let angles: Vec<f32> = (0..n_fft * n_fft)
            .map(|i| 2.0 * std::f32::consts::PI * (i / n_fft) as f32 * (i % n_fft) as f32 / n)
            .collect();
        Ok(Self {
            n_fft,
            hop,
            window,
            cos: angles.iter().map(|a| a.cos()).collect(),
            sin: angles.iter().map(|a| a.sin()).collect(),
        })
    }

    fn bins(&self) -> usize {
        self.n_fft / 2 + 1
    }

    /// `samples` padded by reflection at both ends.
    fn reflected(&self, samples: &[f32]) -> Result<Vec<f32>> {
        let pad = self.n_fft / 2;
        if samples.len() <= pad {
            candle_core::bail!("{} samples is too short for the STFT", samples.len());
        }
        let len = samples.len();
        let head = samples.iter().skip(1).take(pad).rev();
        let tail = samples.iter().rev().skip(1).take(pad);
        let mut out = Vec::with_capacity(len + 2 * pad);
        out.extend(head);
        out.extend_from_slice(samples);
        out.extend(tail);
        Ok(out)
    }

    /// Mono `[1, len]` to magnitude and phase, each `[1, bins, frames]`.
    fn transform(&self, wave: &Tensor) -> Result<(Tensor, Tensor)> {
        let samples: Vec<f32> = wave.flatten_all()?.to_dtype(DType::F32)?.to_vec1()?;
        let padded = self.reflected(&samples)?;
        let frames = 1 + samples.len() / self.hop;
        let bins = self.bins();
        let mut magnitude = vec![0f32; bins * frames];
        let mut phase = vec![0f32; bins * frames];
        let mut frame = vec![0f32; self.n_fft];
        for f in 0..frames {
            let piece = padded.iter().skip(f * self.hop);
            for ((slot, s), w) in frame.iter_mut().zip(piece).zip(&self.window) {
                *slot = s * w;
            }
            let rows = self
                .cos
                .chunks_exact(self.n_fft)
                .zip(self.sin.chunks_exact(self.n_fft));
            for (k, (cos, sin)) in rows.take(bins).enumerate() {
                // Summed from +0: a negative DC frame keeps phase +π, not -π.
                let re = frame.iter().zip(cos).fold(0f32, |a, (s, c)| a + s * c);
                let im = frame.iter().zip(sin).fold(0f32, |a, (s, c)| a - s * c);
                if let Some(m) = magnitude.get_mut(k * frames + f) {
                    *m = (re * re + im * im).sqrt();
                }
                if let Some(p) = phase.get_mut(k * frames + f) {
                    *p = im.atan2(re);
                }
            }
        }
        let shape = (1, bins, frames);
        Ok((
            Tensor::from_vec(magnitude, shape, &Device::Cpu)?,
            Tensor::from_vec(phase, shape, &Device::Cpu)?,
        ))
    }

    /// Magnitude and phase `[1, bins, frames]` back to samples.
    fn inverse(&self, magnitude: &Tensor, phase: &Tensor) -> Result<Vec<f32>> {
        let (_, bins, frames) = magnitude.dims3()?;
        if phase.dims() != magnitude.dims() || bins != self.bins() || frames == 0 {
            candle_core::bail!("magnitude and phase don't fit the inverse STFT");
        }
        let mags: Vec<f32> = magnitude.flatten_all()?.to_dtype(DType::F32)?.to_vec1()?;
        let phases: Vec<f32> = phase.flatten_all()?.to_dtype(DType::F32)?.to_vec1()?;
        let n = self.n_fft;
        let half = n / 2;
        let len = n + self.hop * (frames - 1);
        let mut out = vec![0f32; len];
        let mut envelope = vec![0f32; len];
        let mut re = vec![0f32; n];
        let mut im = vec![0f32; n];
        for f in 0..frames {
            // The full spectrum: each bin, and its mirror for a real signal.
            for k in 0..bins {
                let i = k * frames + f;
                let (m, p) = (
                    mags.get(i).copied().unwrap_or(0.0),
                    phases.get(i).copied().unwrap_or(0.0),
                );
                let (r, j) = (m * p.cos(), m * p.sin());
                if let (Some(a), Some(b)) = (re.get_mut(k), im.get_mut(k)) {
                    (*a, *b) = (r, j);
                }
                if k > 0
                    && k < half
                    && let (Some(a), Some(b)) = (re.get_mut(n - k), im.get_mut(n - k))
                {
                    (*a, *b) = (r, -j);
                }
            }
            let at = f * self.hop;
            let slots = out.iter_mut().skip(at).zip(envelope.iter_mut().skip(at));
            for (t, ((slot, env), w)) in slots.zip(&self.window).enumerate() {
                let cos = self.cos.iter().skip(t).step_by(n);
                let sin = self.sin.iter().skip(t).step_by(n);
                let parts = re.iter().zip(&im).zip(cos.zip(sin));
                let value: f32 = parts.map(|((r, j), (c, s))| r * c - j * s).sum();
                *slot += value / n as f32 * w;
                *env += w * w;
            }
        }
        for (v, e) in out.iter_mut().zip(&envelope) {
            if *e > 1e-8 {
                *v /= e;
            }
        }
        match len > 2 * half {
            true => Ok(out.into_iter().skip(half).take(len - 2 * half).collect()),
            false => Ok(out),
        }
    }
}

/// Snake: `x + sin²(αx) / α`, a periodic-friendly activation.
fn snake(x: &Tensor, alpha: &Tensor) -> Result<Tensor> {
    let wave = x.broadcast_mul(alpha)?.sin()?.sqr()?;
    x + wave.broadcast_mul(&alpha.recip()?)?
}

/// Dilated convolutions with snake activations and style-led norms.
struct SnakeBlock {
    layers: Vec<SnakeLayer>,
}

struct SnakeLayer {
    norm1: AdaIn1d,
    alpha1: Tensor,
    conv1: Conv1d,
    norm2: AdaIn1d,
    alpha2: Tensor,
    conv2: Conv1d,
}

impl SnakeBlock {
    fn load(
        ch: usize,
        kernel: usize,
        dilations: &[usize],
        style: usize,
        vb: &VarBuilder,
    ) -> Result<Self> {
        let alpha = |name: String| -> Result<Tensor> {
            vb.get((1, ch, 1), &name)
                .or_else(|_| Tensor::ones((1, ch, 1), DType::F32, &Device::Cpu))
        };
        let layers = dilations
            .iter()
            .enumerate()
            .map(|(i, &d)| {
                Ok(SnakeLayer {
                    norm1: AdaIn1d::load(style, ch, &vb.pp("adain1").pp(i))?,
                    alpha1: alpha(format!("alpha1.{i}"))?,
                    conv1: Conv::new(ch, ch, kernel)
                        .padding((kernel * d - d) / 2)
                        .dilation(d)
                        .load(&vb.pp("convs1").pp(i))?,
                    norm2: AdaIn1d::load(style, ch, &vb.pp("adain2").pp(i))?,
                    alpha2: alpha(format!("alpha2.{i}"))?,
                    conv2: Conv::new(ch, ch, kernel)
                        .padding((kernel - 1) / 2)
                        .load(&vb.pp("convs2").pp(i))?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self { layers })
    }

    fn forward(&self, x: &Tensor, s: &Tensor) -> Result<Tensor> {
        let mut x = x.clone();
        for l in &self.layers {
            let y = l
                .conv1
                .forward(&snake(&l.norm1.forward(&x, s)?, &l.alpha1)?)?;
            let y = l
                .conv2
                .forward(&snake(&l.norm2.forward(&y, s)?, &l.alpha2)?)?;
            x = (y + x)?;
        }
        Ok(x)
    }
}

/// One upsampling stage: the transposed conv, its source path, its blocks.
struct Stage {
    up: ConvTranspose1d,
    noise_conv: Conv1d,
    noise_res: SnakeBlock,
    blocks: Vec<SnakeBlock>,
}

struct Generator {
    source: Source,
    stft: Stft,
    stages: Vec<Stage>,
    post: Conv1d,
    /// Samples per pitch frame.
    f0_scale: usize,
}

impl Generator {
    fn load(c: &Istft, style: usize, vb: &VarBuilder) -> Result<Self> {
        let f0_scale = c.upsample_rates.iter().product::<usize>() * c.gen_istft_hop_size;
        let n_fft = c.gen_istft_n_fft;
        let count = c.upsample_rates.len();
        let first = c.upsample_initial_channel;
        let rates = c.upsample_rates.iter().zip(&c.upsample_kernel_sizes);
        let stages = rates
            .enumerate()
            .map(|(i, (&u, &k))| {
                let ch = first >> (i + 1);
                let up = Conv::new(first >> i, ch, k)
                    .stride(u)
                    .padding(k.saturating_sub(u) / 2)
                    .load_transposed(&vb.pp("ups").pp(i))?;
                let later: usize = c.upsample_rates.iter().skip(i + 1).product();
                let (noise_conv, kernel) = match i + 1 < count {
                    true => (
                        Conv::new(n_fft + 2, ch, later * 2)
                            .stride(later)
                            .padding(later.div_ceil(2)),
                        7,
                    ),
                    false => (Conv::new(n_fft + 2, ch, 1), 11),
                };
                let kinds = c
                    .resblock_kernel_sizes
                    .iter()
                    .zip(&c.resblock_dilation_sizes);
                let blocks = kinds
                    .enumerate()
                    .map(|(j, (&k, d))| {
                        let at = i * c.resblock_kernel_sizes.len() + j;
                        SnakeBlock::load(ch, k, d, style, &vb.pp("resblocks").pp(at))
                    })
                    .collect::<Result<_>>()?;
                Ok(Stage {
                    up,
                    noise_conv: noise_conv.load(&vb.pp("noise_convs").pp(i))?,
                    noise_res: SnakeBlock::load(
                        ch,
                        kernel,
                        &[1, 3, 5],
                        style,
                        &vb.pp("noise_res").pp(i),
                    )?,
                    blocks,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            source: Source::load(f0_scale, &vb.pp("m_source"))?,
            stft: Stft::new(n_fft, c.gen_istft_hop_size)?,
            stages,
            post: Conv::new(first >> count, n_fft + 2, 7)
                .padding(3)
                .load(&vb.pp("conv_post"))?,
            f0_scale,
        })
    }

    fn forward(
        &self,
        x: &Tensor,
        s: &Tensor,
        f0: &Tensor,
        noise: &mut Noise,
        probes: &mut Probes,
    ) -> Result<Vec<f32>> {
        probes.keep(|| "x0".into(), x);
        let f0 = upsample(&f0.unsqueeze(1)?, f0.dim(1)? * self.f0_scale)?.transpose(1, 2)?;
        let source = self.source.forward(&f0, noise)?;
        let source = source.transpose(1, 2)?.squeeze(1)?;
        probes.keep(|| "source".into(), &source);
        let (spec, phase) = self.stft.transform(&source)?;
        let har = Tensor::cat(&[&spec, &phase], 1)?;
        probes.keep(|| "har".into(), &har);
        let mut x = x.clone();
        let last = self.stages.len().saturating_sub(1);
        for (i, stage) in self.stages.iter().enumerate() {
            x = leaky_relu(&x, 0.1)?;
            let src = stage.noise_conv.forward(&har)?;
            let src = stage.noise_res.forward(&src, s)?;
            probes.keep(|| format!("stage{i}.src"), &src);
            x = stage.up.forward(&x)?;
            if i == last {
                // Reflection-pad one step at the start.
                x = Tensor::cat(&[&x.narrow(2, 1, 1)?, &x], 2)?;
            }
            let n = x.dim(2)?.min(src.dim(2)?);
            x = (x.narrow(2, 0, n)? + src.narrow(2, 0, n)?)?;
            probes.keep(|| format!("stage{i}.sum"), &x);
            let mut sum: Option<Tensor> = None;
            for block in &stage.blocks {
                let y = block.forward(&x, s)?;
                sum = Some(match sum {
                    Some(prev) => (prev + y)?,
                    None => y,
                });
            }
            if let Some(sum) = sum {
                x = scale(&sum, 1.0 / stage.blocks.len() as f32)?;
            }
            probes.keep(|| format!("stage{i}.out"), &x);
        }
        let x = self.post.forward(&leaky_relu(&x, 0.01)?)?;
        probes.keep(|| "post".into(), &x);
        let bins = self.stft.bins();
        // Speech stays far below the ceiling; bad weights can't reach infinity.
        let spec = x.narrow(1, 0, bins)?.minimum(LOG_CEILING)?.exp()?;
        let phase = x.narrow(1, bins, bins)?.sin()?;
        self.stft.inverse(&spec, &phase)
    }
}

/// Largest log-magnitude the generator may pass to `exp`, about 1e13.
const LOG_CEILING: f32 = 30.0;

pub(crate) struct Decoder {
    encode: AdainResBlk1d,
    decode: Vec<AdainResBlk1d>,
    f0_conv: Conv1d,
    noise_conv: Conv1d,
    asr_res: Conv1d,
    generator: Generator,
}

impl Decoder {
    pub(crate) fn load(hidden: usize, style: usize, c: &Istft, vb: &VarBuilder) -> Result<Self> {
        let wide = 1024 + 2 + 64;
        let decode = (0..4)
            .map(|i| {
                let out = if i == 3 { 512 } else { 1024 };
                AdainResBlk1d::load((wide, out), style, i == 3, &vb.pp("decode").pp(i))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            encode: AdainResBlk1d::load((hidden + 2, 1024), style, false, &vb.pp("encode"))?,
            decode,
            f0_conv: Conv::new(1, 1, 3)
                .stride(2)
                .padding(1)
                .load(&vb.pp("F0_conv"))?,
            noise_conv: Conv::new(1, 1, 3)
                .stride(2)
                .padding(1)
                .load(&vb.pp("N_conv"))?,
            asr_res: Conv::new(hidden, 64, 1).load(&vb.pp("asr_res.0"))?,
            generator: Generator::load(c, style, &vb.pp("generator"))?,
        })
    }

    /// Aligned features, pitch and energy curves, and style, to samples.
    pub(crate) fn forward(
        &self,
        asr: &Tensor,
        curves: (&Tensor, &Tensor),
        s: &Tensor,
        noise: &mut Noise,
        probes: &mut Probes,
    ) -> Result<Vec<f32>> {
        let (f0_curve, energy) = curves;
        let f0 = self.f0_conv.forward(&f0_curve.unsqueeze(1)?)?;
        let n = self.noise_conv.forward(&energy.unsqueeze(1)?)?;
        let mut x = self.encode.forward(&Tensor::cat(&[asr, &f0, &n], 1)?, s)?;
        let res = self.asr_res.forward(asr)?;
        let mut residual = true;
        for block in &self.decode {
            if residual {
                x = Tensor::cat(&[&x, &res, &f0, &n], 1)?;
            }
            x = block.forward(&x, s)?;
            residual &= !block.upsamples();
        }
        self.generator.forward(&x, s, f0_curve, noise, probes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_ends_and_halves() {
        assert_eq!(resample(&[0.0, 2.0], 2), [0.0, 2.0]);
        assert_eq!(resample(&[1.0], 3), [1.0, 1.0, 1.0]);
        assert_eq!(resample(&[0.0, 1.0, 2.0, 3.0], 2), [0.5, 2.5]);
        assert!(resample(&[], 4).is_empty());
    }

    #[test]
    fn a_negative_level_has_phase_plus_pi_at_dc() -> Result<()> {
        let stft = Stft::new(20, 5)?;
        let t = Tensor::from_vec(vec![-0.3f32; 100], (1, 100), &Device::Cpu)?;
        let (_, phase) = stft.transform(&t)?;
        let dc = phase.get(0)?.get(0)?.to_vec1::<f32>()?;
        assert!(dc.iter().all(|p| *p == std::f32::consts::PI), "{dc:?}");
        Ok(())
    }

    #[test]
    fn the_inverse_stft_gives_back_what_it_was_given() -> Result<()> {
        let stft = Stft::new(20, 5)?;
        let wave: Vec<f32> = (0..400).map(|i| (i as f32 * 0.07).sin() * 0.5).collect();
        let t = Tensor::from_vec(wave.clone(), (1, 400), &Device::Cpu)?;
        let (m, p) = stft.transform(&t)?;
        let back = stft.inverse(&m, &p)?;
        assert_eq!(back.len(), wave.len());
        let worst = back
            .iter()
            .zip(&wave)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(worst < 1e-4, "round trip off by {worst}");
        Ok(())
    }
}
