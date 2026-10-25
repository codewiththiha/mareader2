//! Hot loops by hand: candle runs them a value, and a core, at a time.

use std::sync::{Mutex, PoisonError};

use candle_core::{Device, Result, Storage, Tensor};

use crate::simd;

/// Values below which a job stays on one core: threads cost more there.
const ALONE: usize = 1 << 15;
/// Floats in one block of gathered conv inputs, about 8 MB.
const BLOCK: usize = 1 << 21;

/// The cores worth using, at most eight.
fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(8))
}

/// Runs `f` on `t`'s values, laid out contiguously.
pub(crate) fn with_values<R>(t: &Tensor, f: impl FnOnce(&[f32]) -> Result<R>) -> Result<R> {
    let t = t.contiguous()?;
    let (storage, layout) = t.storage_and_layout();
    let Storage::Cpu(cpu) = &*storage else {
        candle_core::bail!("hand-written loops run on the CPU only");
    };
    let start = layout.start_offset();
    let all = cpu.as_slice::<f32>()?;
    let Some(values) = all.get(start..start + layout.shape().elem_count()) else {
        candle_core::bail!("a tensor's values run past its storage");
    };
    f(values)
}

/// `out` cut into rows of `len`, each filled by `fill(row, slot)` across cores.
pub(crate) fn by_rows(out: &mut [f32], len: usize, fill: impl Fn(usize, &mut [f32]) + Sync) {
    let rows = out.len().checked_div(len).unwrap_or(0);
    let cores = cores().min(rows);
    if cores < 2 || out.len() < ALONE {
        for (i, row) in out.chunks_exact_mut(len.max(1)).enumerate() {
            fill(i, row);
        }
        return;
    }
    let per = rows.div_ceil(cores);
    let parts: Vec<Mutex<Option<&mut [f32]>>> = (out.chunks_mut(per * len))
        .map(|part| Mutex::new(Some(part)))
        .collect();
    // Each worker takes the parts no one has; this thread helps too.
    let work = || {
        for (at, part) in parts.iter().enumerate() {
            let taken = part.lock().unwrap_or_else(PoisonError::into_inner).take();
            for (i, row) in taken
                .into_iter()
                .flat_map(|p| p.chunks_exact_mut(len))
                .enumerate()
            {
                fill(at * per + i, row);
            }
        }
    };
    std::thread::scope(|s| {
        for _ in 1..cores {
            let named = std::thread::Builder::new().name("kokoro maths".into());
            // A thread that can't start leaves its share to the others.
            named.spawn_scoped(s, work).ok();
        }
        work();
    });
}

/// What follows a norm, value by value.
#[derive(Clone, Copy)]
pub(crate) enum Then<'a> {
    /// Leaky ReLU with this slope.
    Leaky(f32),
    /// Snake, `x + sin²(αx) / α`, with each channel's `α`.
    Snake(&'a [f32]),
}

/// Sums eight lanes at a time, so the loop vectorises.
#[inline(always)]
fn lane_sum(values: &[f32], f: impl Fn(f32) -> f32) -> f32 {
    let mut lanes = [0f32; 8];
    let (blocks, rest) = values.as_chunks::<8>();
    for block in blocks {
        for (lane, v) in lanes.iter_mut().zip(block) {
            *lane += f(*v);
        }
    }
    lanes.iter().sum::<f32>() + rest.iter().map(|v| f(*v)).sum::<f32>()
}

/// Rows `[channels, len]` normalised, scaled, shifted, then `then`.
pub(crate) fn norm_rows(
    values: &[f32],
    len: usize,
    (scale, shift): (&[f32], &[f32]),
    eps: f32,
    then: Then<'_>,
) -> Vec<f32> {
    let mut out = vec![0f32; values.len()];
    by_rows(&mut out, len, |c, row| {
        let Some(x) = values.get(c * len..(c + 1) * len) else {
            return;
        };
        let k = scale.get(c).copied().unwrap_or(1.0);
        let m = shift.get(c).copied().unwrap_or(0.0);
        let after = match then {
            Then::Leaky(slope) => After::Leaky(slope),
            Then::Snake(alphas) => After::Snake(alphas.get(c).copied().unwrap_or(1.0)),
        };
        simd::pick(norm_row, (x, row, (k, m), eps, after));
    });
    out
}

