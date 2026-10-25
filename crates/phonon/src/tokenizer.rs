//! The checkpoint's unigram tokenizer: Viterbi over pieces, bytes as fallback.

use std::collections::HashMap;

use serde::Deserialize;

use crate::{Error, Result};

const SPACE: char = '\u{2581}';
/// What an uncovered character costs below the cheapest piece.
const UNKNOWN_PENALTY: f64 = 10.0;

#[derive(Deserialize)]
struct File {
    model: Model,
    #[serde(default)]
    added_tokens: Vec<Added>,
}

#[derive(Deserialize)]
struct Model {
    #[serde(rename = "type")]
    kind: String,
    unk_id: Option<u32>,
    vocab: Vec<(String, f64)>,
    #[serde(default)]
    byte_fallback: bool,
}

#[derive(Deserialize)]
struct Added {
    id: u32,
    #[serde(default)]
    special: bool,
}

/// Text to ids and back, as the reference tokenizer does it.
pub struct Tokenizer {
    pieces: Vec<(String, f64)>,
    ids: HashMap<String, u32>,
    unknown: u32,
    unknown_score: f64,
    bytes: bool,
    /// Longest piece, in bytes.
    longest: usize,
    special: Vec<bool>,
}

/// The best path's last piece into a byte offset.
#[derive(Clone, Copy)]
struct Best {
    score: f64,
    from: Option<usize>,
    id: u32,
}

