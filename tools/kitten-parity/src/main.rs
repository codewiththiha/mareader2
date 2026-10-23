//! Checks mareader-kokoro on Kitten's ONNX builds against onnxruntime.

//! Usage: prepare <g2p> <out>; reference.py <builds> <out>;
//! compare <builds> <out>. Builds hold model.onnx and voices.npz.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use candle_core::{DType, Tensor};
use mareader_g2p::{Dialect, G2p};
use mareader_kokoro::{Kokoro, SAMPLE_RATE, kitten};
use mareader_vits::{Style, espeak_like};
use serde::{Deserialize, Serialize};

type R<T> = Result<T, Box<dyn std::error::Error>>;

const SENTENCES: [&str; 10] = [
    "Hello world.",
    "The quick brown fox jumps over the lazy dog, then naps in the warm afternoon sun.",
    "Reading aloud should feel natural: steady pacing, clear words, and pauses where \
     a person would breathe.",
    "Does it sound right to you?",
    "I hope it does, because this sentence is long enough to need a fair number of \
     phonemes, and the voice must keep its rhythm all the way through to the very end.",
    "On the third of May, nineteen ninety-nine, she paid forty-two dollars for a ticket.",
    "\"Wait!\" he shouted. \"Don't open that door.\"",
    "Thoroughly thoughtful thinkers rarely rush their reasoning.",
    "Our neighbour's colour television was already there when the water heater broke.",
    "Yes.",
];

const VOICES: [&str; 8] = [
    "expr-voice-2-m",
    "expr-voice-2-f",
    "expr-voice-3-m",
    "expr-voice-3-f",
    "expr-voice-4-m",
    "expr-voice-4-f",
    "expr-voice-5-m",
    "expr-voice-5-f",
];

/// Waveform correlation each full-precision sentence must reach.
const SAME_WAVE: f64 = 0.99;
/// Share of sentences whose phoneme lengths must all match.
const SAME_LENGTH: f64 = 0.8;
/// How far total length and loudness may drift, for every build.
const LENGTH_DRIFT: f64 = 0.1;
const LOUDNESS_DRIFT: f64 = 0.25;
/// Phoneme error against espeak, Kitten's own front end.
const PHONEME_ERROR: f64 = 0.25;

#[derive(Serialize, Deserialize)]
struct Case {
    index: usize,
    text: String,
    voice: String,
    misaki: String,
    phonemes: String,
    ids: Vec<u32>,
}

#[derive(Deserialize)]
struct Timing {
    seconds: f64,
    samples: u64,
}

#[derive(Deserialize)]
struct Reference {
    timing: HashMap<String, Timing>,
    espeak: HashMap<String, String>,
}

fn prepare(g2p_dir: &Path, out: &Path) -> R<()> {
    let g2p = G2p::load(g2p_dir, Dialect::American)?;
    let mut cases = Vec::new();
    for (index, text) in SENTENCES.iter().enumerate() {
        let misaki = g2p.phonemize(text);
        let phonemes = espeak_like(&misaki, Style::American);
        cases.push(Case {
            index,
            text: (*text).to_owned(),
            voice: VOICES[index % VOICES.len()].to_owned(),
            ids: kitten::ids(&phonemes),
            misaki,
            phonemes,
        });
    }
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join("cases.json"), serde_json::to_vec_pretty(&cases)?)?;
    println!("prepared {} sentences", cases.len());
    Ok(())
}

fn builds(dir: &Path) -> R<Vec<String>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.join("model.onnx").exists() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            out.push(name.to_owned());
        }
    }
    out.sort();
    Ok(out)
}

/// The nano sentence whose generator steps are compared one by one.
const STAGE_CASE: usize = 1;

