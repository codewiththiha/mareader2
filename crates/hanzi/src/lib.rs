//! Chinese text to the IPA Kokoro's Mandarin voices were trained on.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod json;
mod numbers;
mod pinyin;
mod syllable;
mod table;
mod words;

#[cfg(test)]
mod tests;

use std::path::Path;

/// The files `Hanzi::load` reads, named as their sources name them.
pub const FILES: [&str; 4] = [
    "pinyin_dict.json",
    "phrases_dict.json",
    "dict.txt",
    "prob_emit.py",
];

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// A file that doesn't read as it should, by name.
    Bad(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Bad(name) => write!(f, "{name} doesn't read as expected"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// misaki's first Chinese reader: numbers, words, pinyin, then phonemes.
pub struct Hanzi {
    words: words::Words,
    pinyin: pinyin::Pinyin,
}

impl Hanzi {
    /// From the `FILES` in `dir`.
    pub fn load(dir: &Path) -> Result<Self, Error> {
        // One source at a time, so a file's text goes before the next loads.
        let read = |name: &str| std::fs::read_to_string(dir.join(name));
        let [chars, phrases, dict, emit] = FILES;
        let words = words::Words::parse(&read(dict)?, &read(emit)?)?;
        let pinyin = pinyin::Pinyin::parse(&read(chars)?, &read(phrases)?)?;
        Ok(Self { words, pinyin })
    }

    /// From the text of each of `FILES`, in order.
    pub fn parse(chars: &str, phrases: &str, dict: &str, emit: &str) -> Result<Self, Error> {
        Ok(Self {
            words: words::Words::parse(dict, emit)?,
            pinyin: pinyin::Pinyin::parse(chars, phrases)?,
        })
    }

    /// How many characters, phrases and words it knows.
    pub fn size(&self) -> (usize, usize, usize) {
        let (chars, phrases) = self.pinyin.len();
        (chars, phrases, self.words.len())
    }

    /// `text` as Kokoro's phonemes: words apart, tones as arrows.
    pub fn phonemize(&self, text: &str) -> String {
        if text.trim().is_empty() {
            return String::new();
        }
        let text = punctuate(&numbers::spell(text));
        let mut out = String::with_capacity(text.len() * 3);
        let mut words = Vec::new();
        for (han, run) in runs(&text) {
            if !han {
                out.push_str(run);
                continue;
            }
            words.clear();
            self.words.cut(run, &mut words);
            for (i, word) in words.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                self.pinyin.push(word, &mut out);
            }
        }
        out.retain(|c| c != '\u{32F}');
        out
    }
}

fn is_han(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c)
}

/// The text in alternating runs of Han and other characters.
fn runs(text: &str) -> impl Iterator<Item = (bool, &str)> {
    let mut rest = text;
    std::iter::from_fn(move || {
        let first = rest.chars().next()?;
        let han = is_han(first);
        let end = rest.find(|c: char| is_han(c) != han).unwrap_or(rest.len());
        let (run, tail) = rest.split_at_checked(end)?;
        rest = tail;
        Some((han, run))
    })
}

/// Full-width punctuation as Kokoro reads it, as misaki maps it.
fn punctuate(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '、' | '，' => out.push_str(", "),
            '。' | '．' => out.push_str(". "),
            '！' => out.push_str("! "),
            '：' => out.push_str(": "),
            '；' => out.push_str("; "),
            '？' => out.push_str("? "),
            '«' | '《' | '「' | '【' => out.push_str(" “"),
            '»' | '》' | '」' | '】' => out.push_str("” "),
            '（' => out.push_str(" ("),
            '）' => out.push_str(") "),
            c => out.push(c),
        }
    }
    out.trim().into()
}

/// One tone-marked pinyin syllable as Kokoro's phonemes, for checks.
pub fn syllable_ipa(syllable: &str) -> String {
    let mut out = String::new();
    syllable::push(syllable, &mut out);
    out
}
