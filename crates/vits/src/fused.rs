//! Frame-rate steps fused into single passes over plain buffers.

use crate::dense::{Signal, each_row};
use crate::{Error, Result, simd};

/// tanh as a ratio of odd and even polynomials, within 4e-7 everywhere.
#[inline(always)]
pub(crate) fn tanh(x: f32) -> f32 {
    const EDGE: f32 = 7.905_311;
    const P: [f32; 7] = [
        4.893_524_6e-3,
        6.372_619_5e-4,
        1.485_722_35e-5,
        5.122_297_3e-8,
        -8.604_672e-11,
        2.000_188e-13,
        -2.760_768_4e-16,
    ];
    const Q: [f32; 4] = [4.893_525e-3, 2.268_434_7e-3, 1.185_347_1e-4, 1.198_258_4e-6];
    let x = x.clamp(-EDGE, EDGE);
    let x2 = x * x;
    let mut p = P[6];
    for c in P[..6].iter().rev() {
        p = p * x2 + c;
    }
    let mut q = Q[3];
    for c in Q[..3].iter().rev() {
        q = q * x2 + c;
    }
    x * p / q
}

/// The error function as a ratio of polynomials, within 5e-7 everywhere.
#[inline(always)]
fn erf(x: f32) -> f32 {
    const P: [f32; 7] = [
        -2.726_142_4e-10,
        2.770_681_5e-8,
        -2.101_024e-6,
        -5.692_506_2e-5,
        -7.349_906_4e-4,
        -2.954_6e-3,
        -1.609_603_3e-2,
    ];
    const Q: [f32; 5] = [
        -1.456_607_15e-5,
        -2.133_740_5e-4,
        -1.682_827e-3,
        -7.373_329_3e-3,
        -1.426_473_9e-2,
    ];
    let x = x.clamp(-4.0, 4.0);
    let x2 = x * x;
    let p = P.iter().skip(1).fold(P[0], |p, c| p * x2 + c);
    let q = Q.iter().skip(1).fold(Q[0], |q, c| q * x2 + c);
    x * p / q
}

/// One row through exact-form GELU.
#[inline(always)]
fn gelu_row((row,): (&mut [f32],)) {
    for v in row {
        *v = (erf(*v * std::f32::consts::FRAC_1_SQRT_2) + 1.0) * 0.5 * *v;
    }
}

/// The logistic curve, through [`tanh`].
#[inline(always)]
fn sigmoid(x: f32) -> f32 {
    0.5 + 0.5 * tanh(0.5 * x)
}

/// One gated row: `tanh(a + bias_a) · sigmoid(b + bias_b)`.
#[inline(always)]
fn gate_row((dst, a, b, (bias_a, bias_b)): (&mut [f32], &[f32], &[f32], (f32, f32))) {
    for ((d, &a), &b) in dst.iter_mut().zip(a).zip(b) {
        *d = tanh(a + bias_a) * sigmoid(b + bias_b);
    }
}

/// WaveNet's gate: first half's tanh times second half's sigmoid, plus `bias`.
pub(crate) fn gated(x: &Signal, bias: Option<&[f32]>) -> Result<Signal> {
    let hidden = x.channels / 2;
    if hidden * 2 != x.channels || bias.is_some_and(|b| b.len() != x.channels) {
        return Err(Error::Compute("a gate needs paired channels".into()));
    }
    let len = x.len;
    let mut data = vec![0.0; hidden * len];
    each_row(&mut data, len, |c, dst| {
        let shift = |c: usize| bias.and_then(|b| b.get(c)).copied().unwrap_or(0.0);
        let shifts = (shift(c), shift(hidden + c));
        simd::pick(gate_row, (dst, x.row(c), x.row(hidden + c), shifts));
    });
    Ok(Signal {
        data,
        channels: hidden,
        len,
    })
}

/// GELU in its error-function form, as candle's `gelu_erf` has it.
pub(crate) fn gelu(x: &mut Signal) {
    let len = x.len;
    each_row(&mut x.data, len, |_, row| simd::pick(gelu_row, (row,)));
}

/// Each step normalised across channels, then scaled and shifted per channel.
pub(crate) fn channel_norm(x: &mut Signal, gamma: &[f32], beta: &[f32], eps: f32) {
    let (channels, len) = (x.channels, x.len);
    let rows = || x.data.chunks(len.max(1));
    let mut mean = vec![0.0f32; len];
    for row in rows() {
        mean.iter_mut().zip(row).for_each(|(m, &v)| *m += v);
    }
    let n = channels.max(1) as f32;
    mean.iter_mut().for_each(|m| *m /= n);
    let mut var = vec![0.0f32; len];
    for row in rows() {
        for ((s, &v), &m) in var.iter_mut().zip(row).zip(&mean) {
            *s += (v - m) * (v - m);
        }
    }
    let inv: Vec<f32> = var.iter().map(|s| 1.0 / (s / n + eps).sqrt()).collect();
    let rows = x.data.chunks_mut(len.max(1));
    for ((row, &g), &b) in rows.zip(gamma).zip(beta) {
        for ((v, &m), &i) in row.iter_mut().zip(&mean).zip(&inv) {
            *v = (*v - m) * i * g + b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tanh_stays_close_to_std() {
        let worst = (-120_000..=120_000)
            .map(|i| i as f32 / 10_000.0)
            .map(|x| (tanh(x) - x.tanh()).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-6, "{worst}");
        assert_eq!(tanh(f32::INFINITY), tanh(7.905_311));
    }

    #[test]
    fn erf_stays_close_to_candle() {
        let worst = (-60_000..=60_000)
            .map(|i| i as f32 / 10_000.0)
            .map(|x| (erf(x) - candle_core::cpu::erf::erf_f32(x)).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-6, "{worst}");
    }

    #[test]
    fn gate_multiplies_the_halves() {
        let x = Signal {
            data: vec![0.5, -1.0, 2.0, 0.0],
            channels: 2,
            len: 2,
        };
        let y = gated(&x, Some(&[0.0, 1.0])).unwrap();
        let want = |a: f32, b: f32| a.tanh() / (1.0 + (-b).exp());
        let expect = [want(0.5, 3.0), want(-1.0, 1.0)];
        for (g, e) in y.data.iter().zip(expect) {
            assert!((g - e).abs() < 1e-6, "{g} {e}");
        }
        assert!(gated(&Signal { channels: 3, ..x }, None).is_err());
    }

    #[test]
    fn norm_matches_by_hand() {
        let mut x = Signal {
            data: vec![1.0, 2.0, 3.0, 6.0],
            channels: 2,
            len: 2,
        };
        channel_norm(&mut x, &[1.0, 2.0], &[0.0, 1.0], 0.0);
        let want = [-1.0, -1.0, 3.0, 3.0];
        for (g, e) in x.data.iter().zip(want) {
            assert!((g - e).abs() < 1e-5, "{g} {e}");
        }
    }
}
