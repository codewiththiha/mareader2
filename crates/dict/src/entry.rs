//! One dictionary row as the reader sees it, and the order rows show in.

use crate::search::{WordMatch, fold};

/// One answer, whichever pack gave it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The headword, on the pack's source side.
    pub word: String,
    /// The pack's part-of-speech cell, as written.
    pub pos: Option<String>,
    /// The other side: a word or a short gloss.
    pub definition: String,
    pub romanization: Option<String>,
    /// Which sense of the headword this translates.
    pub sense: Option<String>,
    pub pack: &'static str,
    /// The English word between two packs, on a bridged answer.
    pub via: Option<String>,
    /// How the side that was asked fits the ask.
    pub fit: WordMatch,
    /// Asked from the definition side: the answer is the headword.
    pub flipped: bool,
}

impl Entry {
    /// The side that answers the ask.
    pub fn answer(&self) -> &str {
        if self.flipped {
            &self.word
        } else {
            &self.definition
        }
    }

    /// The side that was asked.
    pub fn asked(&self) -> &str {
        if self.flipped {
            &self.definition
        } else {
            &self.word
        }
    }

    /// The part of speech, readable: "v,n" is "verb · noun".
    pub fn tags(&self) -> String {
        let cell = self.pos.as_deref().unwrap_or_default();
        let tags: Vec<&str> = cell
            .split([',', ';', '/'])
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(tag)
            .collect();
        tags.join(" · ")
    }
}

/// A pack's tag spelled out; an unknown one is shown as written.
fn tag(raw: &str) -> &str {
    match raw.to_ascii_lowercase().as_str() {
        "n" | "ns" | "noun" => "noun",
        "name" | "proper_noun" => "name",
        "v" | "vs" | "verb" => "verb",
        "adj" | "adjs" | "adjective" => "adjective",
        "adv" | "advs" | "adverb" | "indefadv" => "adverb",
        "advpart" => "adverb particle",
        "pron" | "pers" | "indefpron" | "pronoun" => "pronoun",
        "possess" => "possessive",
        "det" | "indefdet" => "determiner",
        "article" | "indefart" => "article",
        "prep" => "preposition",
        "postp" => "postposition",
        "conj" => "conjunction",
        "interj" | "intj" => "interjection",
        "num" => "number",
        "pref" | "prefix" => "prefix",
        "suff" | "suffix" => "suffix",
        "symb" | "symbol" => "symbol",
        "abbr" => "abbreviation",
        "prep_phrase" => "prepositional phrase",
        "aux" => "auxiliary",
        "cop" => "copula",
        "phrase" => "phrase",
        "proverb" => "proverb",
        "particle" => "particle",
        "contraction" => "contraction",
        "counter" => "counter",
        "other" => "other",
        _ => raw,
    }
}

/// Tight fits first, pack order within a fit; repeated rows show once.
pub fn order(entries: &mut Vec<Entry>, limit: usize) {
    entries.sort_by_key(|e| e.fit);
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| {
        let key = (
            fold(&e.word),
            e.pos.clone(),
            fold(&e.definition),
            e.sense.clone(),
        );
        seen.insert(key)
    });
    entries.truncate(limit);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(word: &str, pos: &str, definition: &str, fit: WordMatch) -> Entry {
        Entry {
            word: word.into(),
            pos: Some(pos.into()),
            definition: definition.into(),
            romanization: None,
            sense: None,
            pack: "test",
            via: None,
            fit,
            flipped: false,
        }
    }

    #[test]
    fn tags_read_as_words_and_unknown_ones_stay() {
        assert_eq!(
            entry("a", "v,n", "", WordMatch::Exact).tags(),
            "verb · noun"
        );
        assert_eq!(entry("a", "Gerund", "", WordMatch::Exact).tags(), "Gerund");
        let mut bare = entry("a", "", "", WordMatch::Exact);
        assert_eq!(bare.tags(), "");
        bare.pos = None;
        assert_eq!(bare.tags(), "");
    }

    #[test]
    fn exact_rows_lead_and_repeats_show_once() {
        let mut rows = vec![
            entry("runner", "n", "a", WordMatch::Prefix),
            entry("run", "v", "b", WordMatch::Exact),
            entry("Run", "v", "b", WordMatch::Exact),
            entry("run", "n", "c", WordMatch::Exact),
        ];
        order(&mut rows, 10);
        let words: Vec<_> = rows.iter().map(|e| e.definition.as_str()).collect();
        assert_eq!(words, ["b", "c", "a"]);
        order(&mut rows, 1);
        assert_eq!(rows.len(), 1);
    }
}
