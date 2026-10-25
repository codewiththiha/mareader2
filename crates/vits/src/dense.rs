//! Convolutions on plain buffers, as wide multiplies or short loops.

use std::sync::{Mutex, PoisonError};

use candle_core::{Device, Storage, Tensor};

use crate::onnx::ConvSpec;
use crate::{Error, Result, simd};

/// Floats of unrolled input per multiply, so memory stays flat on long text.
const CHUNK_FLOATS: usize = 2 << 20;
/// Floats below which a job stays on one core: threads cost more there.
const ALONE: usize = 1 << 15;

/// Channels as rows of `len` steps, end to end.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Signal {
    pub(crate) data: Vec<f32>,
    pub(crate) channels: usize,
    pub(crate) len: usize,
}

impl Signal {
    /// From a `[1, C, T]` tensor.
    pub(crate) fn from_tensor(x: &Tensor) -> Result<Self> {
        let (batch, channels, len) = x.dims3()?;
        if batch != 1 {
            return Err(Error::Compute("one sentence at a time".into()));
        }
        let data = x.flatten_all()?.to_vec1::<f32>()?;
        Ok(Self {
            data,
            channels,
            len,
        })
    }

    /// As a `[1, C, T]` tensor, the buffer moved, not copied.
    pub(crate) fn into_tensor(self) -> Result<Tensor> {
        let shape = (1, self.channels, self.len);
        Ok(Tensor::from_vec(self.data, shape, &Device::Cpu)?)
    }

    pub(crate) fn row(&self, channel: usize) -> &[f32] {
        let at = channel * self.len;
        self.data.get(at..at + self.len).unwrap_or(&[])
    }

    /// Every value through `prep`, as the next layer will see it.
    pub(crate) fn prepped(&self, prep: Prep) -> Self {
        let mut data = vec![0.0; self.data.len()];
        each_row(&mut data, self.len, |c, row| {
            for (d, &s) in row.iter_mut().zip(self.row(c)) {
                *d = prep.apply(s);
            }
        });
        Self { data, ..*self }
    }

    /// Adds `other`, which must have the same shape.
    pub(crate) fn add(&mut self, other: &Self) -> Result<()> {
        if (self.channels, self.len) != (other.channels, other.len) {
            return Err(Error::Compute("signals differ in shape".into()));
        }
        let len = self.len;
        each_row(&mut self.data, len, |c, row| {
            for (d, &s) in row.iter_mut().zip(other.row(c)) {
                *d += s;
            }
        });
        Ok(())
    }

    /// Channels `from..from + count`, copied out.
    pub(crate) fn rows(&self, from: usize, count: usize) -> Result<Self> {
        let at = from * self.len..(from + count) * self.len;
        let data = self
            .data
            .get(at)
            .ok_or_else(|| Error::Compute("past the channels".into()))?;
        Ok(Self {
            data: data.to_vec(),
            channels: count,
            len: self.len,
        })
    }

    /// Adds channels `from..` of `other` to this signal's channels.
    pub(crate) fn add_rows(&mut self, other: &Self, from: usize) -> Result<()> {
        let at = from * other.len;
        let src = other.data.get(at..at + self.data.len());
        match src {
            Some(src) if other.len == self.len => {
                self.data.iter_mut().zip(src).for_each(|(d, s)| *d += s);
                Ok(())
            }
            _ => Err(Error::Compute("signals differ in shape".into())),
        }
    }

    /// Adds `per_channel[c]` to every step of channel `c`.
    pub(crate) fn add_per_channel(&mut self, per_channel: &[f32]) -> Result<()> {
        if per_channel.len() != self.channels {
            return Err(Error::Compute("one value per channel expected".into()));
        }
        for (row, &v) in self.data.chunks_mut(self.len.max(1)).zip(per_channel) {
            row.iter_mut().for_each(|d| *d += v);
        }
        Ok(())
    }
}

/// A scale, then an optional leaky ReLU, applied to a layer's input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Prep {
    pub(crate) scale: f32,
    pub(crate) slope: Option<f32>,
}

impl Prep {
    pub(crate) const NONE: Self = Self {
        scale: 1.0,
        slope: None,
    };

    pub(crate) fn leaky(slope: f32) -> Self {
        Self {
            scale: 1.0,
            slope: Some(slope),
        }
    }

    fn apply(self, v: f32) -> f32 {
        let v = v * self.scale;
        match self.slope {
            Some(s) => v.max(v * s),
            None => v,
        }
    }
}

