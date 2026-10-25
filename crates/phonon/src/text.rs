//! Text cut into the runs the model speaks, as the reference plans them.

use crate::tokenizer::Tokenizer;

/// Tokens a run gathers sentences up to.
pub const CHUNK_TOKENS: usize = 50;
/// Tokens past which one sentence is cut at words.
const FIT_TOKENS: usize = 200;
/// Words up to which a text counts as short.
const SHORT_WORDS: usize = 4;

/// One run: its text, its tokens and how long it may go on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub text: String,
    pub tokens: Vec<u32>,
    /// Frames kept after the model first says it is done.
    pub frames_after_eos: usize,
    /// Most frames the run may take.
    pub frame_budget: usize,
}

impl Chunk {
    fn new(text: String, tokenizer: &Tokenizer, frame_rate: f64) -> Self {
        let (prepared, frames_after_eos) = prepare(&text);
        let tokens = tokenizer.encode(&prepared);
        let frame_budget = budget(tokens.len(), frame_rate);
        Self {
            text,
            tokens,
            frames_after_eos,
            frame_budget,
        }
    }
}

/// Frames a run may take: a third of a second a token, plus two.
fn budget(tokens: usize, frame_rate: f64) -> usize {
    ((tokens as f64 / 3.0 + 2.0) * frame_rate).ceil() as usize
}

/// One line, capitalised and closed; short ones keep more frames after.
pub fn prepare(text: &str) -> (String, usize) {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() {
        return (joined, 3);
    }
    let words = joined.split(' ').count();
    let after = if words <= SHORT_WORDS { 3 } else { 1 };
    let mut chars = joined.chars();
    let mut out: String = chars
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    out.push_str(chars.as_str());
    if out.chars().last().is_some_and(char::is_alphanumeric) {
        out.push('.');
    }
    (out, after)
}

/// `text` as runs of whole sentences, each at most [`CHUNK_TOKENS`] long.
pub fn chunks(tokenizer: &Tokenizer, text: &str, frame_rate: f64) -> Vec<Chunk> {
    let groups = sentences(tokenizer, text, CHUNK_TOKENS);
    let planned = groups
        .into_iter()
        .map(|t| Chunk::new(t, tokenizer, frame_rate));
    fit(planned.collect(), tokenizer, frame_rate)
}

/// Sentences grouped greedily under `max` tokens, as text.
fn sentences(tokenizer: &Tokenizer, text: &str, max: usize) -> Vec<String> {
    let (prepared, _) = prepare(text);
    let tokens = tokenizer.encode(prepared.trim());
    let marks = tokenizer.encode(".!...?");
    let ends: &[u32] = marks.get(1..).filter(|m| !m.is_empty()).unwrap_or(&marks);
    let mut cuts = vec![0];
    let mut after_end = false;
    for (i, t) in tokens.iter().enumerate() {
        match ends.contains(t) {
            true => after_end = true,
            false => {
                if after_end {
                    cuts.push(i);
                }
                after_end = false;
            }
        }
    }
    cuts.push(tokens.len());
    let mut groups = Vec::new();
    let mut current = String::new();
    let mut count = 0;
    for w in cuts.windows(2) {
        let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else {
            continue;
        };
        let sentence = tokenizer.decode(tokens.get(a..b).unwrap_or_default());
        let n = b - a;
        if current.is_empty() {
            (current, count) = (sentence, n);
        } else if count + n > max {
            groups.push(current.trim().to_owned());
            (current, count) = (sentence, n);
        } else {
            current.push(' ');
            current.push_str(&sentence);
            count += n;
        }
    }
    if !current.is_empty() {
        groups.push(current.trim().to_owned());
    }
    groups
}

/// Runs over [`FIT_TOKENS`] cut in two at words until each fits.
fn fit(chunks: Vec<Chunk>, tokenizer: &Tokenizer, frame_rate: f64) -> Vec<Chunk> {
    let mut done = Vec::with_capacity(chunks.len());
    let mut todo: Vec<Chunk> = chunks.into_iter().rev().collect();
    while let Some(chunk) = todo.pop() {
        if chunk.tokens.len() <= FIT_TOKENS {
            done.push(chunk);
            continue;
        }
        let words: Vec<&str> = chunk.text.split_whitespace().collect();
        let halves = match words.len() >= 2 {
            true => {
                let (a, b) = words.split_at(cut(&words, FIT_TOKENS.div_ceil(4), tokenizer));
                [a.join(" "), b.join(" ")]
            }
            false => {
                let word = chunk.text.trim();
                match word.char_indices().nth(word.chars().count() / 2) {
                    Some((mid, _)) if mid > 0 => {
                        let (a, b) = word.split_at(mid);
                        [a.to_owned(), b.to_owned()]
                    }
                    _ => {
                        done.push(chunk);
                        continue;
                    }
                }
            }
        };
        for half in halves.into_iter().rev() {
            todo.push(Chunk::new(half, tokenizer, frame_rate));
        }
    }
    done
}

/// Where to cut `words`: after a sentence end, else a clause, else the middle.
fn cut(words: &[&str], least: usize, tokenizer: &Tokenizer) -> usize {
    let mut ends = Vec::with_capacity(words.len());
    let mut total = 0;
    for w in words {
        total += tokenizer.encode(w).len();
        ends.push(total);
    }
    let inner = || ends.iter().copied().enumerate().take(words.len() - 1);
    let imbalance = |&(_, end): &(usize, usize)| end.abs_diff(total - end);
    let closers = ['"', '\'', ')', ']', '\u{201D}', '\u{2019}', '\u{BB}'];
    let after = |marks: &[char]| {
        inner()
            .filter(|&(i, end)| {
                let w = words.get(i).map_or("", |w| w.trim_end_matches(closers));
                w.ends_with(marks) && end >= least && total - end >= least
            })
            .min_by_key(imbalance)
    };
    let picked = after(&['.', '!', '?'])
        .or_else(|| after(&[',', ';', ':']))
        .or_else(|| inner().min_by_key(imbalance));
    picked.map_or(1, |(i, _)| i + 1)
}
