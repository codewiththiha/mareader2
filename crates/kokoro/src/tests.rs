//! A tiny model of constant weights, run end to end.

use candle_core::{DType, Device, Shape, Tensor};
use candle_nn::{Init, VarBuilder, var_builder::SimpleBackend};

use super::*;

/// Every weight asked for, filled with one small value.
struct Constant;

impl SimpleBackend for Constant {
    fn get(
        &self,
        s: Shape,
        _: &str,
        _: Init,
        dtype: DType,
        dev: &Device,
    ) -> candle_core::Result<Tensor> {
        Tensor::full(0.01f32, s, dev)?.to_dtype(dtype)
    }

    fn get_unchecked(&self, name: &str, _: DType, _: &Device) -> candle_core::Result<Tensor> {
        candle_core::bail!("{name} has no known shape")
    }

    fn contains_tensor(&self, _: &str) -> bool {
        true
    }
}

fn tiny() -> Result<Kokoro> {
    let config = Config {
        hidden_dim: 16,
        style_dim: 8,
        n_token: 12,
        n_layer: 1,
        max_dur: 4,
        plbert: Plbert {
            vocab_size: 12,
            hidden_size: 8,
            num_attention_heads: 2,
            num_hidden_layers: 2,
            intermediate_size: 16,
            max_position_embeddings: 8,
            embedding_size: 4,
            type_vocab_size: 2,
        },
        istftnet: Istft {
            upsample_rates: vec![2, 2],
            upsample_kernel_sizes: vec![4, 4],
            resblock_kernel_sizes: vec![3],
            resblock_dilation_sizes: vec![vec![1]],
            ..Istft::default()
        },
        vocab: [("a", 1), ("b", 2), (" ", 3), (".", 4)]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        ..Config::default()
    };
    let vb = VarBuilder::from_backend(Box::new(Constant), DType::F32, Device::Cpu);
    Ok(Kokoro::build(config, &vb)?)
}

fn voice() -> Result<Voice> {
    Voice::from_tensor(Tensor::full(0.01f32, (4, 1, 16), &Device::Cpu)?)
}

#[test]
fn a_pass_gives_two_pitch_points_per_frame_and_matching_audio() -> Result<()> {
    let model = tiny()?;
    let ids = [0, 1, 2, 1, 0];
    let trace = model.trace(&ids, &voice()?.style(ids.len())?, 1.0)?;
    // Each phoneme lasts sigmoid(0)-ish times 4 frames, so 2.
    assert_eq!(trace.durations, [2; 5]);
    let frames = 10;
    assert_eq!(trace.pitch.dims(), [1, 2 * frames]);
    assert_eq!(trace.energy.dims(), [1, 2 * frames]);
    assert_eq!(trace.bert.dims(), [1, 5, 8]);
    assert_eq!(trace.text.dims(), [1, 16, 5]);
    // Two pitch points per frame, twenty samples per point.
    assert_eq!(trace.audio.len(), 2 * frames * 20);
    assert!(trace.audio.iter().all(|s| s.is_finite()));
    Ok(())
}

#[test]
fn faster_speech_is_shorter_and_equal_input_sounds_equal() -> Result<()> {
    let model = tiny()?;
    let v = voice()?;
    let chunk = Chunk {
        ids: vec![1, 2, 1],
        pause_ms: 0,
    };
    let normal = model.render(&chunk, &v, 1.0)?;
    assert_eq!(normal, model.render(&chunk, &v, 1.0)?);
    assert!(model.render(&chunk, &v, 2.0)?.len() < normal.len());
    assert!(matches!(
        model.render(&chunk, &v, 9.0),
        Err(Error::Input(_))
    ));
    Ok(())
}

#[test]
fn speaking_stops_when_the_sink_says_so() -> Result<()> {
    let model = tiny()?;
    let v = voice()?;
    // Six tokens fit per chunk, so this makes three chunks.
    let text = model.normalise("ab. ab. ab. ab ✓");
    assert_eq!(model.chunks(&text).len(), 3);
    let mut heard = 0;
    model.speak(&text, &v, 1.0, |_| {
        heard += 1;
        heard < 2
    })?;
    assert_eq!(heard, 2);
    assert!(matches!(
        model.speak("✓", &v, 1.0, |_| true),
        Err(Error::Input(_))
    ));
    Ok(())
}

#[test]
fn a_voice_picks_its_row_by_length() -> Result<()> {
    let rows: Vec<f32> = (0..4).flat_map(|r| [r as f32; 16]).collect();
    let v = Voice::from_tensor(Tensor::from_vec(rows, (4, 16), &Device::Cpu)?)?;
    let row = |n| -> Result<f32> {
        let style = v.style(n)?.flatten_all()?.to_vec1::<f32>()?;
        Ok(style.first().copied().unwrap_or(-1.0))
    };
    assert_eq!(row(2)?, 0.0);
    assert_eq!(row(5)?, 2.0);
    assert_eq!(row(99)?, 3.0);
    assert!(Voice::from_tensor(Tensor::zeros((2, 2, 2, 2), DType::F32, &Device::Cpu)?).is_err());
    Ok(())
}
