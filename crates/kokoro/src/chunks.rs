//! Phoneme text into model-sized pieces, split where speech pauses.

use std::collections::HashMap;

/// One piece of phonemes the model reads at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Token ids, without the padding the model adds.
    pub ids: Vec<u32>,
    /// Silence owed after it, at normal speed.
    pub pause_ms: u32,
}

/// How strong a pause the character marks: 3 sentence, 2 clause, 1 word.
fn rank(ch: char, previous: u8) -> u8 {
    match ch {
        '.' | '!' | '?' | '。' | '！' | '？' | '\n' => 3,
        ',' | ';' | ':' | '，' | '；' | '：' => 2,
        ch if ch.is_whitespace() => previous.max(1),
        _ => 0,
    }
}

/// Chunks of at most `most` tokens, split at the strongest pause.
pub(crate) fn split(phonemes: &str, vocab: &HashMap<String, u32>, most: usize) -> Vec<Chunk> {
    split_led(phonemes, vocab, 0, most)
}

/// As [`split`], but the first chunk ends near `lead` tokens; 0 never does.
pub(crate) fn split_led(
    phonemes: &str,
    vocab: &HashMap<String, u32>,
    lead: usize,
    most: usize,
) -> Vec<Chunk> {
    let tokens = tokens(phonemes, vocab);
    let mut chunks = Vec::new();
    let mut rest = tokens.as_slice();
    if let Some((end, boundary)) = lead_end(rest, lead.min(most)) {
        let (head, tail) = rest.split_at(end);
        chunks.push(chunk(head, boundary));
        rest = tail;
    }
    while !rest.is_empty() && most > 0 {
        let window = rest.get(..most.min(rest.len())).unwrap_or(rest);
        let (end, boundary) = if window.len() == rest.len() {
            (rest.len(), 0)
        } else {
            // Never right after the first token: that leaves a lone space.
            let inner = window.get(1..).unwrap_or_default();
            (1..=3u8)
                .rev()
                .find_map(|r| inner.iter().rposition(|t| t.1 == r).map(|at| (at + 2, r)))
                .unwrap_or((window.len(), 0))
        };
        let (head, tail) = rest.split_at(end);
        chunks.push(chunk(head, boundary));
        rest = tail;
    }
    chunks
}

/// A short first chunk's end and pause: an early clause, else a word.
fn lead_end(tokens: &[(u32, u8)], lead: usize) -> Option<(usize, u8)> {
    if lead == 0 || tokens.len() <= lead {
        return None;
    }
    let from = (lead / 3).max(1);
    let window = tokens.get(from..lead)?;
    if let Some(at) = window.iter().position(|t| t.1 >= 2) {
        let mut end = from + at + 1;
        // The space after a comma goes with it.
        while tokens.get(end).is_some_and(|t| t.1 >= 2) {
            end += 1;
        }
        let boundary = tokens.get(from + at).map_or(2, |t| t.1);
        return Some((end, boundary));
    }
    let at = window.iter().rposition(|t| t.1 == 1)?;
    Some((from + at + 1, 0))
}

fn chunk(tokens: &[(u32, u8)], boundary: u8) -> Chunk {
    Chunk {
        ids: tokens.iter().map(|t| t.0).collect(),
        pause_ms: match boundary {
            3 => 200,
            2 => 100,
            _ => 0,
        },
    }
}

/// Each known symbol's id, with the strongest pause it ends.
fn tokens(phonemes: &str, vocab: &HashMap<String, u32>) -> Vec<(u32, u8)> {
    let mut tokens: Vec<(u32, u8)> = Vec::new();
    let mut previous = 0;
    let mut utf8 = [0u8; 4];
    for ch in phonemes.chars() {
        let boundary = rank(ch, previous);
        if let Some(&id) = vocab.get(&*ch.encode_utf8(&mut utf8)) {
            tokens.push((id, boundary));
        } else if let (true, Some((_, last))) = (boundary > 0, tokens.last_mut()) {
            *last = (*last).max(boundary);
        }
        previous = boundary;
    }
    tokens
}

/// Pads with silence to end quiet for `millis`, counting quiet already there.
pub(crate) fn pause(samples: &mut Vec<f32>, rate: u32, millis: u32, speed: f32) {
    let target = (f64::from(rate) * f64::from(millis) / (1000.0 * f64::from(speed))).round();
    let target = target as usize;
    let quiet = samples
        .iter()
        .rev()
        .take(target)
        .take_while(|s| s.abs() < 0.003)
        .count();
    samples.resize(samples.len() + target.saturating_sub(quiet), 0.0);
}

