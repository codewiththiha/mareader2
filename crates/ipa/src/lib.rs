//! Text to IPA as espeak-ng writes it, by our own rules per language.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod french;
mod italian;
mod letters;
mod portuguese;
mod spanish;
mod text;

#[cfg(test)]
mod tests;

/// A language these rules read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    /// Castilian, as `es` in espeak-ng.
    Spanish,
    Italian,
    /// Brazilian, as `pt-br` in espeak-ng.
    Portuguese,
    /// As `fr-fr` in espeak-ng.
    French,
}

impl Language {
    pub const ALL: [Self; 4] = [Self::Spanish, Self::Italian, Self::Portuguese, Self::French];

    /// The language of a Kokoro voice, from the first letter of its id.
    pub fn for_kokoro_voice(voice: &str) -> Option<Self> {
        match voice.as_bytes().first()? {
            b'e' => Some(Self::Spanish),
            b'i' => Some(Self::Italian),
            b'p' => Some(Self::Portuguese),
            b'f' => Some(Self::French),
            _ => None,
        }
    }

    /// Its BCP 47 tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Spanish => "es",
            Self::Italian => "it",
            Self::Portuguese => "pt",
            Self::French => "fr",
        }
    }
}

/// `text` in IPA, stress marks before vowels and `^` tying affricates.
pub fn phonemize(language: Language, text: &str) -> String {
    let tokens = text::split(text);
    let spoken = match language {
        Language::Spanish => spanish::read(&tokens),
        Language::Italian => italian::read(&tokens),
        Language::Portuguese => portuguese::read(&tokens),
        Language::French => french::read(&tokens),
    };
    render(&spoken)
}

/// Ties Kokoro writes as one letter; misaki's table for espeak.
const KOKORO: [(&str, &str); 11] = [
    ("a^ɪ", "I"),
    ("a^ʊ", "W"),
    ("d^z", "ʣ"),
    ("d^ʒ", "ʤ"),
    ("e^ɪ", "A"),
    ("o^ʊ", "O"),
    ("ə^ʊ", "Q"),
    ("s^s", "S"),
    ("t^s", "ʦ"),
    ("t^ʃ", "ʧ"),
    ("ɔ^ɪ", "Y"),
];

/// [`phonemize`], spelled as Kokoro's voices were trained to read it.
pub fn kokoro(language: Language, text: &str) -> String {
    let mut ipa = phonemize(language, text);
    for (tie, letter) in KOKORO {
        if ipa.contains(tie) {
            ipa = ipa.replace(tie, letter);
        }
    }
    ipa.retain(|ch| ch != '^' && ch != '-');
    ipa
}

/// How strongly a vowel is said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stress {
    None,
    Secondary,
    Primary,
}

/// One sound of a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sound {
    pub ipa: &'static str,
    pub vowel: bool,
    pub stress: Stress,
}

impl Sound {
    pub(crate) const fn consonant(ipa: &'static str) -> Self {
        Self {
            ipa,
            vowel: false,
            stress: Stress::None,
        }
    }

    pub(crate) const fn vowel(ipa: &'static str) -> Self {
        Self {
            ipa,
            vowel: true,
            stress: Stress::None,
        }
    }
}

/// A word's sounds or a mark, as said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Word(Vec<Sound>),
    Mark(char),
    Break,
}

/// A part, and whether space came before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Spoken {
    pub part: Part,
    pub space: bool,
}

fn render(spoken: &[Spoken]) -> String {
    let mut out = String::new();
    let mut owed = false;
    for Spoken { part, space } in spoken {
        if (*space || owed) && !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
        owed = *part == Part::Break;
        match part {
            Part::Word(sounds) => {
                for sound in sounds {
                    match sound.stress {
                        Stress::Primary => out.push('ˈ'),
                        Stress::Secondary => out.push('ˌ'),
                        Stress::None => {}
                    }
                    out.push_str(sound.ipa);
                }
            }
            Part::Break => {}
            Part::Mark('«') => out.push('“'),
            Part::Mark('»') => out.push('”'),
            Part::Mark(mark) => out.push(*mark),
        }
    }
    out
}