impl Tokenizer {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let file: File = serde_json::from_slice(bytes)
            .map_err(|e| Error::Load(format!("tokenizer.json: {e}")))?;
        let model = file.model;
        if model.kind != "Unigram" {
            return Err(Error::Load(format!(
                "a {} tokenizer, not unigram",
                model.kind
            )));
        }
        let unknown = model.unk_id.unwrap_or(0);
        let low = model
            .vocab
            .iter()
            .map(|(_, s)| *s)
            .fold(f64::INFINITY, f64::min);
        let mut ids = HashMap::with_capacity(model.vocab.len());
        for (i, (piece, _)) in model.vocab.iter().enumerate() {
            ids.entry(piece.clone()).or_insert(i as u32);
        }
        let mut special = vec![false; model.vocab.len()];
        for a in file.added_tokens.iter().filter(|a| a.special) {
            if let Some(s) = special.get_mut(a.id as usize) {
                *s = true;
            }
        }
        Ok(Self {
            longest: model.vocab.iter().map(|(p, _)| p.len()).max().unwrap_or(1),
            pieces: model.vocab,
            ids,
            unknown,
            unknown_score: low - UNKNOWN_PENALTY,
            bytes: model.byte_fallback,
            special,
        })
    }

    /// Ids for `text`, no special tokens added.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        if text.is_empty() {
            return Vec::new();
        }
        let marked: String = std::iter::once(SPACE)
            .chain(text.chars().map(|c| if c == ' ' { SPACE } else { c }))
            .collect();
        let mut ids = Vec::new();
        for word in words(&marked) {
            for piece in self.viterbi(word) {
                self.push(piece, &mut ids);
            }
        }
        ids
    }

    /// Text for `ids`, special tokens skipped.
    pub fn decode(&self, ids: &[u32]) -> String {
        let mut out = String::new();
        let mut run: Vec<u8> = Vec::new();
        let kept = ids
            .iter()
            .filter(|&&id| !self.special.get(id as usize).copied().unwrap_or(false));
        for (i, &id) in kept.enumerate() {
            let piece = self.pieces.get(id as usize).map_or("", |(p, _)| p.as_str());
            if let Some(b) = byte_of(piece) {
                run.push(b);
                continue;
            }
            flush(&mut run, &mut out);
            let mut text: String = piece
                .chars()
                .map(|c| if c == SPACE { ' ' } else { c })
                .collect();
            if i == 0 && text.starts_with(' ') {
                text.remove(0);
            }
            out.push_str(&text);
        }
        flush(&mut run, &mut out);
        out
    }

    /// One piece's ids: its own, its bytes', or the unknown one.
    fn push(&self, piece: &str, ids: &mut Vec<u32>) {
        if let Some(&id) = self.ids.get(piece) {
            ids.push(id);
            return;
        }
        if self.bytes {
            let found: Option<Vec<u32>> = (piece.bytes())
                .map(|b| self.ids.get(&format!("<0x{b:02X}>")).copied())
                .collect();
            if let Some(found) = found {
                ids.extend(found);
                return;
            }
        }
        ids.push(self.unknown);
    }

    /// The best-scoring pieces for `word`; uncovered runs come back joined.
    fn viterbi<'a>(&self, word: &'a str) -> Vec<&'a str> {
        let size = word.len();
        let none = Best {
            score: 0.0,
            from: None,
            id: self.unknown,
        };
        let mut best = vec![none; size + 1];
        for (start, c) in word.char_indices() {
            let here = best.get(start).map_or(0.0, |b| b.score);
            let mut single = false;
            let end = (start + self.longest).min(size);
            for stop in (start + 1..=end).filter(|&e| word.is_char_boundary(e)) {
                let Some(&id) = word.get(start..stop).and_then(|p| self.ids.get(p)) else {
                    continue;
                };
                let score = here + self.pieces.get(id as usize).map_or(0.0, |(_, s)| *s);
                if let Some(slot) = best.get_mut(stop)
                    && (slot.from.is_none() || score > slot.score)
                {
                    *slot = Best {
                        score,
                        from: Some(start),
                        id,
                    };
                }
                single |= stop - start == c.len_utf8();
            }
            if !single {
                let score = here + self.unknown_score;
                if let Some(slot) = best.get_mut(start + c.len_utf8())
                    && (slot.from.is_none() || score > slot.score)
                {
                    *slot = Best {
                        score,
                        from: Some(start),
                        id: self.unknown,
                    };
                }
            }
        }
        self.backtrack(word, &best)
    }

    fn backtrack<'a>(&self, word: &'a str, best: &[Best]) -> Vec<&'a str> {
        let mut out: Vec<&'a str> = Vec::new();
        // An unknown run's start, kept while the run grows backwards.
        let mut run_end: Option<usize> = None;
        let mut end = word.len();
        while end > 0 {
            let Some(node) = best.get(end) else { break };
            let start = node.from.unwrap_or(0);
            if node.id == self.unknown {
                run_end.get_or_insert(end);
            } else {
                if let Some(e) = run_end.take() {
                    out.extend(word.get(end..e));
                }
                out.extend(word.get(start..end));
            }
            end = start;
        }
        if let Some(e) = run_end {
            out.extend(word.get(0..e));
        }
        out.reverse();
        out
    }
}

/// `text` cut before each `▁`, so every word but a lead starts with one.
fn words(text: &str) -> impl Iterator<Item = &str> {
    let mut starts: Vec<usize> = text.match_indices(SPACE).map(|(i, _)| i).collect();
    if starts.first() != Some(&0) {
        starts.insert(0, 0);
    }
    let ends = starts
        .iter()
        .skip(1)
        .copied()
        .chain(std::iter::once(text.len()));
    let cuts: Vec<(usize, usize)> = starts.iter().copied().zip(ends).collect();
    cuts.into_iter().filter_map(move |(a, b)| text.get(a..b))
}

/// The byte a `<0xHH>` piece stands for.
fn byte_of(piece: &str) -> Option<u8> {
    let hex = piece.strip_prefix("<0x")?.strip_suffix('>')?;
    match hex.len() {
        2 => u8::from_str_radix(hex, 16).ok(),
        _ => None,
    }
}

/// A run of bytes as text, each bad byte a replacement mark.
fn flush(run: &mut Vec<u8>, out: &mut String) {
    if run.is_empty() {
        return;
    }
    match std::str::from_utf8(run) {
        Ok(s) => out.push_str(s),
        Err(_) => out.extend(run.iter().map(|_| '\u{FFFD}')),
    }
    run.clear();
}
