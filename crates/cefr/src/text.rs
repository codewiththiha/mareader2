//! Words in running text: where each is, and the keys it is known by.

use cefr::tags::ABBREVIATION_MAPPING;

/// A word's place in its text, in characters, end exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
}

/// No dataset word is longer; a longer run is never looked up.
pub const MAX_WORD_CHARS: usize = 32;

fn joiner(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '-')
}

/// Runs of ASCII letters; an apostrophe or hyphen joins two runs.
pub fn tokenize(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let letter = |i: usize| chars.get(i).is_some_and(char::is_ascii_alphabetic);
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !letter(i) {
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i + 1;
        while j < chars.len() {
            let joins = chars.get(j).copied().is_some_and(joiner) && letter(j + 1);
            if !letter(j) && !joins {
                break;
            }
            j += 1;
        }
        spans.push(Span { start, end: j });
        i = j;
    }
    spans
}

/// The key a word is stored under: lower case, one apostrophe.
pub fn normalize(word: &str) -> String {
    word.chars()
        .map(|c| if c == '\u{2019}' { '\'' } else { c })
        .flat_map(char::to_lowercase)
        .collect()
}

/// Every key that may make `word` hard: itself, a contraction's base,
/// and a compound's parts.
pub fn candidates(word: &str) -> Vec<String> {
    let key = normalize(word);
    let mut out = vec![key.clone()];
    let base = ABBREVIATION_MAPPING
        .iter()
        .find(|(suffix, _)| key.ends_with(suffix));
    if let Some((_, base)) = base {
        out.push((*base).to_owned());
    }
    if key.contains('-') {
        let parts = key
            .split('-')
            .filter(|p| p.chars().any(|c| c.is_ascii_alphabetic()));
        for part in parts {
            if !out.iter().any(|k| k == part) {
                out.push(part.to_owned());
            }
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let spans = tokenize(text);
        spans
            .iter()
            .map(|s| chars[s.start..s.end].iter().collect())
            .collect()
    }

    #[test]
    fn prose_splits_into_words_in_order() {
        let got = words("The palimpsest was scraped clean.");
        assert_eq!(got, ["The", "palimpsest", "was", "scraped", "clean"]);
    }

    #[test]
    fn apostrophes_and_hyphens_join_only_between_letters() {
        assert_eq!(words("don't stop"), ["don't", "stop"]);
        assert_eq!(words("it’s fine"), ["it’s", "fine"]);
        assert_eq!(words("the 'quoted' word"), ["the", "quoted", "word"]);
        assert_eq!(words("a well-known fact"), ["a", "well-known", "fact"]);
        assert_eq!(words("a -- b state- -start"), ["a", "b", "state", "start"]);
    }

    #[test]
    fn other_letters_digits_and_marks_end_a_word() {
        assert_eq!(
            words("café 16 horses x86_chip"),
            ["caf", "horses", "x", "chip"]
        );
        assert!(tokenize("中文").is_empty());
        // Spans count characters, not bytes.
        assert_eq!(tokenize("\u{1F600} sea"), [Span { start: 2, end: 5 }]);
    }

    #[test]
    fn a_word_is_known_by_its_base_and_parts_too() {
        assert_eq!(candidates("Palimpsest"), ["palimpsest"]);
        assert_eq!(candidates("Don’t"), ["don't", "not"]);
        assert_eq!(candidates("well-known"), ["well-known", "well", "known"]);
        assert_eq!(candidates("so-so"), ["so-so", "so"]);
    }
}
