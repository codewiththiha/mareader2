//! Which words of the sentence said are lit: a few, a phrase, or all.

use std::ops::Range;
use std::time::{Duration, Instant};

use mareader_settings::Highlight;
use mareader_speech::Word;

/// Words in a phrase at most; a longer one is cut in even parts.
const PHRASE_MOST: usize = 6;
/// Bytes said a second at normal speed, before a sentence is timed.
const PACE: f64 = 14.0;
/// Marks a voice pauses after, ending a phrase.
const PAUSES: [char; 13] = [
    ',', ';', ':', '—', '–', '.', '!', '?', '…', ')', '"', '”', '»',
];

/// The words lit while word `at` of `words` is heard.
pub(super) fn group(
    text: &str,
    words: &[Word],
    at: usize,
    (mode, count): (Highlight, u8),
) -> Range<usize> {
    let len = words.len();
    let at = at.min(len.saturating_sub(1));
    match mode {
        Highlight::Sentence => 0..len,
        Highlight::Words => {
            let n = usize::from(count.max(1));
            let from = at / n * n;
            from..(from + n).min(len)
        }
        Highlight::Phrases => (phrases(text, words).into_iter())
            .find(|r| r.contains(&at))
            .unwrap_or(0..len),
    }
}

/// `words` cut where the voice pauses, none longer than [`PHRASE_MOST`].
fn phrases(text: &str, words: &[Word]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    for (i, word) in words.iter().enumerate() {
        let said = text.get(word.bytes.clone()).unwrap_or_default();
        if said.ends_with(PAUSES) || i + 1 == words.len() {
            out.extend(even(from..i + 1));
            from = i + 1;
        }
    }
    out
}

/// `run` in the fewest even parts no longer than [`PHRASE_MOST`].
fn even(run: Range<usize>) -> Vec<Range<usize>> {
    let len = run.len();
    let parts = len.div_ceil(PHRASE_MOST).max(1);
    let edge = |k: usize| run.start + len * k / parts;
    (0..parts).map(|k| edge(k)..edge(k + 1)).collect()
}

/// The word heard: at `byte` when the voice knows it, else at `guess`.
pub(super) fn heard(words: &[Word], byte: Option<usize>, guess: f64) -> usize {
    let byte = byte.unwrap_or(guess.max(0.0) as usize);
    words
        .partition_point(|w| w.bytes.start <= byte)
        .saturating_sub(1)
}

/// How long the sentence said has been heard, pauses left out.
#[derive(Default)]
pub(super) struct Clock {
    since: Option<Instant>,
    banked: Duration,
}

impl Clock {
    /// From nothing, now.
    pub fn start(&mut self) {
        *self = Self {
            since: Some(Instant::now()),
            banked: Duration::ZERO,
        };
    }

    pub fn stop(&mut self) {
        if let Some(since) = self.since.take() {
            self.banked += since.elapsed();
        }
    }

    pub fn go(&mut self) {
        self.since.get_or_insert_with(Instant::now);
    }

    pub fn seconds(&self) -> f64 {
        let running = self.since.map_or(Duration::ZERO, |s| s.elapsed());
        (self.banked + running).as_secs_f64()
    }
}

/// Bytes said a second at normal speed, learnt from sentences heard.
pub(super) struct Pace(f64);

impl Default for Pace {
    fn default() -> Self {
        Self(PACE)
    }
}

impl Pace {
    /// `bytes` heard whole in `seconds` at `rate`.
    pub fn learn(&mut self, bytes: usize, seconds: f64, rate: f32) {
        let rate = f64::from(rate.max(0.1));
        if seconds < 0.5 || bytes < 8 {
            return;
        }
        let now = bytes as f64 / seconds / rate;
        self.0 = (self.0 * 0.7 + now * 0.3).clamp(4.0, 60.0);
    }

    /// Bytes likely heard after `seconds` at `rate`.
    pub fn bytes(&self, seconds: f64, rate: f32) -> f64 {
        seconds * self.0 * f64::from(rate.max(0.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_speech::Spot;

    fn words(text: &str) -> Vec<Word> {
        let mut out = Vec::new();
        let mut at = 0;
        for run in text.split(' ') {
            let spot = Spot { page: 0, char: at };
            out.push(Word {
                bytes: at..at + run.len(),
                start: spot,
                end: spot,
            });
            at += run.len() + 1;
        }
        out
    }

    #[test]
    fn a_few_words_light_in_steps_and_a_sentence_whole() {
        let text = "one two three four five";
        let w = words(text);
        let ask = |at, mode| group(text, &w, at, mode);
        assert_eq!(ask(3, (Highlight::Words, 1)), 3..4);
        assert_eq!(ask(3, (Highlight::Words, 2)), 2..4);
        assert_eq!(
            ask(4, (Highlight::Words, 2)),
            4..5,
            "the last step is short"
        );
        assert_eq!(ask(1, (Highlight::Words, 10)), 0..5);
        assert_eq!(ask(2, (Highlight::Sentence, 3)), 0..5);
        assert_eq!(ask(99, (Highlight::Words, 1)), 4..5, "held to the last");
    }

    #[test]
    fn phrases_end_where_the_voice_pauses_and_long_ones_halve() {
        let text = "Well, it was late; a b c d e f g h i j k l m n.";
        let w = words(text);
        let ask = |at| group(text, &w, at, (Highlight::Phrases, 1));
        assert_eq!(ask(0), 0..1, "\"Well,\"");
        assert_eq!(ask(2), 1..4, "\"it was late;\"");
        // Fourteen words without a pause: three even parts.
        assert_eq!(ask(4), 4..8);
        assert_eq!(ask(9), 8..13);
        assert_eq!(ask(17), 13..18);
    }

    #[test]
    fn the_word_heard_is_found_by_byte_or_guessed() {
        let w = words("one two three");
        assert_eq!(heard(&w, Some(4), 0.0), 1);
        assert_eq!(heard(&w, Some(6), 0.0), 1, "inside a word");
        assert_eq!(heard(&w, None, 9.5), 2);
        assert_eq!(heard(&w, None, -3.0), 0);
        assert_eq!(heard(&[], Some(3), 0.0), 0);
    }

    #[test]
    fn the_pace_learns_from_sentences_heard_and_keeps_to_its_speed() {
        let mut pace = Pace::default();
        assert_eq!(pace.bytes(2.0, 1.0), 28.0);
        pace.learn(40, 1.0, 2.0);
        // Twenty a second at normal speed, a share of the way there.
        assert!((pace.0 - (14.0 * 0.7 + 20.0 * 0.3)).abs() < 1e-9);
        pace.learn(3, 9.0, 1.0);
        pace.learn(90, 0.1, 1.0);
        assert!((pace.0 - 15.8).abs() < 1e-9, "too short to trust");
        assert!((pace.bytes(1.0, 2.0) - 31.6).abs() < 1e-9);
    }
}