/// G2P output mapped onto the model's alphabet; unknown symbols dropped.
pub(crate) fn normalise(phonemes: &str, vocab: &HashMap<String, u32>) -> String {
    let mut out = String::with_capacity(phonemes.len());
    let mut utf8 = [0u8; 4];
    for ch in phonemes.chars() {
        let ch = match ch {
            'g' => 'ɡ',
            'ɝ' => 'ɚ',
            'ɬ' => 'l',
            ch if ch.is_whitespace() => ' ',
            '\u{0361}' => continue,
            ch => ch,
        };
        let known = vocab.contains_key(&*ch.encode_utf8(&mut utf8));
        if known && (ch != ' ' || !out.ends_with(' ')) {
            out.push(ch);
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab(symbols: &str) -> HashMap<String, u32> {
        symbols
            .chars()
            .zip(1..)
            .map(|(ch, id)| (ch.to_string(), id))
            .collect()
    }

    fn sizes(chunks: &[Chunk]) -> Vec<usize> {
        chunks.iter().map(|c| c.ids.len()).collect()
    }

    #[test]
    fn long_runs_split_at_the_limit_and_keep_every_token() {
        let v = vocab("aʃ .?,;");
        for count in [1, 510, 511, 2048] {
            let chunks = split(&"ʃ".repeat(count), &v, 510);
            assert!(
                chunks
                    .iter()
                    .all(|c| !c.ids.is_empty() && c.ids.len() <= 510)
            );
            assert_eq!(sizes(&chunks).iter().sum::<usize>(), count);
            assert_eq!(chunks.len(), count.div_ceil(510));
        }
        assert!(split("", &v, 510).is_empty());
        assert!(split("aaa", &v, 0).is_empty());
    }

    #[test]
    fn splits_prefer_sentence_ends_then_spaces() {
        let v = vocab("a .");
        let chunks = split("aa. aaa aaa", &v, 8);
        assert_eq!(sizes(&chunks), [4, 7]);
        assert_eq!(chunks.first().map(|c| c.pause_ms), Some(200));
        assert_eq!(chunks.get(1).map(|c| c.pause_ms), Some(0));
        let chunks = split("aaa aaa aaa", &v, 7);
        assert_eq!(sizes(&chunks), [4, 7]);
        assert_eq!(chunks.first().map(|c| c.pause_ms), Some(0));
        // The space after a stop never makes a chunk alone.
        let chunks = split("aa. aaa aaa. a", &v, 5);
        assert_eq!(sizes(&chunks), [4, 4, 5, 1]);
    }

    #[test]
    fn a_led_first_chunk_ends_at_an_early_clause_else_a_word() {
        let v = vocab("a ,.");
        // A comma past a third of the lead ends it, with its space.
        let chunks = split_led("aaaa, aaaaaaaa aaaaaaaa aaaa.", &v, 12, 510);
        assert_eq!(sizes(&chunks), [6, 23]);
        assert_eq!(chunks.first().map(|c| c.pause_ms), Some(100));
        // Too early a comma is passed over for the last word in reach.
        let chunks = split_led("a, aaaa aaaa aaaa aaaa.", &v, 12, 510);
        assert_eq!(sizes(&chunks), [8, 15]);
        assert_eq!(chunks.first().map(|c| c.pause_ms), Some(0));
        // Short text, one long word, or no lead: as split makes it.
        let short = "aaaa, aaaa.";
        assert_eq!(split_led(short, &v, 12, 510), split(short, &v, 510));
        assert_eq!(sizes(&split_led(&"a".repeat(40), &v, 12, 510)), [40]);
        let long = "aaaa, aaaaaaaa aaaaaaaa aaaa.";
        assert_eq!(split_led(long, &v, 0, 510), split(long, &v, 510));
        let all: usize = sizes(&split_led(long, &v, 12, 8)).iter().sum();
        assert_eq!(all, long.chars().count(), "every token kept");
    }

    #[test]
    fn pauses_top_up_quiet_that_is_already_there() {
        let mut samples = vec![0.2, 0.0, 0.0];
        pause(&mut samples, 1000, 5, 1.0);
        assert_eq!(samples.len(), 6);
        pause(&mut samples, 1000, 5, 1.0);
        assert_eq!(samples.len(), 6);
    }

    #[test]
    fn normalising_maps_lookalikes_and_drops_strangers() {
        let v = vocab("ɡɚl həO");
        assert_eq!(normalise("  gɝɬ\t\thə͡O ✓ ", &v), "ɡɚl həO");
    }
}