/// A stride-1, ungrouped convolution as one wide multiply per chunk.
pub(crate) struct DenseConv {
    /// `[outputs, kernel·inputs]`, columns tap by tap to match `unroll`.
    weight: Tensor,
    bias: Vec<f32>,
    inputs: usize,
    outputs: usize,
    kernel: usize,
    dilation: usize,
    pads: (usize, usize),
}

impl DenseConv {
    pub(crate) fn new(weight: &Tensor, bias: Option<&Tensor>, s: ConvSpec) -> Result<Self> {
        if s.stride != 1 || s.groups != 1 {
            return Err(Error::Load("only plain convolutions run dense".into()));
        }
        let (outputs, inputs, kernel) = weight.dims3()?;
        let weight = weight
            .permute((0, 2, 1))?
            .contiguous()?
            .reshape((outputs, kernel * inputs))?;
        let bias = match bias {
            Some(b) => b.flatten_all()?.to_vec1::<f32>()?,
            None => vec![0.0; outputs],
        };
        if bias.len() != outputs {
            return Err(Error::Load("a bias doesn't fit its convolution".into()));
        }
        Ok(Self {
            weight,
            bias,
            inputs,
            outputs,
            kernel,
            dilation: s.dilation.max(1),
            pads: s.pads,
        })
    }

    pub(crate) fn outputs(&self) -> usize {
        self.outputs
    }

    pub(crate) fn kernel(&self) -> usize {
        self.kernel
    }

    /// `conv(prep(x)) + residual`; the residual has the output's shape.
    pub(crate) fn forward(
        &self,
        x: &Signal,
        prep: Prep,
        residual: Option<&Signal>,
    ) -> Result<Signal> {
        self.forward_in(x, prep, residual, CHUNK_FLOATS)
    }

    fn forward_in(
        &self,
        x: &Signal,
        prep: Prep,
        residual: Option<&Signal>,
        chunk_floats: usize,
    ) -> Result<Signal> {
        if x.channels != self.inputs {
            return Err(Error::Compute(
                "a convolution got the wrong channels".into(),
            ));
        }
        let reach = self.dilation * (self.kernel.saturating_sub(1));
        let len = (x.len + self.pads.0 + self.pads.1)
            .checked_sub(reach)
            .ok_or_else(|| Error::Input("too short to convolve".into()))?;
        if let Some(r) = residual
            && (r.channels, r.len) != (self.outputs, len)
        {
            return Err(Error::Compute("a residual differs in shape".into()));
        }
        let mut out = vec![0.0f32; self.outputs * len];
        let rows = self.kernel * self.inputs;
        let step = (chunk_floats / rows.max(1)).max(64);
        let mut start = 0;
        while start < len {
            let cols = step.min(len - start);
            let y = self.weight.matmul(&self.unroll(x, prep, start, cols)?)?;
            read(&y, |y| {
                each_row(&mut out, len, |co, dst| {
                    let src = y.get(co * cols..(co + 1) * cols).unwrap_or_default();
                    let dst = dst.get_mut(start..start + cols).unwrap_or_default();
                    let bias = self.bias.get(co).copied().unwrap_or(0.0);
                    let add = residual.map(|r| r.row(co).get(start..start + cols));
                    match add.flatten() {
                        Some(add) => {
                            for ((d, &s), &a) in dst.iter_mut().zip(src).zip(add) {
                                *d = s + bias + a;
                            }
                        }
                        None => {
                            for (d, &s) in dst.iter_mut().zip(src) {
                                *d = s + bias;
                            }
                        }
                    }
                });
            })?;
            start += cols;
        }
        Ok(Signal {
            data: out,
            channels: self.outputs,
            len,
        })
    }

    /// `[kernel·inputs, cols]`; row `k·inputs + c` is channel `c` at tap `k`.
    fn unroll(&self, x: &Signal, prep: Prep, start: usize, cols: usize) -> Result<Tensor> {
        let rows = self.kernel * self.inputs;
        let mut col = vec![0.0f32; rows * cols];
        let inputs = self.inputs.max(1);
        each_row(&mut col, cols, |r, dst| {
            let (tap, channel) = (r / inputs, r % inputs);
            let from = (start + tap * self.dilation) as isize - self.pads.0 as isize;
            shifted(dst, x.row(channel), from, prep);
        });
        Ok(Tensor::from_vec(col, (rows, cols), &Device::Cpu)?)
    }
}