/// What follows one row's norm.
#[derive(Clone, Copy)]
enum After {
    Leaky(f32),
    /// Snake with this row's `α`.
    Snake(f32),
}

/// One row normalised, scaled by `k`, shifted by `m`, then `after`.
#[inline(always)]
fn norm_row((x, row, (k, m), eps, after): (&[f32], &mut [f32], (f32, f32), f32, After)) {
    let len = x.len().max(1) as f32;
    let mean = lane_sum(x, |v| v) / len;
    let var = lane_sum(x, |v| (v - mean) * (v - mean)) / len;
    let k = k / (var + eps).sqrt();
    let normed = x.iter().map(|v| (v - mean) * k + m);
    match after {
        After::Leaky(slope) => {
            (row.iter_mut().zip(normed)).for_each(|(o, y)| *o = y.max(y * slope))
        }
        After::Snake(a) => {
            let inv = a.recip();
            for (o, y) in row.iter_mut().zip(normed) {
                *o = y + sin_squared(y * a) * inv;
            }
        }
    }
}

/// `sin²(x)` without branches, so loops vectorise; within 1e-6 of `sin`'s.
#[inline(always)]
fn sin_squared(x: f32) -> f32 {
    use std::f32::consts::{FRAC_1_PI, FRAC_PI_2};
    // Adding 1.5·2²³ rounds to a whole number in plain float maths.
    const ROUND: f32 = 12_582_912.0;
    // π in two parts, the first exact in few bits, so k·π loses nothing.
    const PI_HIGH: f32 = 3.140_625;
    const PI_LOW: f32 = 9.676_536e-4;
    // Squaring drops the sign, so the period shrinks to π.
    let k = (x * FRAC_1_PI + ROUND) - ROUND;
    let r = ((x - k * PI_HIGH) - k * PI_LOW).clamp(-FRAC_PI_2, FRAC_PI_2);
    let r2 = r * r;
    let tail = 1.0 / 362_880.0 - r2 / 39_916_800.0;
    let s = r * (1.0 + r2 * (-1.0 / 6.0 + r2 * (1.0 / 120.0 + r2 * (-1.0 / 5040.0 + r2 * tail))));
    s * s
}

/// A stride-1, ungrouped conv's shape.
pub(crate) struct ConvShape {
    pub(crate) inputs: usize,
    pub(crate) kernel: usize,
    pub(crate) padding: usize,
    pub(crate) dilation: usize,
}

/// `x` `[inputs, len]` convolved, gathered a block of steps at a time.
pub(crate) fn conv(
    x: &[f32],
    len: usize,
    shape: &ConvShape,
    weights: &Tensor,
    bias: Option<&[f32]>,
) -> Result<(Vec<f32>, usize)> {
    let ConvShape {
        inputs,
        kernel,
        padding,
        dilation,
    } = *shape;
    let outputs = weights.dim(0)?;
    let span = dilation * kernel.saturating_sub(1);
    let Some(out_len) = (len + 2 * padding).checked_sub(span) else {
        candle_core::bail!("{len} steps is too short for the conv");
    };
    let rows = inputs * kernel;
    let block = (BLOCK / rows.max(1)).max(256).min(out_len.max(1));
    let mut out = vec![0f32; outputs * out_len];
    for start in (0..out_len).step_by(block) {
        let width = block.min(out_len - start);
        let mut col = vec![0f32; rows * width];
        by_rows(&mut col, width, |r, slot| {
            let (c, k) = (r / kernel, r % kernel);
            // Step t reads x[t + k·dilation − padding], zero outside.
            let shift = (k * dilation) as isize - padding as isize;
            let from = (start as isize + shift).max(0) as usize;
            let to = ((start + width) as isize + shift).clamp(0, len as isize) as usize;
            if from >= to {
                return;
            }
            let at = (from as isize - start as isize - shift) as usize;
            let src = x.get(c * len + from..c * len + to);
            let dst = slot.get_mut(at..at + (to - from));
            if let (Some(src), Some(dst)) = (src, dst) {
                dst.copy_from_slice(src);
            }
        });
        let gathered = Tensor::from_vec(col, (rows, width), &Device::Cpu)?;
        let made = weights.matmul(&gathered)?;
        with_values(&made, |made| {
            for (o, row) in made.chunks_exact(width).enumerate() {
                let b = bias.and_then(|b| b.get(o)).copied().unwrap_or(0.0);
                let dst = out.get_mut(o * out_len + start..o * out_len + start + width);
                if let Some(dst) = dst {
                    dst.iter_mut().zip(row).for_each(|(d, v)| *d = v + b);
                }
            }
            Ok(())
        })?;
    }
    Ok((out, out_len))
}

