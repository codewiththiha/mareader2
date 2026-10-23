use candle_core::{Device, Tensor};

use crate::duration::spline_inverse;
use crate::encoder::{absolute_to_relative, relative_to_absolute};
use crate::{Scales, Vits};

fn grid(heads: usize, rows: usize, cols: usize) -> (Vec<f32>, Tensor) {
    let v: Vec<f32> = (0..heads * rows * cols).map(|i| i as f32 + 1.0).collect();
    let t = Tensor::from_vec(v.clone(), (heads, rows, cols), &Device::Cpu).unwrap();
    (v, t)
}

#[test]
fn offsets_turn_into_positions_and_back() {
    for len in [1usize, 2, 5] {
        let span = 2 * len - 1;
        let (by_offset, t) = grid(2, len, span);
        let abs = relative_to_absolute(&t).unwrap().to_vec3::<f32>().unwrap();
        for (h, head) in abs.iter().enumerate() {
            for (i, row) in head.iter().enumerate() {
                for (j, &v) in row.iter().enumerate() {
                    let offset = len - 1 + j - i;
                    assert_eq!(v, by_offset[(h * len + i) * span + offset]);
                }
            }
        }
        let (by_pos, t) = grid(2, len, len);
        let rel = absolute_to_relative(&t).unwrap().to_vec3::<f32>().unwrap();
        for (h, head) in rel.iter().enumerate() {
            for (i, row) in head.iter().enumerate() {
                for (k, &v) in row.iter().enumerate() {
                    let j = (i + k).checked_sub(len - 1).filter(|&j| j < len);
                    let want = j.map_or(0.0, |j| by_pos[(h * len + i) * len + j]);
                    assert_eq!(v, want, "len {len} head {h} row {i} offset {k}");
                }
            }
        }
    }
}

#[test]
fn a_flat_spline_is_the_identity_and_tails_pass_through() {
    let edge = ((1.0f64 - 1e-3).exp() - 1.0).ln() as f32;
    let (w, h, d) = (vec![0.0; 10], vec![0.0; 10], vec![edge; 9]);
    for y in [-4.9f32, -1.0, 0.0, 0.3, 2.5, 4.99] {
        assert!((spline_inverse(y, &w, &h, &d) - y).abs() < 1e-4, "{y}");
    }
    let bent: Vec<f32> = (0..10).map(|i| i as f32 * 0.3 - 1.0).collect();
    assert_eq!(spline_inverse(7.0, &bent, &w, &d), 7.0);
    let mut last = f32::NEG_INFINITY;
    for i in -50..=50 {
        let x = spline_inverse(i as f32 * 0.1, &bent, &h, &d);
        assert!(x.is_finite() && x >= last, "monotone at {i}");
        last = x;
    }
}

#[test]
fn files_that_are_not_voices_are_refused() {
    assert!(Vits::from_onnx(b"").is_err());
    assert!(Vits::from_onnx(b"not a model at all").is_err());
    let bytes = crate::onnx::tests::model(&[(
        5,
        crate::onnx::tests::tensor("enc_p.emb.weight", &[4, 2], &[0.0; 8]),
    )]);
    assert!(Vits::from_onnx(&bytes).is_err());
}

#[test]
fn scales_out_of_range_are_input_errors() {
    let bad = [
        Scales {
            noise: f32::NAN,
            length: 1.0,
            noise_w: 0.8,
        },
        Scales {
            noise: 0.6,
            length: 0.0,
            noise_w: 0.8,
        },
        Scales {
            noise: 0.6,
            length: 1.0,
            noise_w: -1.0,
        },
    ];
    for s in bad {
        assert!(s.check().is_err(), "{s:?}");
    }
}
