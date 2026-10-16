//! Who explains words: a contract, and the answerers every build has.

use crate::answer::WordInfo;

/// Explains a word in context; runs on a worker, so it may block.
pub trait Explainer: Send + Sync {
    fn explain(&self, word: &str, context: &str) -> Result<WordInfo, Failure>;
}

/// Why no answer came; the card shows [`Why::message`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// This machine has nothing to explain with.
    NotHere,
    /// The model is there but not ready yet.
    NotReady,
    /// The model declined the text.
    Refused,
    TimedOut,
    Busy,
    /// An answer came that couldn't be read.
    BadAnswer,
}

impl Why {
    pub fn message(self) -> &'static str {
        match self {
            Self::NotHere => {
                "Explaining words needs Apple Intelligence, on a Mac with Apple silicon."
            }
            Self::NotReady => "The model isn't ready yet. Try again in a moment.",
            Self::Refused => "The model declined to explain this word.",
            Self::TimedOut => "The answer took too long.",
            Self::Busy => "The model is busy. Try again in a moment.",
            Self::BadAnswer => "The answer couldn't be read.",
        }
    }

    /// Whether asking again may help.
    pub fn retry(self) -> bool {
        !matches!(self, Self::NotHere | Self::Refused)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure {
    pub why: Why,
}

impl From<Why> for Failure {
    fn from(why: Why) -> Self {
        Self { why }
    }
}

/// Explains nothing: this build or machine has no model.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unavailable;

impl Explainer for Unavailable {
    fn explain(&self, _word: &str, _context: &str) -> Result<WordInfo, Failure> {
        Err(Why::NotHere.into())
    }
}

/// Answers from a script, for tests and shots; "fail" fails retryably.
#[derive(Clone, Copy, Debug, Default)]
pub struct Canned;

impl Explainer for Canned {
    fn explain(&self, word: &str, context: &str) -> Result<WordInfo, Failure> {
        if word.eq_ignore_ascii_case("fail") {
            return Err(Why::NotReady.into());
        }
        let lower = word.to_lowercase();
        let info = match lower.as_str() {
            "quick" => WordInfo {
                pos: "adjective".into(),
                meaning: "Moving fast, or done in a short time.".into(),
                synonyms: vec!["fast".into(), "swift".into(), "rapid".into()],
                usages: vec![
                    "She took a quick look at the map.".into(),
                    "The quick reply surprised him.".into(),
                ],
            },
            _ => WordInfo {
                pos: "word".into(),
                meaning: format!("What “{word}” means here, in plain words."),
                synonyms: vec![format!("{lower}-like"), "kindred".into()],
                usages: vec![format!("As in: {context}")],
            },
        };
        Ok(info.clamped())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stand_ins_answer_as_promised() {
        let none = Unavailable.explain("fox", "the fox");
        assert_eq!(none, Err(Why::NotHere.into()));
        assert!(!Why::NotHere.retry());
        let quick = Canned.explain("Quick", "The quick fox").map(|i| i.pos);
        assert_eq!(quick.as_deref(), Ok("adjective"));
        assert_eq!(Canned.explain("fail", ""), Err(Why::NotReady.into()));
        assert!(Why::NotReady.retry());
    }
}
