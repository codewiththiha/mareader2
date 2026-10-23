//! spaCy's English tokenizer, step for step, for misaki assumes its pieces.

use std::collections::HashMap;

use fancy_regex::Regex;

use crate::Error;
use crate::text::slice;

/// One token, and whether a single space followed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub text: String,
    pub space_after: bool,
}

/// The rules as exported from spaCy.
#[derive(serde::Deserialize)]
struct Rules {
    prefix: String,
    suffix: String,
    infix: String,
    url: String,
    special_cases: HashMap<String, Vec<String>>,
}

pub(crate) struct Tokenizer {
    prefix: Regex,
    suffix: Regex,
    infix: Regex,
    url: Regex,
    specials: HashMap<String, Vec<String>>,
    /// Special cases that affix splitting breaks apart, by their first piece.
    split_specials: HashMap<String, Vec<SplitSpecial>>,
}

/// A special case as it looks after affix splitting, and what it becomes.
struct SplitSpecial {
    pieces: Vec<String>,
    replacement: Vec<String>,
}

impl Tokenizer {
    pub(crate) fn from_json(json: &str) -> Result<Self, Error> {
        let rules: Rules =
            serde_json::from_str(json).map_err(|e| Error::Data(format!("tokenizer rules: {e}")))?;
        let mut tokenizer = Self {
            prefix: compile(&rules.prefix)?,
            suffix: compile(&rules.suffix)?,
            infix: compile(&rules.infix)?,
            url: compile(&rules.url)?,
            specials: rules.special_cases,
            split_specials: HashMap::new(),
        };
        // spaCy re-merges specials holding an affix or space, as they split.
        let mut split_specials: HashMap<String, Vec<SplitSpecial>> = HashMap::new();
        for (text, replacement) in &tokenizer.specials {
            let has_affix = text.contains(' ')
                || tokenizer.find_prefix(text) != 0
                || tokenizer.find_suffix(text) != 0
                || tokenizer.infix.find(text).ok().flatten().is_some();
            if !has_affix {
                continue;
            }
            let mut pieces = Vec::new();
            tokenizer.tokenize_span(text, &mut pieces, false);
            let pieces: Vec<String> = pieces.into_iter().map(|t| t.text).collect();
            if let Some(first) = pieces.first() {
                split_specials
                    .entry(first.clone())
                    .or_default()
                    .push(SplitSpecial {
                        pieces,
                        replacement: replacement.clone(),
                    });
            }
        }
        tokenizer.split_specials = split_specials;
        Ok(tokenizer)
    }

    pub(crate) fn tokenize(&self, text: &str) -> Vec<Token> {
        let mut tokens: Vec<Token> = Vec::new();
        let Some(first) = text.chars().next() else {
            return tokens;
        };
        // As str.split, but only a lone ' ' goes; longer runs become tokens.
        let mut in_ws = first.is_whitespace();
        let mut start = 0;
        for (i, c) in text.char_indices() {
            if c.is_whitespace() == in_ws {
                continue;
            }
            if start < i {
                self.tokenize_span(slice(text, start..i), &mut tokens, true);
            }
            if c == ' ' {
                if let Some(last) = tokens.last_mut() {
                    last.space_after = true;
                }
                start = i + 1;
            } else {
                start = i;
            }
            in_ws = !in_ws;
        }
        if start < text.len() {
            self.tokenize_span(slice(text, start..), &mut tokens, true);
            if let Some(last) = tokens.last_mut() {
                last.space_after = text.ends_with(' ') && !in_ws;
            }
        }
        self.merge_split_specials(tokens)
    }

    fn tokenize_span(&self, span: &str, out: &mut Vec<Token>, with_specials: bool) {
        if with_specials && self.push_special(span, out) {
            return;
        }
        let mut prefixes = Vec::new();
        let mut suffixes = Vec::new();
        let core = self.split_affixes(span, &mut prefixes, &mut suffixes, with_specials);
        out.extend(prefixes.into_iter().map(token));
        let special = !core.is_empty() && with_specials && self.push_special(core, out);
        if !core.is_empty() && !special {
            if self.url.is_match(core).unwrap_or(false) {
                out.push(token(core));
            } else {
                self.split_infixes(core, out);
            }
        }
        out.extend(suffixes.into_iter().rev().map(token));
    }