/// A transposed convolution: one wide multiply, then each output's taps
/// summed into place.
pub(crate) struct Transposed {
    /// `[outputs·kernel, inputs]`: row `o·kernel + k` is tap `k` of output `o`.
    weight: Tensor,
    bias: Vec<f32>,
    inputs: usize,
    outputs: usize,
    kernel: usize,
    stride: usize,
    pads: (usize, usize),
}

impl Transposed {
    /// From a weight stored `[inputs, outputs, kernel]`, as the file keeps it.
    pub(crate) fn new(weight: &Tensor, bias: Option<&Tensor>, s: ConvSpec) -> Result<Self> {
        let (inputs, outputs, kernel) = weight.dims3()?;
        let weight = weight
            .permute((1, 2, 0))?
            .contiguous()?
            .reshape((outputs * kernel, inputs))?;
        let bias = match bias {
            Some(b) => b.flatten_all()?.to_vec1::<f32>()?,
            None => vec![0.0; outputs],
        };
        if bias.len() != outputs {
            return Err(Error::Load("a bias doesn't fit its convolution".into()));
        }
        Ok(Self {
            weight,
            bias,
            inputs,
            outputs,
            kernel,
            stride: s.stride.max(1),
            pads: s.pads,
        })
    }

    pub(crate) fn outputs(&self) -> usize {
        self.outputs
    }

    pub(crate) fn kernel(&self) -> usize {
        self.kernel
    }

    /// `prep(x)` upsampled, with the padding at both ends cut away.
    pub(crate) fn forward(&self, x: &Signal, prep: Prep) -> Result<Signal> {
        self.forward_in(x, prep, CHUNK_FLOATS)
    }

    fn forward_in(&self, x: &Signal, prep: Prep, chunk_floats: usize) -> Result<Signal> {
        if x.channels != self.inputs {
            return Err(Error::Compute(
                "a convolution got the wrong channels".into(),
            ));
        }
        let full = x.len.saturating_sub(1) * self.stride + self.kernel;
        let len = (full.checked_sub(self.pads.0 + self.pads.1))
            .filter(|_| x.len > 0)
            .ok_or_else(|| Error::Input("too short to upsample".into()))?;
        let mut out = vec![0.0f32; self.outputs * len];
        each_row(&mut out, len, |o, row| {
            row.fill(self.bias.get(o).copied().unwrap_or(0.0));
        });
        let x = x.prepped(prep);
        let rows = self.outputs * self.kernel;
        let step = (chunk_floats / rows.max(1)).max(64);
        let mut start = 0;
        while start < x.len {
            let cols = step.min(x.len - start);
            let mut part = vec![0.0f32; self.inputs * cols];
            each_row(&mut part, cols, |c, dst| {
                if let Some(src) = x.row(c).get(start..start + cols) {
                    dst.copy_from_slice(src);
                }
            });
            let part = Tensor::from_vec(part, (self.inputs, cols), &Device::Cpu)?;
            let products = self.weight.matmul(&part)?;
            read(&products, |p| {
                each_row(&mut out, len, |o, row| {
                    let at = Taps {
                        output: o,
                        kernel: self.kernel,
                        cols,
                        start,
                        stride: self.stride,
                        pad: self.pads.0,
                    };
                    simd::pick(add_taps, (row, p, at));
                });
            })?;
            start += cols;
        }
        Ok(Signal {
            data: out,
            channels: self.outputs,
            len,
        })
    }
}

/// Where one output's tap rows land in its row.
#[derive(Clone, Copy)]
struct Taps {
    output: usize,
    kernel: usize,
    /// Input steps in this block, from `start`.
    cols: usize,
    start: usize,
    stride: usize,
    pad: usize,
}

/// Adds one output's taps: tap `k` of step `j` lands at
/// `(start+j)·stride+k−pad`.
#[inline(always)]
fn add_taps((row, products, at): (&mut [f32], &[f32], Taps)) {
    for k in 0..at.kernel {
        let from = (at.output * at.kernel + k) * at.cols;
        let Some(taps) = products.get(from..from + at.cols) else {
            break;
        };
        for (j, v) in taps.iter().enumerate() {
            let t = ((at.start + j) * at.stride + k).checked_sub(at.pad);
            if let Some(slot) = t.and_then(|t| row.get_mut(t)) {
                *slot += v;
            }
        }
    }
}

/// A stride-1 convolution with one group per channel, as plain loops.
pub(crate) struct Depthwise {
    /// `[channels, kernel]`.
    weight: Vec<f32>,
    bias: Vec<f32>,
    channels: usize,
    kernel: usize,
    dilation: usize,
    pads: (usize, usize),
}

