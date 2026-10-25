//! Checks mareader-phonon against Gradium's own runtime, `ptts`.

//! Usage: prepare <model> <out>; reference.py <model> <voices> <out>;
//! compare <model> <voices> <out>.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use mareader_phonon::{Noise, Phonon, SAMPLE_RATE};
use serde::{Deserialize, Serialize};

type R<T> = Result<T, Box<dyn std::error::Error>>;

const SENTENCES: [&str; 10] = [
    "Hello world.",
    "The quick brown fox jumps over the lazy dog, then naps in the warm afternoon sun.",
    "Reading aloud should feel natural: steady pacing, clear words, and pauses where \
     a person would breathe.",
    "does it sound right to you",
    "I hope it does, because this sentence is long enough to need a fair number of \
     tokens, and the voice must keep its rhythm all the way through to the very end.",
    "\"Wait!\" he shouted. \"Don't open that door.\" Then the hallway went quiet, \
     and nobody moved. A clock ticked somewhere upstairs. She counted the seconds, \
     one after another, until her breathing slowed. Finally, the door creaked open, \
     and a small grey cat padded in, looking very pleased with itself.",
    "Thoroughly thoughtful thinkers rarely rush their reasoning.",
    "Our neighbour's colour television was already there when the water heater broke.",
    "Café owners in Zürich serve crème brûlée — naïvely, perhaps.",
    "Yes.",
];

/// Voices by file stem; `default` is the checkpoint's own.
const VOICES: [&str; 5] = [
    "default",
    "LMDIYE0Oa377Qh6Z",
    "6PWnV0Nq4wu7RVBT",
    "NbpkqMVS3CJeq2j8",
    "X6NceqF285Gr04gb",
];

/// Extra strings whose tokens must match exactly.
const TOKEN_TESTS: [&str; 8] = [
    "Hello world.",
    "a  b",
    ".!...?",
    " lead",
    "x<0x41>y",
    "Ünïcödé façade, naïve coöperation… “quoted” — ok?",
    "日本語 and emoji 🙂 too",
    "Mr. O'Neill's well-known 'twas-it?",
];

/// Builds: the name, its weights file, and the reference's quantisation.
const BUILDS: [(&str, &str, &str); 2] = [
    ("f32", "model.safetensors", "f32"),
    ("q8", "model.q8.gguf", "q8_0"),
];

/// Waveform correlation every full-precision case must reach.
const SAME_WAVE: f64 = 0.999;
/// The 8-bit build's median envelope correlation; its waves can't match.
const Q8_ENVELOPE: f64 = 0.95;
/// How far the 8-bit build's total length may drift.
const LENGTH_DRIFT: f64 = 0.05;
/// Largest latent difference from the numpy model.
const LATENT_DIFF: f32 = 1e-3;
/// Cases whose first run's latents are compared with numpy's.
const STAGE_CASES: [usize; 2] = [1, 3];

#[derive(Serialize, Deserialize)]
struct Planned {
    text: String,
    tokens: Vec<u32>,
    frames_after_eos: usize,
    frame_budget: usize,
}

#[derive(Serialize, Deserialize)]
struct Case {
    index: usize,
    text: String,
    voice: String,
    chunks: Vec<Planned>,
}

#[derive(Serialize, Deserialize)]
struct Cases {
    cases: Vec<Case>,
    /// Each string tokenized as is: the planner's inputs and the extras.
    tokens: Vec<(String, Vec<u32>)>,
}

#[derive(Deserialize)]
struct Timing {
    seconds: f64,
    samples: u64,
}

#[derive(Deserialize)]
struct Reference {
    timing: HashMap<String, Timing>,
    /// The reference tokenizer's ids for [`Cases::tokens`], in order.
    tokens: Vec<Vec<u32>>,
    /// Its decode of those ids.
    decoded: Vec<String>,
    load: HashMap<String, f64>,
}