    /// Peels affixes until none comes off or a special case remains.
    fn split_affixes<'a>(
        &self,
        mut span: &'a str,
        prefixes: &mut Vec<&'a str>,
        suffixes: &mut Vec<&'a str>,
        with_specials: bool,
    ) -> &'a str {
        let mut last_len = usize::MAX;
        while !span.is_empty() && span.len() != last_len {
            if with_specials && self.specials.contains_key(span) {
                break;
            }
            last_len = span.len();
            let pre_len = self.find_prefix(span);
            if pre_len != 0 {
                let minus_pre = slice(span, pre_len..);
                if with_specials && !minus_pre.is_empty() && self.specials.contains_key(minus_pre) {
                    prefixes.push(slice(span, ..pre_len));
                    span = minus_pre;
                    break;
                }
            }
            let suf_len = self.find_suffix(slice(span, pre_len..));
            let suf_at = span.len().saturating_sub(suf_len);
            if suf_len != 0 {
                let minus_suf = slice(span, ..suf_at);
                if with_specials && !minus_suf.is_empty() && self.specials.contains_key(minus_suf) {
                    suffixes.push(slice(span, suf_at..));
                    span = minus_suf;
                    break;
                }
            }
            if pre_len != 0 && suf_len != 0 && pre_len + suf_len <= span.len() {
                prefixes.push(slice(span, ..pre_len));
                suffixes.push(slice(span, suf_at..));
                span = slice(span, pre_len..suf_at);
            } else if pre_len != 0 {
                prefixes.push(slice(span, ..pre_len));
                span = slice(span, pre_len..);
            } else if suf_len != 0 {
                suffixes.push(slice(span, suf_at..));
                span = slice(span, ..suf_at);
            }
            // spaCy checks special cases here even when they are off.
            if !span.is_empty() && self.specials.contains_key(span) {
                break;
            }
        }
        span
    }

    /// Re-merges broken specials ("bed):" ends in "):"), longest first.
    fn merge_split_specials(&self, tokens: Vec<Token>) -> Vec<Token> {
        let mut matches: Vec<(usize, usize, &[String])> = Vec::new();
        for (start, first) in tokens.iter().enumerate() {
            let Some(candidates) = self.split_specials.get(&first.text) else {
                continue;
            };
            for special in candidates {
                let end = start + special.pieces.len();
                let Some(window) = tokens.get(start..end) else {
                    continue;
                };
                let joined = window
                    .split_last()
                    .is_none_or(|(_, init)| init.iter().all(|t| !t.space_after));
                if joined
                    && window
                        .iter()
                        .zip(&special.pieces)
                        .all(|(t, p)| &t.text == p)
                {
                    matches.push((start, end, &special.replacement));
                }
            }
        }
        if matches.is_empty() {
            return tokens;
        }
        // Longest first, then the later start; both ends must be unclaimed.
        matches.sort_by_key(|&(start, end, _)| {
            (std::cmp::Reverse(end - start), std::cmp::Reverse(start))
        });
        let mut claimed = vec![false; tokens.len()];
        let mut chosen: Vec<(usize, usize, &[String])> = Vec::new();
        for (start, end, replacement) in matches {
            let taken = |k: usize| claimed.get(k).copied().unwrap_or(true);
            if !taken(start) && !taken(end.saturating_sub(1)) {
                claimed
                    .iter_mut()
                    .take(end)
                    .skip(start)
                    .for_each(|c| *c = true);
                chosen.push((start, end, replacement));
            }
        }
        chosen.sort_by_key(|&(start, ..)| start);

        let mut out = Vec::with_capacity(tokens.len());
        let mut chosen = chosen.into_iter().peekable();
        let mut i = 0;
        while let Some(current) = tokens.get(i) {
            match chosen.peek() {
                Some(&(start, end, replacement)) if start == i => {
                    out.extend(replacement.iter().map(|p| token(p)));
                    let space = tokens
                        .get(end.saturating_sub(1))
                        .is_some_and(|t| t.space_after);
                    if let Some(last) = out.last_mut() {
                        last.space_after = space;
                    }
                    chosen.next();
                    i = end;
                }
                _ => {
                    out.push(current.clone());
                    i += 1;
                }
            }
        }
        out
    }

    /// Splits on infixes, each kept as a token, as spaCy's `_attach_tokens`.
    fn split_infixes(&self, core: &str, out: &mut Vec<Token>) {
        let mut start = 0;
        for m in self.infix.find_iter(core).filter_map(Result::ok) {
            if m.start() == 0 {
                continue;
            }
            if m.start() != start {
                out.push(token(slice(core, start..m.start())));
            }
            if m.start() != m.end() {
                out.push(token(m.as_str()));
            }
            start = m.end();
        }
        if start < core.len() {
            out.push(token(slice(core, start..)));
        }
    }

    /// A URL or e-mail address by spaCy's pattern, as "docs.rs/x".
    pub(crate) fn is_url(&self, text: &str) -> bool {
        text.contains('.') && self.url.is_match(text).unwrap_or(false)
    }

    fn push_special(&self, span: &str, out: &mut Vec<Token>) -> bool {
        match self.specials.get(span) {
            Some(pieces) => {
                out.extend(pieces.iter().map(|p| token(p)));
                true
            }
            None => false,
        }
    }

    fn find_prefix(&self, s: &str) -> usize {
        self.prefix
            .find(s)
            .ok()
            .flatten()
            .map_or(0, |m| m.end() - m.start())
    }

    fn find_suffix(&self, s: &str) -> usize {
        self.suffix
            .find(s)
            .ok()
            .flatten()
            .map_or(0, |m| m.end() - m.start())
    }
}