impl Depthwise {
    /// `None` unless the weight is `[C, 1, K]` with `C` groups and stride 1.
    pub(crate) fn new(weight: &Tensor, bias: Option<&Tensor>, s: ConvSpec) -> Result<Option<Self>> {
        let (channels, per_group, kernel) = weight.dims3()?;
        if s.stride != 1 || per_group != 1 || s.groups != channels || channels < 2 {
            return Ok(None);
        }
        let bias = match bias {
            Some(b) => b.flatten_all()?.to_vec1::<f32>()?,
            None => vec![0.0; channels],
        };
        if bias.len() != channels {
            return Err(Error::Load("a bias doesn't fit its convolution".into()));
        }
        Ok(Some(Self {
            weight: weight.flatten_all()?.to_vec1::<f32>()?,
            bias,
            channels,
            kernel,
            dilation: s.dilation.max(1),
            pads: s.pads,
        }))
    }

    pub(crate) fn channels(&self) -> usize {
        self.channels
    }

    pub(crate) fn kernel(&self) -> usize {
        self.kernel
    }

    pub(crate) fn forward(&self, x: &Signal) -> Result<Signal> {
        if x.channels != self.channels {
            return Err(Error::Compute(
                "a convolution got the wrong channels".into(),
            ));
        }
        let reach = self.dilation * self.kernel.saturating_sub(1);
        let len = (x.len + self.pads.0 + self.pads.1)
            .checked_sub(reach)
            .ok_or_else(|| Error::Input("too short to convolve".into()))?;
        let mut data = vec![0.0f32; self.channels * len];
        let kernel = self.kernel.max(1);
        each_row(&mut data, len, |c, out| {
            let reach = Reach {
                bias: self.bias.get(c).copied().unwrap_or(0.0),
                dilation: self.dilation,
                pad: self.pads.0,
            };
            let taps = self.weight.get(c * kernel..(c + 1) * kernel).unwrap_or(&[]);
            simd::pick(depthwise_row, (out, x.row(c), taps, reach));
        });
        Ok(Signal {
            data,
            channels: self.channels,
            len,
        })
    }
}

/// A depthwise channel's bias, and how its taps spread.
#[derive(Clone, Copy)]
struct Reach {
    bias: f32,
    dilation: usize,
    pad: usize,
}

/// One channel of a depthwise convolution: the bias plus each tap's share.
#[inline(always)]
fn depthwise_row((out, src, taps, reach): (&mut [f32], &[f32], &[f32], Reach)) {
    let len = out.len();
    out.fill(reach.bias);
    for (k, &w) in taps.iter().enumerate() {
        let from = (k * reach.dilation) as isize - reach.pad as isize;
        let lo = (-from).clamp(0, len as isize) as usize;
        let hi = (src.len() as isize - from).clamp(lo as isize, len as isize) as usize;
        let src_lo = (from + lo as isize).max(0) as usize;
        let src = src.get(src_lo..src_lo + hi - lo).unwrap_or(&[]);
        for (d, &v) in out.get_mut(lo..hi).unwrap_or_default().iter_mut().zip(src) {
            *d += w * v;
        }
    }
}

/// `dst[j] = prep(src[from + j])`, zero where that falls outside `src`.
fn shifted(dst: &mut [f32], src: &[f32], from: isize, prep: Prep) {
    let n = dst.len() as isize;
    let lo = (-from).clamp(0, n);
    let hi = (src.len() as isize - from).clamp(lo, n);
    let (lo, hi) = (lo as usize, hi as usize);
    let src_lo = (from + lo as isize).max(0) as usize;
    if let (Some(d), Some(s)) = (dst.get_mut(lo..hi), src.get(src_lo..src_lo + hi - lo)) {
        for (d, &s) in d.iter_mut().zip(s) {
            *d = prep.apply(s);
        }
    }
    dst.get_mut(..lo).unwrap_or_default().fill(0.0);
    dst.get_mut(hi..).unwrap_or_default().fill(0.0);
}

/// A fresh, contiguous CPU tensor's floats, borrowed.
fn read<R>(t: &Tensor, f: impl FnOnce(&[f32]) -> R) -> Result<R> {
    let (storage, layout) = t.storage_and_layout();
    let Storage::Cpu(cpu) = &*storage else {
        return Err(Error::Compute("expected a CPU tensor".into()));
    };
    if !layout.is_contiguous() {
        return Err(Error::Compute("expected a contiguous tensor".into()));
    }
    let floats = cpu.as_slice::<f32>()?;
    let floats = floats.get(layout.start_offset()..).unwrap_or_default();
    Ok(f(floats))
}