/// Rows comparing our probes with the reference's tensors at each step.
fn stages(folder: &Path, probes: &[(String, Tensor)]) -> R<String> {
    let Ok(shapes) = std::fs::read(folder.join("shapes.json")) else {
        return Ok("No reference steps were written.\n".into());
    };
    let shapes: HashMap<String, Vec<usize>> = serde_json::from_slice(&shapes)?;
    let mut rows = String::new();
    for (name, ours) in probes {
        let Some(shape) = shapes.get(name) else {
            continue;
        };
        let theirs = read_f32(&folder.join(format!("{name}.f32")))?;
        let ours: Vec<f32> = ours.flatten_all()?.to_dtype(DType::F32)?.to_vec1()?;
        let diff = ours
            .iter()
            .zip(&theirs)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        let scale = theirs.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let _ = writeln!(
            rows,
            "| {name} | {} / {:?} | {:.6} | {diff:.2e} | {scale:.2e} |",
            ours.len(),
            shape,
            correlation(&ours, &theirs),
        );
    }
    Ok(rows)
}

fn read_f32(path: &Path) -> R<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    let (chunks, _) = bytes.as_chunks::<4>();
    Ok(chunks.iter().map(|&c| f32::from_le_bytes(c)).collect())
}

fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mean = |v: &[f32]| v[..n].iter().map(|&x| f64::from(x)).sum::<f64>() / n as f64;
    let (ma, mb) = (mean(a), mean(b));
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for (&x, &y) in a[..n].iter().zip(&b[..n]) {
        let (x, y) = (f64::from(x) - ma, f64::from(y) - mb);
        num += x * y;
        da += x * x;
        db += y * y;
    }
    num / (da.sqrt() * db.sqrt()).max(1e-12)
}

/// Loudness per 10 ms, so timing near-misses still compare.
fn envelope(v: &[f32]) -> Vec<f32> {
    v.chunks(240)
        .map(|c| (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).sqrt())
        .collect()
}

fn energy(v: &[f32]) -> f64 {
    v.iter().map(|&x| f64::from(x) * f64::from(x)).sum()
}

