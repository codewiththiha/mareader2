//! A build against the reference loaded with that build's own weights.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use any_tts::models::kokoro::config::KokoroConfig;
use candle_core::{Device, Tensor};
use mareader_kokoro as ours;

use crate::{R, Reference, TWIN_AGREES, TWIN_SEED, compare_stages, pearson, published, reference};

/// Builds checked so, by name and file.
const OWN: [(&str, &str); 1] = [("Tiny", "kokoro-v1_0-q8.onnx")];

/// A folded weight as direction and norm; rows rounded to zero stay zero.
fn split(w: &Tensor) -> R<(Tensor, Tensor)> {
    let norm = w.sqr()?.flatten_from(1)?.sum(1)?.sqrt()?;
    let rows: Vec<usize> = std::iter::once(w.dim(0)?)
        .chain(std::iter::repeat_n(1, w.rank().saturating_sub(1)))
        .collect();
    let dead = norm.eq(0f32)?.reshape(rows)?.broadcast_as(w.shape())?;
    Ok((dead.where_cond(&w.ones_like()?, w)?, norm))
}

/// The build's weights as published; folded convolutions regain a norm.
fn as_published(
    full: HashMap<String, Tensor>,
    build: &HashMap<String, Tensor>,
) -> R<HashMap<String, Tensor>> {
    let mut out = HashMap::new();
    for (name, t) in full {
        let folded = |base: &str| build.get(&format!("{base}.weight"));
        let swapped = if let Some(base) = name.strip_suffix(".weight_v") {
            folded(base).map(split).transpose()?.map(|(v, _)| v)
        } else if let Some(base) = name.strip_suffix(".weight_g") {
            match folded(base).map(split).transpose()? {
                Some((_, g)) => Some(g.reshape(t.shape())?),
                None => None,
            }
        } else {
            build.get(&name).cloned()
        };
        out.insert(name, swapped.unwrap_or(t));
    }
    Ok(out)
}

/// Compares each build in `dir` with the reference on its own weights.
pub fn check(
    dir: &Path,
    config: &KokoroConfig,
    voice: &ours::Voice,
    inputs: &[(String, Vec<u32>)],
    (report, failed): (&mut String, &mut Vec<String>),
) -> R<()> {
    writeln!(
        report,
        "## Builds against the reference on their own weights\n"
    )?;
    writeln!(
        report,
        "| build and text | stage | max abs diff | relative | verdict |"
    )?;
    writeln!(report, "|---|---|---|---|---|")?;
    let mut twins = String::new();
    for (name, file) in OWN {
        let path = dir.join(file);
        if !path.exists() {
            failed.push(format!("{name}: {file} missing"));
            continue;
        }
        let weights = ours::read_model(&path)?;
        // The reference runs on a candle of its own.
        let mut theirs = HashMap::new();
        for (key, t) in &weights {
            let values: Vec<f32> = t.flatten_all()?.to_vec1()?;
            theirs.insert(
                key.clone(),
                Tensor::from_vec(values, t.dims(), &Device::Cpu)?,
            );
        }
        let mut ours_config = ours::Config::parse(&std::fs::read(dir.join("config.json"))?)?;
        ours_config.plbert.hidden_act = ours::Activation::Gelu;
        let model = ours::Kokoro::from_tensors(ours_config, weights)?;
        let full = published(&dir.join(ours::WEIGHTS))?;
        let theirs_model = Reference::from_tensors(config, as_published(full, &theirs)?)?;
        drop(theirs);
        for (tag, ids) in inputs {
            let style = voice.style(ids.len())?;
            let style_v: Vec<f32> = style.flatten_all()?.to_vec1()?;
            let trace = model.trace(ids, &style, 1.0)?;
            let theirs = reference(&theirs_model, ids, &style_v, &trace.durations)?;
            compare_stages(&format!("{name} {tag}"), &trace, &theirs, (report, failed))?;
            let twin = model.trace_noise(ids, &style, 1.0, TWIN_SEED, 1.0, true)?;
            let corr = pearson(&twin.audio, &theirs.twin_audio);
            if corr.is_nan() || corr <= TWIN_AGREES {
                failed.push(format!(
                    "{name} {tag}: audio differs from the twin on equal noise"
                ));
            }
            write!(twins, "{name} {tag} {corr:.5}, ")?;
        }
    }
    writeln!(
        report,
        "\nAudio against the twin, noise equal: {}.\n",
        twins.trim_end_matches(", ")
    )?;
    Ok(())
}
