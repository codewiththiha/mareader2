//! English text to Kokoro's phonemes: misaki's pipeline, ported to Rust.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod en;
mod lexicon;
mod neural;
mod numbers;
mod tagger;
mod text;
mod tokenize;

use std::path::{Path, PathBuf};

use en::{MToken, Patterns};
use lexicon::Lexicon;
use neural::NeuralG2p;
use tagger::Tagger;
use tokenize::Tokenizer;

/// What stands for a word nothing could pronounce.
pub const UNKNOWN: &str = "❓";

/// spaCy's tokenizer rules, shared by both dialects.
pub const TOKENIZER: &str = "spacy-en-tokenizer.json";
/// The part-of-speech tagger's weights, shared by both dialects.
pub const TAGGER: &str = "tagger-en.bin";

/// Which English the lexicons, rules and neural model follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialect {
    American,
    British,
}

impl Dialect {
    /// From a Kokoro voice id's first letter: `a` American, `b` British.
    pub fn for_kokoro_voice(voice: &str) -> Option<Self> {
        match voice.chars().next()? {
            'a' => Some(Self::American),
            'b' => Some(Self::British),
            _ => None,
        }
    }

    /// Its gold and silver lexicons and its neural model, by file name.
    pub fn files(self) -> [&'static str; 3] {
        match self {
            Self::American => [
                "misaki-us_gold.json",
                "misaki-us_silver.json",
                "g2p-en-us.safetensors",
            ],
            Self::British => [
                "misaki-gb_gold.json",
                "misaki-gb_silver.json",
                "g2p-en-gb.safetensors",
            ],
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Read {
        file: PathBuf,
        source: std::io::Error,
    },
    /// A file was read but is not what it should be.
    Data(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { file, source } => write!(f, "couldn't read {}: {source}", file.display()),
            Self::Data(reason) => write!(f, "pronunciation data is damaged: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Data(_) => None,
        }
    }
}

/// The data files' contents, as [`G2p::load`] reads them.
pub struct Sources<'a> {
    pub tokenizer: &'a str,
    pub tagger: &'a [u8],
    pub gold: &'a str,
    pub silver: &'a str,
    /// Without it, unknown words are spelled letter by letter.
    pub neural: Option<&'a [u8]>,
}

/// One spoken word and its phonemes, as Kokoro's chunker takes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhonemeToken {
    pub text: String,
    /// Empty for silent tokens, as a currency sign, and for the unknown.
    pub phonemes: String,
    pub whitespace: bool,
}

/// A phonemizer for one dialect: `Send + Sync`, and slow to build.
pub struct G2p {
    dialect: Dialect,
    tokenizer: Tokenizer,
    tagger: Tagger,
    lexicon: Lexicon,
    patterns: Patterns,
    neural: Option<NeuralG2p>,
}

impl G2p {
    /// Reads [`TOKENIZER`], [`TAGGER`] and the dialect's [`Dialect::files`]
    /// from `dir`.
    pub fn load(dir: &Path, dialect: Dialect) -> Result<Self, Error> {
        let read = |name: &str| {
            let file = dir.join(name);
            std::fs::read(&file).map_err(|source| Error::Read { file, source })
        };
        let text = |name: &str| {
            String::from_utf8(read(name)?).map_err(|_| Error::Data(format!("{name} is not UTF-8")))
        };
        let [gold, silver, neural] = dialect.files();
        Self::from_sources(
            dialect,
            Sources {
                tokenizer: &text(TOKENIZER)?,
                tagger: &read(TAGGER)?,
                gold: &text(gold)?,
                silver: &text(silver)?,
                neural: Some(&read(neural)?),
            },
        )
    }

    pub fn from_sources(dialect: Dialect, sources: Sources<'_>) -> Result<Self, Error> {
        Ok(Self {
            dialect,
            tokenizer: Tokenizer::from_json(sources.tokenizer)?,
            tagger: Tagger::from_bytes(sources.tagger)?,
            lexicon: Lexicon::from_json(dialect == Dialect::British, sources.gold, sources.silver)?,
            patterns: Patterns::new()?,
            neural: sources
                .neural
                .map(|bytes| NeuralG2p::from_bytes(bytes, dialect))
                .transpose()?,
        })
    }

    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Phonemes for `text`, punctuation kept, [`UNKNOWN`] where none were
    /// found. misaki's markup works: `[word](/phonemes/)`, `[word](+2)`.
    pub fn phonemize(&self, text: &str) -> String {
        self.run(text, UNKNOWN)
            .into_iter()
            .map(|tk| tk.phonemes.unwrap_or_else(|| UNKNOWN.to_owned()) + &tk.whitespace)
            .collect()
    }

    /// One token per spoken word, the unknown left silent, as Kokoro reads.
    pub fn tokens(&self, text: &str) -> Vec<PhonemeToken> {
        self.run(text, "")
            .into_iter()
            .map(|tk| PhonemeToken {
                phonemes: tk.phonemes.unwrap_or_default(),
                whitespace: !tk.whitespace.is_empty(),
                text: tk.text,
            })
            .collect()
    }

    fn run(&self, text: &str, unk: &str) -> Vec<MToken> {
        let word_fallback = |word: &str| -> Option<String> {
            // Short and vowelless, as "kg" or "mph": spelled out.
            let vowelless = !word.chars().any(|c| "aeiouyAEIOUY".contains(c));
            if vowelless
                && word.chars().count() <= 5
                && let Some(spelled) = self.lexicon.spell(word)
            {
                return Some(spelled);
            }
            match &self.neural {
                // A word the model can't encode, as "日本", goes silent.
                Some(model) => Some(model.phonemize(word).unwrap_or_default()),
                None => self.lexicon.spell(word),
            }
        };
        let fallback = |word: &str| self.lexicon.compose_fallback(word, &word_fallback);
        en::g2p(
            &self.lexicon,
            &self.patterns,
            text,
            |t| self.tag(t),
            &fallback,
            unk,
        )
    }

    fn tag(&self, text: &str) -> Vec<MToken> {
        let tokens = self.tokenizer.tokenize(text);
        let words: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        let tags = self.tagger.tag(&words);
        tokens
            .into_iter()
            .zip(tags)
            .map(|(t, tag)| {
                // spaCy tags addresses ADD, numerals CD; the pattern is surer.
                let tag = if self.tokenizer.is_url(&t.text) {
                    "ADD".to_owned()
                } else if is_numeral(&t.text) {
                    "CD".to_owned()
                } else {
                    tag
                };
                MToken {
                    whitespace: if t.space_after {
                        " ".to_owned()
                    } else {
                        String::new()
                    },
                    text: t.text,
                    tag,
                    is_head: true,
                    ..MToken::default()
                }
            })
            .collect()
    }
}

/// Digits, thousands commas, a point and a leading minus: "-1,234.5".
fn is_numeral(text: &str) -> bool {
    let body = text.strip_prefix('-').unwrap_or(text);
    body.starts_with(|c: char| c.is_ascii_digit())
        && body
            .chars()
            .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
}

#[cfg(test)]
mod tests;