/// A transposed conv's products `[len, outputs · kernel]` summed into place.
pub(crate) fn overlap_add(
    products: &[f32],
    (outputs, kernel): (usize, usize),
    (stride, padding, out_len): (usize, usize, usize),
    bias: Option<&[f32]>,
) -> Vec<f32> {
    let steps = products.len() / (outputs * kernel).max(1);
    let mut out = vec![0f32; outputs * out_len];
    by_rows(&mut out, out_len, |o, row| {
        row.fill(bias.and_then(|b| b.get(o)).copied().unwrap_or(0.0));
        for step in 0..steps {
            let at = (step * outputs + o) * kernel;
            let Some(taps) = products.get(at..at + kernel) else {
                continue;
            };
            for (k, v) in taps.iter().enumerate() {
                // Output t holds the full result's step t + padding.
                let t = (step * stride + k).checked_sub(padding);
                if let Some(slot) = t.and_then(|t| row.get_mut(t)) {
                    *slot += v;
                }
            }
        }
    });
    out
}

/// One LSTM direction's weights, as plain rows.
pub(crate) struct CellRows {
    /// `[hidden, 4 · hidden]`: row j is what `h[j]` adds to each gate.
    pub(crate) hidden_t: Vec<f32>,
}

/// One direction over `gates_in` `[steps, 4 · hidden]`, into `[steps, hidden]`.
pub(crate) fn lstm_run(
    cell: &CellRows,
    gates_in: &[f32],
    hidden: usize,
    reverse: bool,
) -> Vec<f32> {
    simd::pick(lstm_steps, (cell, gates_in, hidden, reverse))
}