fn edits(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

fn write_wav(path: &Path, samples: &[f32], rate: u32) -> R<()> {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|&s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
        .collect();
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    std::fs::write(path, out)?;
    Ok(())
}

fn trimmed(v: &[f32]) -> &[f32] {
    &v[..v.len().saturating_sub(kitten::TRIM)]
}

#[derive(Default)]
struct Tally {
    sentences: usize,
    same_length: usize,
    worst_corr: f64,
    worst_envelope: f64,
    worst_diff: f32,
    durations_off: usize,
    samples: usize,
    ref_samples: usize,
    ours_energy: f64,
    ref_energy: f64,
    ours_seconds: f64,
}

fn compare(dir: &Path, out: &Path) -> R<bool> {
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(out.join("cases.json"))?)?;
    let reference: Reference =
        serde_json::from_slice(&std::fs::read(out.join("ref").join("reference.json"))?)?;
    let wav_dir = out.join("wav");
    std::fs::create_dir_all(&wav_dir)?;
    let mut md = String::from("# Kitten builds: mareader-kokoro against onnxruntime\n\n");
    let mut all_ok = true;
    let (mut rows, mut speed, mut sizes) = (String::new(), String::new(), String::new());
    // Every build holds at least the full-precision nano's weights.
    let wanted = Kokoro::onnx_names(&dir.join("nano").join("model.onnx"))?;
    let mut held = String::new();
    let mut stage_rows = String::new();
    for build in builds(dir)? {
        let folder = dir.join(&build);
        let names = Kokoro::onnx_names(&folder.join("model.onnx"))?;
        let (merged, missing): (Vec<&String>, Vec<&String>) = (wanted.iter())
            .filter(|n| !names.contains(n))
            .partition(|n| n.ends_with(".norm.weight") || n.ends_with(".norm.bias"));
        all_ok &= missing.is_empty();
        let _ = writeln!(
            held,
            "| {build} | {} | {} | {:?} | {} |",
            names.len(),
            missing.len(),
            missing.iter().take(4).collect::<Vec<_>>(),
            merged.len(),
        );
        let start = Instant::now();
        let model = Kokoro::from_onnx(&folder.join("model.onnx"))?;
        let voices: HashMap<String, _> = kitten::read_voices(&folder.join("voices.npz"))?
            .into_iter()
            .collect();
        let load = start.elapsed().as_secs_f64();
        let c = model.config();
        let _ = writeln!(
            sizes,
            "| {build} | {} | {} | {} | {} | {} | {:?} |",
            c.hidden_dim,
            c.n_layer,
            c.decoder_dim,
            c.istftnet.upsample_initial_channel,
            c.plbert.num_hidden_layers,
            c.istftnet.resblock_kernel_sizes,
        );
        let full = build == "nano";
        let mut t = Tally {
            worst_corr: 1.0,
            worst_envelope: 1.0,
            ..Tally::default()
        };
        for case in &cases {
            let stem = format!("{build}.{}", case.index);
            let ref_audio = read_f32(&out.join("ref").join(format!("{stem}.f32")))?;
            let ref_dur: Vec<usize> = serde_json::from_slice(&std::fs::read(
                out.join("ref").join(format!("{stem}.dur.json")),
            )?)?;
            let voice = voices
                .get(&case.voice)
                .ok_or(format!("{build}: no {}", case.voice))?;
            let style = kitten::style(voice, &case.text)?;
            if full && case.index == STAGE_CASE {
                let probed = model.trace_noise(&case.ids, &style, 1.0, 0, 1.0, true)?;
                stage_rows = stages(&out.join("ref").join("stages"), &probed.probes)?;
            }
            let start = Instant::now();
            let trace = model.trace_noise(&case.ids, &style, 1.0, 0, 1.0, false)?;
            t.ours_seconds += start.elapsed().as_secs_f64();
            let audio = trace.audio;
            t.sentences += 1;
            if trace.durations == ref_dur && audio.len() == ref_audio.len() {
                t.same_length += 1;
                t.worst_corr = t.worst_corr.min(correlation(&audio, &ref_audio));
                let diff = audio
                    .iter()
                    .zip(&ref_audio)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0f32, f32::max);
                t.worst_diff = t.worst_diff.max(diff);
            } else {
                let off = trace.durations.iter().zip(&ref_dur).filter(|(a, b)| a != b);
                t.durations_off += off.count() + trace.durations.len().abs_diff(ref_dur.len());
            }
            let env = correlation(&envelope(&audio), &envelope(&ref_audio));
            t.worst_envelope = t.worst_envelope.min(env);
            t.samples += audio.len();
            t.ref_samples += ref_audio.len();
            t.ours_energy += energy(&audio);
            t.ref_energy += energy(&ref_audio);
            if case.index < 3 {
                write_wav(
                    &wav_dir.join(format!("{stem}.ours.wav")),
                    trimmed(&audio),
                    SAMPLE_RATE,
                )?;
                write_wav(
                    &wav_dir.join(format!("{stem}.ref.wav")),
                    trimmed(&ref_audio),
                    SAMPLE_RATE,
                )?;
            }
        }
        let length_ratio = t.samples as f64 / t.ref_samples.max(1) as f64;
        let loud_ratio = (t.ours_energy / t.samples.max(1) as f64).sqrt()
            / (t.ref_energy / t.ref_samples.max(1) as f64)
                .sqrt()
                .max(1e-12);
        let same_share = t.same_length as f64 / t.sentences.max(1) as f64;
        // Quantised builds round each layer's input as they run; ours don't.
        let exact = !full || (same_share >= SAME_LENGTH && t.worst_corr >= SAME_WAVE);
        let ok = t.sentences > 0
            && exact
            && (length_ratio - 1.0).abs() <= LENGTH_DRIFT
            && (loud_ratio - 1.0).abs() <= LOUDNESS_DRIFT;
        all_ok &= ok;
        let _ = writeln!(
            rows,
            "| {build} | {}/{} | {} | {:.6} | {:.1e} | {:.4} | {:.3} | {:.3} | {} |",
            t.same_length,
            t.sentences,
            t.durations_off,
            t.worst_corr,
            t.worst_diff,
            t.worst_envelope,
            length_ratio,
            loud_ratio,
            if ok { "yes" } else { "**no**" }
        );
        let audio_s = t.samples as f64 / f64::from(SAMPLE_RATE);
        let (ref_s, ref_audio) = reference
            .timing
            .get(&build)
            .map(|r| (r.seconds, r.samples as f64 / f64::from(SAMPLE_RATE)))
            .unwrap_or((0.0, 0.0));
        let _ = writeln!(
            speed,
            "| {build} | {load:.2} s | {:.3} | {:.3} |",
            t.ours_seconds / audio_s.max(1e-9),
            ref_s / ref_audio.max(1e-9),
        );
    }
    let (mut said, mut total_edits, mut total_len) = (String::new(), 0, 0);
    for case in &cases {
        let espeak = reference
            .espeak
            .get(&case.index.to_string())
            .cloned()
            .unwrap_or_default();
        let (ours, theirs) = (kitten::spaced(&case.phonemes), kitten::spaced(&espeak));
        total_edits += edits(&ours, &theirs);
        total_len += theirs.chars().count();
        if case.index < 3 {
            let _ = writeln!(said, "| {} | `{ours}` | `{theirs}` |", case.text);
        }
    }
    let per = total_edits as f64 / total_len.max(1) as f64;
    all_ok &= per <= PHONEME_ERROR;
    let _ = writeln!(
        md,
        "Noise seeded alike on both sides, the source's phases fixed alike; speed 1.\n\n\
         | Build | Same lengths | Lengths off | Worst corr | Max diff | Worst envelope | Length | RMS | Pass |\n\
         |---|---|---|---|---|---|---|---|---|\n{rows}\n\
         Gates: nano (full precision) corr >= {SAME_WAVE} with lengths equal in >= {:.0}%; \
         every build length within {:.0}% and RMS within {:.0}%. \
         The 8-bit builds round activations in onnxruntime, so their waves differ.\n\n\
         ## Generator steps (nano, sentence {STAGE_CASE})\n\n\
         | Step | Values / reference shape | Corr | Max diff | Reference peak |\n\
         |---|---|---|---|---|\n{stage_rows}\n\
         ## Weights by name\n\n\
         Every build must hold each weight the full-precision nano does. An export \
         that merged identical norms drops their nodes; those load as 1 and 0, \
         which Mini's norms all are.\n\n\
         | Build | Names | Missing | First missing | Merged norms |\n\
         |---|---|---|---|---|\n{held}\n\
         ## Sizes read from the files\n\n\
         | Build | Hidden | Layers | Decoder | Generator | ALBERT runs | Block kernels |\n\
         |---|---|---|---|---|---|---|\n{sizes}\n\
         ## Speed (seconds of compute per second of speech)\n\n\
         | Build | Load | Ours | onnxruntime |\n|---|---|---|---|\n{speed}\n\
         ## Phonemes against espeak\n\n\
         Error {:.1}% (gate {:.0}%).\n\n| Text | Ours | espeak |\n|---|---|---|\n{said}",
        SAME_LENGTH * 100.0,
        LENGTH_DRIFT * 100.0,
        LOUDNESS_DRIFT * 100.0,
        per * 100.0,
        PHONEME_ERROR * 100.0,
    );
    md.push_str(if all_ok {
        "\n**All checks pass.**\n"
    } else {
        "\n**Some checks fail.**\n"
    });
    std::fs::write(out.join("summary.md"), &md)?;
    println!("{md}");
    Ok(all_ok)
}

fn main() -> R<()> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| -> R<PathBuf> {
        Ok(PathBuf::from(
            args.get(i).ok_or("see the usage at the top of main.rs")?,
        ))
    };
    match args.get(1).map(String::as_str) {
        Some("prepare") => prepare(&arg(2)?, &arg(3)?),
        Some("compare") => {
            if compare(&arg(2)?, &arg(3)?)? {
                Ok(())
            } else {
                Err("some checks fail".into())
            }
        }
        _ => Err("usage: kitten-parity prepare|compare …".into()),
    }
}