/// The cores worth using, at most eight.
fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(8))
}

/// `fill(i, row)` on each `width`-long row, across cores when worth it.
pub(crate) fn each_row(data: &mut [f32], width: usize, fill: impl Fn(usize, &mut [f32]) + Sync) {
    let width = width.max(1);
    let rows = data.len().div_ceil(width);
    let cores = cores().min(rows);
    if cores < 2 || data.len() < ALONE {
        for (i, row) in data.chunks_mut(width).enumerate() {
            fill(i, row);
        }
        return;
    }
    let per = rows.div_ceil(cores);
    let parts: Vec<Mutex<Option<&mut [f32]>>> = (data.chunks_mut(per * width))
        .map(|part| Mutex::new(Some(part)))
        .collect();
    // Each worker takes the parts no one has yet.
    let work = || {
        for (at, part) in parts.iter().enumerate() {
            let taken = part.lock().unwrap_or_else(PoisonError::into_inner).take();
            for (i, row) in taken
                .into_iter()
                .flat_map(|p| p.chunks_mut(width))
                .enumerate()
            {
                fill(at * per + i, row);
            }
        }
    };
    // On candle's pool: its threads stay warm between calls.
    candle_core::cpu::kernels::par_for_each(cores, |_| work());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers::Strided;

    fn leaky_relu(x: &Tensor, slope: f64) -> Tensor {
        x.maximum(&x.affine(slope, 0.0).unwrap()).unwrap()
    }

    fn wave(n: usize, seed: f32) -> Vec<f32> {
        (0..n)
            .map(|i| (i as f32 * 0.37 + seed).sin() * 1.3)
            .collect()
    }

    fn conv(outputs: usize, inputs: usize, kernel: usize, spec: ConvSpec) -> Strided {
        let w = wave(outputs * inputs * kernel, 0.5);
        let b = wave(outputs, 2.0);
        Strided {
            weight: Tensor::from_vec(w, (outputs, inputs, kernel), &Device::Cpu).unwrap(),
            bias: Some(Tensor::from_vec(b, outputs, &Device::Cpu).unwrap()),
            spec,
            transposed: false,
        }
    }

    fn close(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len());
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            assert!((x - y).abs() <= 1e-3 * (1.0 + y.abs()), "{i}: {x} vs {y}");
        }
    }

    #[test]
    fn dense_matches_candle_in_chunks() {
        let cases = [
            (3, 5, 3, 1, (1, 1), 40),
            (4, 3, 7, 3, (9, 9), 50),
            (2, 4, 5, 1, (0, 4), 9),
            (8, 8, 3, 5, (5, 5), 9_000),
        ];
        for (outputs, inputs, kernel, dilation, pads, len) in cases {
            let spec = ConvSpec {
                pads,
                stride: 1,
                dilation,
                groups: 1,
            };
            let c = conv(outputs, inputs, kernel, spec);
            let dense = DenseConv::new(&c.weight, c.bias.as_ref(), spec).unwrap();
            let x = Signal {
                data: wave(inputs * len, 1.0),
                channels: inputs,
                len,
            };
            let t = x.clone().into_tensor().unwrap();
            let want = c.forward(&leaky_relu(&t, 0.1)).unwrap();
            let want = Signal::from_tensor(&want).unwrap();
            let residual = Signal {
                data: wave(want.data.len(), 3.0),
                ..want.clone()
            };
            for chunk in [CHUNK_FLOATS, 7 * rows_of(&dense)] {
                let got = dense.forward_in(&x, Prep::leaky(0.1), None, chunk).unwrap();
                assert_eq!((got.channels, got.len), (want.channels, want.len));
                close(&got.data, &want.data);
                let got = dense
                    .forward_in(&x, Prep::leaky(0.1), Some(&residual), chunk)
                    .unwrap();
                let mut sum = want.clone();
                sum.add(&residual).unwrap();
                close(&got.data, &sum.data);
            }
        }
    }

    fn rows_of(d: &DenseConv) -> usize {
        d.kernel * d.inputs
    }

    #[test]
    fn depthwise_matches_candle() {
        for (channels, kernel, dilation, pads, len) in [
            (4, 3, 1, (1, 1), 20),
            (3, 5, 2, (4, 4), 31),
            (2, 3, 1, (0, 2), 4),
        ] {
            let spec = ConvSpec {
                pads,
                stride: 1,
                dilation,
                groups: channels,
            };
            let c = conv(channels, 1, kernel, spec);
            let depthwise = Depthwise::new(&c.weight, c.bias.as_ref(), spec)
                .unwrap()
                .unwrap();
            let x = Signal {
                data: wave(channels * len, 1.0),
                channels,
                len,
            };
            let want = c.forward(&x.clone().into_tensor().unwrap()).unwrap();
            let want = Signal::from_tensor(&want).unwrap();
            let got = depthwise.forward(&x).unwrap();
            assert_eq!((got.channels, got.len), (want.channels, want.len));
            close(&got.data, &want.data);
        }
        let plain = ConvSpec::default();
        let c = conv(3, 3, 3, plain);
        assert!(Depthwise::new(&c.weight, None, plain).unwrap().is_none());
    }

    #[test]
    fn transposed_matches_candle_in_chunks() {
        for (inputs, outputs, kernel, stride, pads, len) in [
            (3, 2, 16, 8, (4, 4), 30),
            (4, 3, 4, 2, (1, 1), 9_000),
            (2, 2, 3, 1, (0, 0), 5),
        ] {
            let spec = ConvSpec {
                pads,
                stride,
                dilation: 1,
                groups: 1,
            };
            let w = wave(inputs * outputs * kernel, 0.5);
            let b = wave(outputs, 2.0);
            let c = Strided {
                weight: Tensor::from_vec(w, (inputs, outputs, kernel), &Device::Cpu).unwrap(),
                bias: Some(Tensor::from_vec(b, outputs, &Device::Cpu).unwrap()),
                spec,
                transposed: true,
            };
            let x = Signal {
                data: wave(inputs * len, 1.0),
                channels: inputs,
                len,
            };
            let t = x.clone().into_tensor().unwrap();
            let want = c.forward(&leaky_relu(&t, 0.1)).unwrap();
            let want = Signal::from_tensor(&want).unwrap();
            let up = Transposed::new(&c.weight, c.bias.as_ref(), spec).unwrap();
            for chunk in [CHUNK_FLOATS, 7 * outputs * kernel] {
                let got = up.forward_in(&x, Prep::leaky(0.1), chunk).unwrap();
                assert_eq!((got.channels, got.len), (want.channels, want.len));
                close(&got.data, &want.data);
            }
        }
    }

    #[test]
    fn rows_fill_alike_on_one_core_or_many() {
        let width = 1000;
        let mut many = vec![0f32; 64 * width + 10];
        each_row(&mut many, width, |r, row| row.fill(r as f32));
        let rows: Vec<&[f32]> = many.chunks(width).collect();
        assert_eq!(rows.len(), 65);
        for (r, row) in rows.iter().enumerate() {
            assert!(row.iter().all(|v| *v == r as f32), "row {r}");
        }
    }

    #[test]
    fn prep_scales_then_leaks() {
        let p = Prep {
            scale: 0.5,
            slope: Some(0.1),
        };
        assert_eq!(p.apply(4.0), 2.0);
        assert!((p.apply(-4.0) + 0.2).abs() < 1e-6);
        assert_eq!(Prep::NONE.apply(-3.0), -3.0);
    }

    #[test]
    fn shifts_pad_with_zeros_on_both_sides() {
        let src = [1.0, 2.0, 3.0];
        let mut dst = [9.0; 5];
        shifted(&mut dst, &src, -1, Prep::NONE);
        assert_eq!(dst, [0.0, 1.0, 2.0, 3.0, 0.0]);
        shifted(&mut dst, &src, 2, Prep::NONE);
        assert_eq!(dst, [3.0, 0.0, 0.0, 0.0, 0.0]);
        shifted(&mut dst, &src, 7, Prep::NONE);
        assert_eq!(dst, [0.0; 5]);
    }

    #[test]
    fn mismatched_shapes_are_errors() {
        let spec = ConvSpec::default();
        let c = conv(2, 3, 3, spec);
        let dense = DenseConv::new(&c.weight, c.bias.as_ref(), spec).unwrap();
        let x = Signal {
            data: vec![0.0; 8],
            channels: 2,
            len: 4,
        };
        assert!(dense.forward(&x, Prep::NONE, None).is_err());
        let mut a = x.clone();
        assert!(
            a.add(&Signal {
                len: 2,
                data: vec![0.0; 4],
                ..x
            })
            .is_err()
        );
    }
}