fn prepare(model: &Path, out: &Path) -> R<()> {
    let phonon = Phonon::load(
        &model.join("config.json"),
        &model.join("model.q8.gguf"),
        &model.join("tokenizer.json"),
    )?;
    let tok = phonon.tokenizer();
    let mut cases = Vec::new();
    let mut tokens = Vec::new();
    for (index, text) in SENTENCES.iter().enumerate() {
        let chunks: Vec<Planned> = (phonon.chunks(text).into_iter())
            .map(|c| Planned {
                text: c.text,
                tokens: c.tokens,
                frames_after_eos: c.frames_after_eos,
                frame_budget: c.frame_budget,
            })
            .collect();
        for c in &chunks {
            let prepared = mareader_phonon::prepare(&c.text).0;
            tokens.push((prepared.clone(), tok.encode(&prepared)));
        }
        cases.push(Case {
            index,
            text: (*text).to_owned(),
            voice: VOICES[index % VOICES.len()].to_owned(),
            chunks,
        });
    }
    for t in TOKEN_TESTS.iter().chain(&SENTENCES) {
        tokens.push(((*t).to_owned(), tok.encode(t)));
    }
    std::fs::create_dir_all(out)?;
    let all = Cases { cases, tokens };
    std::fs::write(out.join("cases.json"), serde_json::to_vec_pretty(&all)?)?;
    println!(
        "prepared {} sentences, {} token tests",
        SENTENCES.len(),
        all.tokens.len()
    );
    Ok(())
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

/// The median, NaN last.
fn middle(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(sorted.len() / 2).copied().unwrap_or(0.0)
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, |m, d| {
            if d.is_nan() || m.is_nan() {
                f32::NAN
            } else {
                m.max(d)
            }
        })
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

fn voice_path(model: &Path, voices: &Path, name: &str) -> PathBuf {
    match name {
        "default" => model.join("default-voice.safetensors"),
        id => voices.join(format!("{id}.safetensors")),
    }
}

/// Our tokens against the reference's, as markdown, and whether all match.
fn tokens(cases: &Cases, reference: &Reference, phonon: &Phonon) -> (String, bool) {
    let mut rows = String::new();
    let mut same = 0;
    for (i, (text, ours)) in cases.tokens.iter().enumerate() {
        let theirs = reference.tokens.get(i).cloned().unwrap_or_default();
        let decoded = phonon.tokenizer().decode(&theirs);
        let their_text = reference.decoded.get(i).cloned().unwrap_or_default();
        let ok = *ours == theirs && decoded == their_text;
        same += usize::from(ok);
        if !ok {
            let _ = writeln!(
                rows,
                "| `{text}` | {ours:?} | {theirs:?} | `{decoded}` | `{their_text}` |"
            );
        }
    }
    let all = same == cases.tokens.len();
    let mut md = format!(
        "{same}/{} strings encode and decode alike.\n\n",
        cases.tokens.len()
    );
    if !all {
        md.push_str(
            "| Text | Ours | Reference | Our decode | Its decode |\n|---|---|---|---|---|\n",
        );
        md.push_str(&rows);
    }
    (md, all)
}

#[derive(Default)]
struct Tally {
    cases: usize,
    same_length: usize,
    corrs: Vec<f64>,
    envelopes: Vec<f64>,
    /// The reference's own 8-bit wave against its f32 one.
    drift: Vec<f64>,
    worst_envelope: f64,
    worst_diff: f32,
    samples: usize,
    ref_samples: usize,
    lm_seconds: f64,
    decode_seconds: f64,
}

fn compare(model: &Path, voices: &Path, out: &Path) -> R<bool> {
    let cases: Cases = serde_json::from_slice(&std::fs::read(out.join("cases.json"))?)?;
    let reference: Reference =
        serde_json::from_slice(&std::fs::read(out.join("ref").join("reference.json"))?)?;
    let wav_dir = out.join("wav");
    std::fs::create_dir_all(&wav_dir)?;
    let mut md = String::from("# Phonon: mareader-phonon against ptts\n\n");
    let mut all_ok = true;
    let (mut rows, mut speed, mut per_case, mut stage_rows) =
        (String::new(), String::new(), String::new(), String::new());
    let mut token_md = String::new();
    for (build, file, quant) in BUILDS {
        let start = Instant::now();
        let phonon = Phonon::load(
            &model.join("config.json"),
            &model.join(file),
            &model.join("tokenizer.json"),
        )?;
        let mut primed = HashMap::new();
        for v in VOICES {
            primed.insert(v, phonon.voice(&voice_path(model, voices, v))?);
        }
        let load = start.elapsed().as_secs_f64();
        if build == "f32" {
            let (section, ok) = tokens(&cases, &reference, &phonon);
            token_md = section;
            all_ok &= ok;
        }
        let mut t = Tally {
            worst_envelope: 1.0,
            ..Tally::default()
        };
        for case in &cases.cases {
            let stem = format!("{build}.{}", case.index);
            let ref_audio = read_f32(&out.join("ref").join(format!("{stem}.f32")))?;
            let voice = primed.get(case.voice.as_str()).ok_or("no voice")?;
            let mut audio = Vec::new();
            for (i, planned) in phonon.chunks(&case.text).iter().enumerate() {
                let mut noise = Noise::new(0.0, 0);
                let start = Instant::now();
                let run = phonon.run(voice, planned, &mut noise)?;
                t.lm_seconds += start.elapsed().as_secs_f64();
                let start = Instant::now();
                audio.extend(phonon.decode(&run.latents)?);
                t.decode_seconds += start.elapsed().as_secs_f64();
                if build == "f32" && i == 0 && STAGE_CASES.contains(&case.index) {
                    let numpy =
                        read_f32(&out.join("ref").join(format!("latents.{}.f32", case.index)))?;
                    let ours: Vec<f32> = run.latents.concat();
                    let diff = max_diff(&ours, &numpy);
                    let ok = ours.len() == numpy.len() && diff <= LATENT_DIFF;
                    all_ok &= ok;
                    let _ = writeln!(
                        stage_rows,
                        "| {} | {} / {} | {:.7} | {diff:.2e} | {:?} | {} |",
                        case.index,
                        ours.len() / 32,
                        numpy.len() / 32,
                        correlation(&ours, &numpy),
                        run.ended,
                        if ok { "yes" } else { "**no**" },
                    );
                }
            }
            t.cases += 1;
            let corr = correlation(&audio, &ref_audio);
            let env = correlation(&envelope(&audio), &envelope(&ref_audio));
            t.worst_envelope = t.worst_envelope.min(env);
            t.envelopes.push(env);
            if build != "f32" {
                let f32_ref = read_f32(&out.join("ref").join(format!("f32.{}.f32", case.index)))?;
                t.drift.push(correlation(&ref_audio, &f32_ref));
            }
            if audio.len() == ref_audio.len() {
                t.same_length += 1;
                t.worst_diff = t.worst_diff.max(max_diff(&audio, &ref_audio));
            }
            t.corrs.push(corr);
            t.samples += audio.len();
            t.ref_samples += ref_audio.len();
            let _ = writeln!(
                per_case,
                "| {build} | {} | {} | {} | {} | {corr:.6} | {env:.4} |",
                case.index,
                case.voice,
                audio.len(),
                ref_audio.len(),
            );
            if case.index < 3 || case.index == 5 {
                write_wav(
                    &wav_dir.join(format!("{stem}.ours.wav")),
                    &audio,
                    SAMPLE_RATE,
                )?;
                write_wav(
                    &wav_dir.join(format!("{stem}.ref.wav")),
                    &ref_audio,
                    SAMPLE_RATE,
                )?;
            }
        }
        let worst = t.corrs.iter().copied().fold(1.0, f64::min);
        let median = middle(&t.corrs);
        let envelope = middle(&t.envelopes);
        let drift = match t.drift.is_empty() {
            true => "-".to_owned(),
            false => format!("{:.4}", middle(&t.drift)),
        };
        let length_ratio = t.samples as f64 / t.ref_samples.max(1) as f64;
        let ok = t.cases > 0
            && match build {
                "f32" => t.same_length == t.cases && worst >= SAME_WAVE,
                _ => envelope >= Q8_ENVELOPE && (length_ratio - 1.0).abs() <= LENGTH_DRIFT,
            };
        all_ok &= ok;
        let _ = writeln!(
            rows,
            "| {build} vs ptts {quant} | {}/{} | {worst:.6} | {median:.6} | {drift} | {:.1e} | {envelope:.4} | {:.4} | {length_ratio:.3} | {} |",
            t.same_length,
            t.cases,
            t.worst_diff,
            t.worst_envelope,
            if ok { "yes" } else { "**no**" }
        );
        let audio_s = t.samples as f64 / f64::from(SAMPLE_RATE);
        let (ref_s, ref_audio) = (reference.timing.get(quant))
            .map(|r| (r.seconds, r.samples as f64 / f64::from(SAMPLE_RATE)))
            .unwrap_or((0.0, 0.0));
        let ref_load = reference.load.get(quant).copied().unwrap_or(0.0);
        let _ = writeln!(
            speed,
            "| {build} | {load:.2} s | {ref_load:.2} s | {:.3} | {:.3} | {:.3} | {:.3} |",
            t.lm_seconds / audio_s.max(1e-9),
            t.decode_seconds / audio_s.max(1e-9),
            (t.lm_seconds + t.decode_seconds) / audio_s.max(1e-9),
            ref_s / ref_audio.max(1e-9),
        );
    }
    let _ = write!(
        md,
        "Temperature 0 on both sides, text not normalised (`lang=\"none\"`).\n\n\
         | Build | Same lengths | Worst corr | Median corr | ptts q8 vs f32 | Max diff | Median envelope | Worst envelope | Length | Pass |\n\
         |---|---|---|---|---|---|---|---|---|---|\n{rows}\n\
         Gates: f32 every length equal and corr >= {SAME_WAVE}; q8 median envelope \
         >= {Q8_ENVELOPE} and total length within {:.0}%. ptts runs its 8-bit \
         matrices on rounded activations, ours widen them to f32, so q8 waves \
         drift apart as ptts's own q8 drifts from its f32.\n\n\
         ## Latents against the numpy model (f32, first run)\n\n\
         | Case | Frames ours / numpy | Corr | Max diff | Ended at | Pass |\n\
         |---|---|---|---|---|---|\n{stage_rows}\n\
         Gate: same frames, max diff <= {LATENT_DIFF:.0e}.\n\n\
         ## Tokenizer against `tokenizers`\n\n{token_md}\n\
         ## Speed (seconds of compute per second of speech)\n\n\
         | Build | Load | ptts load | Model | Mimi | Ours | ptts |\n|---|---|---|---|---|---|---|\n{speed}\n\
         ## Each case\n\n\
         | Build | Case | Voice | Samples | Reference | Corr | Envelope |\n\
         |---|---|---|---|---|---|---|\n{per_case}",
        LENGTH_DRIFT * 100.0,
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
            if compare(&arg(2)?, &arg(3)?, &arg(4)?)? {
                Ok(())
            } else {
                Err("some checks fail".into())
            }
        }
        _ => Err("usage: phonon-parity prepare|compare …".into()),
    }
}