/// `lstm_run`'s loop, built once per CPU kind.
#[inline(always)]
fn lstm_steps((cell, gates_in, hidden, reverse): (&CellRows, &[f32], usize, bool)) -> Vec<f32> {
    let width = 4 * hidden;
    let steps = gates_in.len() / width.max(1);
    let mut out = vec![0f32; steps * hidden];
    let (mut h, mut c) = (vec![0f32; hidden], vec![0f32; hidden]);
    let mut gates = vec![0f32; width];
    let sigmoid = |v: f32| 1.0 / (1.0 + (-v).exp());
    for step in 0..steps {
        let t = if reverse { steps - 1 - step } else { step };
        let Some(start) = gates_in.get(t * width..(t + 1) * width) else {
            continue;
        };
        gates.copy_from_slice(start);
        for (hj, row) in h.iter().zip(cell.hidden_t.chunks_exact(width)) {
            gates.iter_mut().zip(row).for_each(|(g, w)| *g += hj * w);
        }
        let (i, rest) = gates.split_at(hidden);
        let (f, rest) = rest.split_at(hidden);
        let (g, o) = rest.split_at(hidden);
        let parts = (i.iter().zip(f)).zip(g.iter().zip(o));
        for ((cv, hv), ((i, f), (g, o))) in c.iter_mut().zip(h.iter_mut()).zip(parts) {
            *cv = sigmoid(*f) * *cv + sigmoid(*i) * g.tanh();
            *hv = sigmoid(*o) * cv.tanh();
        }
        if let Some(slot) = out.get_mut(t * hidden..(t + 1) * hidden) {
            slot.copy_from_slice(&h);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_fill_alike_on_one_core_or_many() {
        let len = 1000;
        let mut many = vec![0f32; 64 * len];
        by_rows(&mut many, len, |r, row| row.fill(r as f32));
        assert!(
            many.chunks_exact(len)
                .enumerate()
                .all(|(r, row)| row.iter().all(|v| *v == r as f32))
        );
    }

    #[test]
    fn norms_centre_scale_and_then_apply() {
        let x = [1f32, 3.0, 5.0, 7.0];
        let plain = norm_rows(&x, 4, (&[1.0], &[0.0]), 0.0, Then::Leaky(1.0));
        let spread = (5f32).sqrt();
        let want = [-3.0 / spread, -1.0 / spread, 1.0 / spread, 3.0 / spread];
        assert!(plain.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6));
        let leaky = norm_rows(&x, 4, (&[1.0], &[0.0]), 0.0, Then::Leaky(0.5));
        assert!((leaky[0] - want[0] * 0.5).abs() < 1e-6 && leaky[3] == plain[3]);
        let snake = norm_rows(&x, 4, (&[1.0], &[0.0]), 0.0, Then::Snake(&[2.0]));
        let y = want[1];
        assert!((snake[1] - (y + (2.0 * y).sin().powi(2) / 2.0)).abs() < 1e-6);
    }

    #[test]
    fn the_quick_sine_squared_matches_the_librarys() {
        for i in -200_000..=200_000 {
            let x = i as f32 * 0.05;
            let want = x.sin() * x.sin();
            assert!((sin_squared(x) - want).abs() < 2e-6, "{x}");
        }
    }

    #[test]
    fn the_gathered_conv_matches_candles() -> Result<()> {
        for len in [2000, 40] {
            gathered_conv_matches(len)?;
        }
        Ok(())
    }

    fn gathered_conv_matches(len: usize) -> Result<()> {
        let (inputs, outputs) = (3, 4);
        let x: Vec<f32> = (0..inputs * len).map(|i| (i as f32 * 0.37).sin()).collect();
        let w: Vec<f32> = (0..outputs * inputs * 5)
            .map(|i| (i as f32 * 0.11).cos())
            .collect();
        let xt = Tensor::from_slice(&x, (1, inputs, len), &Device::Cpu)?;
        let wt = Tensor::from_slice(&w, (outputs, inputs, 5), &Device::Cpu)?;
        for (padding, dilation) in [(2, 1), (6, 3), (0, 2)] {
            let want = xt.conv1d(&wt, padding, 1, dilation, 1)?;
            let shape = ConvShape {
                inputs,
                kernel: 5,
                padding,
                dilation,
            };
            let flat = wt.reshape((outputs, inputs * 5))?;
            let (got, got_len) = conv(&x, len, &shape, &flat, Some(&[0.5; 4]))?;
            assert_eq!(got_len, want.dim(2)?);
            let want = (want + 0.5)?.flatten_all()?.to_vec1::<f32>()?;
            let worst = got
                .iter()
                .zip(&want)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(
                worst < 1e-4,
                "padding {padding}, dilation {dilation}: off by {worst}"
            );
        }
        Ok(())
    }

    #[test]
    fn overlap_add_matches_candles_transposed_conv() -> Result<()> {
        let (inputs, outputs, kernel, len) = (3, 2, 6, 50);
        let x: Vec<f32> = (0..inputs * len).map(|i| (i as f32 * 0.21).sin()).collect();
        let w: Vec<f32> = (0..inputs * outputs * kernel)
            .map(|i| (i as f32 * 0.07).cos())
            .collect();
        let xt = Tensor::from_slice(&x, (1, inputs, len), &Device::Cpu)?;
        let wt = Tensor::from_slice(&w, (inputs, outputs, kernel), &Device::Cpu)?;
        for (stride, padding, extra) in [(3, 2, 1), (2, 0, 0), (3, 1, 1)] {
            let want = xt.conv_transpose1d(&wt, padding, extra, stride, 1, 1)?;
            let products = xt
                .squeeze(0)?
                .t()?
                .matmul(&wt.reshape((inputs, outputs * kernel))?)?
                .flatten_all()?
                .to_vec1::<f32>()?;
            let out_len = want.dim(2)?;
            let got = overlap_add(
                &products,
                (outputs, kernel),
                (stride, padding, out_len),
                None,
            );
            let want = want.flatten_all()?.to_vec1::<f32>()?;
            let worst = got
                .iter()
                .zip(&want)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(
                worst < 1e-4,
                "stride {stride}, padding {padding}: off by {worst}"
            );
        }
        Ok(())
    }
}