fn token(text: &str) -> Token {
    Token {
        text: text.to_owned(),
        space_after: false,
    }
}

/// Python's `re` allows escaped quotes and spells out the default `(?u)`.
fn compile(pattern: &str) -> Result<Regex, Error> {
    let pattern = pattern
        .replace("(?u)", "")
        .replace("\\'", "'")
        .replace("\\\"", "\"");
    Regex::new(&pattern).map_err(|e| Error::Data(format!("tokenizer pattern: {e}")))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A few rules in spaCy's shape, enough to exercise each step.
    pub(crate) fn rules_json() -> String {
        serde_json::json!({
            "prefix": r#"^(?:\$|\(|\")"#,
            "suffix": r"(?:\.|,|\?|\)|:|%)$",
            "infix": r"(?<=[a-z])-(?=[a-z])",
            "url": r"^[a-z]+\.[a-z]+(?:/[a-z]+)?$",
            "special_cases": {"don't": ["do", "n't"], "Mr.": ["Mr."], "):": ["):"]},
        })
        .to_string()
    }

    fn tokenizer() -> Option<Tokenizer> {
        Tokenizer::from_json(&rules_json()).ok()
    }

    fn texts(text: &str) -> Vec<String> {
        let tokens = tokenizer().map(|t| t.tokenize(text)).unwrap_or_default();
        tokens.into_iter().map(|t| t.text).collect()
    }

    #[test]
    fn affixes_specials_infixes_and_urls_split_as_spacy() {
        let cases: &[(&str, &[&str])] = &[
            ("$12.50 a month", &["$", "12.50", "a", "month"]),
            ("(don't)", &["(", "do", "n't", ")"]),
            ("Mr. Smith, too.", &["Mr.", "Smith", ",", "too", "."]),
            ("one-on-one", &["one", "-", "on", "-", "one"]),
            ("see example.com/docs", &["see", "example.com/docs"]),
            ("bed):", &["bed", "):"]),
            ("bed) :", &["bed", ")", ":"]),
            ("", &[]),
        ];
        for (text, expected) in cases {
            assert_eq!(texts(text), *expected, "tokenizing {text:?}");
        }
    }

    #[test]
    fn single_spaces_are_recorded_and_longer_runs_kept() {
        let tokens = tokenizer().map(|t| t.tokenize("a  b ")).unwrap_or_default();
        let got: Vec<_> = tokens
            .iter()
            .map(|t| (t.text.as_str(), t.space_after))
            .collect();
        assert_eq!(got, [("a", true), (" ", false), ("b", true)]);
    }
}
