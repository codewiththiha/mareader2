//! misaki's pipeline, run from the last word so each knows what follows.

use std::collections::HashMap;

use fancy_regex::Regex;

use crate::lexicon::{Lexicon, TokenContext, currency_units};
use crate::text::{
    PRIMARY, VOWELS, apply_stress_opt, is_digit, isalpha, last_char, len, slice, stress_weight,
};

const SUBTOKEN_JUNKS: &str = "',-._‘’/";
const PUNCTS: &str = ";:,.!?—…\"“”";
const NON_QUOTE_PUNCTS: &str = ";:,.!?—…";
const PUNCT_TAGS: [&str; 10] = [
    ".", ",", "-LRB-", "-RRB-", "``", "\"\"", "''", ":", "$", "#",
];
const CONSONANTS: &str = "bdfhjklmnpstvwzðŋɡɹɾʃʒʤʧθ";

fn punct_tag_phonemes(tag: &str) -> Option<&'static str> {
    match tag {
        "-LRB-" => Some("("),
        "-RRB-" => Some(")"),
        "``" => Some("\u{201c}"),
        "\"\"" | "''" => Some("\u{201d}"),
        _ => None,
    }
}

/// The patterns that split text into words and markup.
pub(crate) struct Patterns {
    subtoken: Regex,
    link: Regex,
}

impl Patterns {
    pub(crate) fn new() -> Result<Self, crate::Error> {
        let compile =
            |p: &str| Regex::new(p).map_err(|e| crate::Error::Data(format!("pattern: {e}")));
        Ok(Self {
            subtoken: compile(concat!(
                r"^['‘’]+|\p{Lu}(?=\p{Lu}\p{Ll})|(?:^-)?(?:\d?[,.]?\d)+|[-_]+|['‘’]{2,}",
                r"|\p{L}*?(?:['‘’]\p{L})*?\p{Ll}(?=\p{Lu})|\p{L}+(?:['‘’]\p{L})*|[^-_\p{L}'‘’\d]|['‘’]+$",
            ))?,
            link: compile(r"\[([^\]]+)\]\(([^\)]*)\)")?,
        })
    }
}

/// misaki's `MToken` with its `_` extension fields flattened in.
#[derive(Debug, Clone, Default)]
pub(crate) struct MToken {
    pub text: String,
    pub tag: String,
    pub whitespace: String,
    pub phonemes: Option<String>,
    pub is_head: bool,
    pub alias: Option<String>,
    pub stress: Option<f64>,
    pub currency: Option<String>,
    pub num_flags: String,
    pub prespace: bool,
}

/// A word after retokenizing: one token, or pieces written together ("don't").
enum Word {
    One(MToken),
    Many(Vec<MToken>),
}

/// A `[text](feature)` annotation from the input, per misaki's markup.
#[derive(Debug, Clone)]
enum Feature {
    Stress(f64),
    Phonemes(String),
    NumFlags(String),
}

pub(crate) fn merge_tokens(tokens: &[MToken], unk: Option<&str>) -> MToken {
    let stresses: Vec<f64> = tokens.iter().filter_map(|t| t.stress).collect();
    let stress = match stresses.first() {
        Some(&s) if stresses.iter().all(|&x| x == s) => Some(s),
        _ => None,
    };
    let currency = tokens.iter().filter_map(|t| t.currency.clone()).max();
    let phonemes = unk.map(|unk| {
        let mut ps = String::new();
        for tk in tokens {
            if tk.prespace
                && !ps.is_empty()
                && !last_char(&ps).is_some_and(char::is_whitespace)
                && tk.phonemes.as_deref().is_some_and(|p| !p.is_empty())
            {
                ps.push(' ');
            }
            ps.push_str(tk.phonemes.as_deref().unwrap_or(unk));
        }
        ps
    });
    let mut text = String::new();
    for (i, tk) in tokens.iter().enumerate() {
        text.push_str(&tk.text);
        if i + 1 < tokens.len() {
            text.push_str(&tk.whitespace);
        }
    }
    // The tag of the most "capitalised" token, first among equals.
    let weight = |t: &MToken| {
        t.text
            .chars()
            .map(|c| {
                if c.to_lowercase().eq(std::iter::once(c)) {
                    1
                } else {
                    2
                }
            })
            .sum::<usize>()
    };
    let tag = tokens.iter().fold(None::<&MToken>, |best, t| match best {
        Some(b) if weight(b) >= weight(t) => Some(b),
        _ => Some(t),
    });
    let mut num_flags: Vec<char> = tokens.iter().flat_map(|t| t.num_flags.chars()).collect();
    num_flags.sort_unstable();
    num_flags.dedup();
    MToken {
        text,
        tag: tag.map(|t| t.tag.clone()).unwrap_or_default(),
        whitespace: tokens
            .last()
            .map(|t| t.whitespace.clone())
            .unwrap_or_default(),
        phonemes,
        is_head: tokens.first().is_some_and(|t| t.is_head),
        alias: None,
        stress,
        currency,
        num_flags: num_flags.into_iter().collect(),
        prespace: tokens.first().is_some_and(|t| t.prespace),
    }
}

/// Strips `[text](feature)` markup: the text, its words, features by word.
fn preprocess(text: &str, link: &Regex) -> (String, Vec<String>, HashMap<usize, Feature>) {
    let text = text.trim_start();
    let mut result = String::new();
    let mut words: Vec<String> = Vec::new();
    let mut features = HashMap::new();
    let mut last_end = 0;
    for caps in link.captures_iter(text).filter_map(Result::ok) {
        let Some(whole) = caps.get(0) else {
            continue;
        };
        let (inner, f) = (
            caps.get(1).map_or("", |m| m.as_str()),
            caps.get(2).map_or("", |m| m.as_str()),
        );
        let before = slice(text, last_end..whole.start());
        result.push_str(before);
        words.extend(before.split_whitespace().map(str::to_owned));
        let signless = f.strip_prefix(['-', '+']).unwrap_or(f);
        let feature = if is_digit(signless) {
            f.parse::<i64>().ok().map(|n| Feature::Stress(n as f64))
        } else if f == "0.5" || f == "+0.5" {
            Some(Feature::Stress(0.5))
        } else if f == "-0.5" {
            Some(Feature::Stress(-0.5))
        } else if len(f) > 1 && f.starts_with('/') && f.ends_with('/') {
            Some(Feature::Phonemes(
                slice(f, 1..).trim_end_matches('/').to_owned(),
            ))
        } else if len(f) > 1 && f.starts_with('#') && f.ends_with('#') {
            Some(Feature::NumFlags(
                slice(f, 1..).trim_end_matches('#').to_owned(),
            ))
        } else {
            None
        };
        if let Some(feature) = feature {
            features.insert(words.len(), feature);
        }
        result.push_str(inner);
        words.push(inner.to_owned());
        last_end = whole.end();
    }
    if last_end < text.len() {
        let after = slice(text, last_end..);
        result.push_str(after);
        words.extend(after.split_whitespace().map(str::to_owned));
    }
    (result, words, features)
}

/// Each token's word, matched with whitespace removed, as spaCy aligns.
fn align(words: &[String], tokens: &[MToken]) -> Vec<Option<usize>> {
    let mut owner = Vec::new();
    for (w, word) in words.iter().enumerate() {
        owner.extend(word.chars().filter(|c| !c.is_whitespace()).map(|_| w));
    }
    let mut at = 0;
    tokens
        .iter()
        .map(|t| {
            let n = t.text.chars().filter(|c| !c.is_whitespace()).count();
            let w = if n == 0 { None } else { owner.get(at).copied() };
            at += n;
            w
        })
        .collect()
}

fn apply_features(tokens: &mut [MToken], words: &[String], features: &HashMap<usize, Feature>) {
    if features.is_empty() {
        return;
    }
    let owners = align(words, tokens);
    for (&k, feature) in features {
        let aligned = owners
            .iter()
            .enumerate()
            .filter(|(_, o)| **o == Some(k))
            .map(|(j, _)| j);
        for (i, j) in aligned.enumerate() {
            let Some(tk) = tokens.get_mut(j) else {
                continue;
            };
            match feature {
                Feature::Stress(s) => tk.stress = Some(*s),
                Feature::Phonemes(ps) => {
                    tk.is_head = i == 0;
                    tk.phonemes = Some(if i == 0 { ps.clone() } else { String::new() });
                }
                Feature::NumFlags(flags) => tk.num_flags = flags.clone(),
            }
        }
    }
}

fn fold_left(tokens: Vec<MToken>, unk: &str) -> Vec<MToken> {
    let mut result: Vec<MToken> = Vec::with_capacity(tokens.len());
    for tk in tokens {
        match result.pop() {
            Some(prev) if !tk.is_head => result.push(merge_tokens(&[prev, tk], Some(unk))),
            Some(prev) => {
                result.push(prev);
                result.push(tk);
            }
            None => result.push(tk),
        }
    }
    result
}

fn subtokenize<'a>(text: &'a str, subtoken: &Regex) -> Vec<&'a str> {
    subtoken
        .find_iter(text)
        .filter_map(Result::ok)
        .map(|m| m.as_str())
        .collect()
}

fn retokenize(tokens: Vec<MToken>, subtoken: &Regex) -> Vec<Word> {
    let tags: Vec<String> = tokens.iter().map(|t| t.tag.clone()).collect();
    let mut words: Vec<Word> = Vec::new();
    let mut currency: Option<String> = None;
    for (i, token) in tokens.into_iter().enumerate() {
        let mut tks: Vec<MToken> = if token.alias.is_none() && token.phonemes.is_none() {
            subtokenize(&token.text, subtoken)
                .into_iter()
                .map(|t| MToken {
                    text: t.to_owned(),
                    tag: token.tag.clone(),
                    whitespace: String::new(),
                    is_head: true,
                    num_flags: token.num_flags.clone(),
                    stress: token.stress,
                    ..MToken::default()
                })
                .collect()
        } else {
            vec![token.clone()]
        };
        if let Some(last) = tks.last_mut() {
            last.whitespace = token.whitespace.clone();
        }
        let n = tks.len();
        let texts: Vec<String> = tks.iter().map(|t| t.text.clone()).collect();
        for (j, mut tk) in tks.into_iter().enumerate() {
            if tk.alias.is_some() || tk.phonemes.is_some() {
            } else if tk.tag == "$" && currency_units(&tk.text).is_some() {
                currency = Some(tk.text.clone());
                tk.phonemes = Some(String::new());
            } else if tk.tag == ":" && (tk.text == "-" || tk.text == "–") {
                tk.phonemes = Some("—".to_owned());
            } else if PUNCT_TAGS.contains(&tk.tag.as_str())
                && !tk.text.chars().all(|c| c.is_ascii_alphabetic())
            {
                tk.phonemes = Some(match punct_tag_phonemes(&tk.tag) {
                    Some(p) => p.to_owned(),
                    None => tk.text.chars().filter(|c| PUNCTS.contains(*c)).collect(),
                });
            } else if currency.is_some() {
                if tk.tag != "CD" {
                    currency = None;
                } else if j + 1 == n && tags.get(i + 1).is_none_or(|t| t != "CD") {
                    tk.currency = currency.clone();
                }
            } else if 0 < j && j + 1 < n && tk.text == "2" {
                let before = texts.get(j - 1).and_then(|t| last_char(t));
                let after = texts.get(j + 1).and_then(|t| t.chars().next());
                let around: String = before.into_iter().chain(after).collect();
                if isalpha(&around) {
                    tk.alias = Some("to".to_owned());
                }
            }

            if tk.alias.is_some() || tk.phonemes.is_some() {
                words.push(Word::One(tk));
            } else if let Some(Word::Many(group)) = words.last_mut().filter(
                |w| matches!(w, Word::Many(g) if g.last().is_some_and(|t| t.whitespace.is_empty())),
            ) {
                tk.is_head = false;
                group.push(tk);
            } else if tk.whitespace.is_empty() {
                words.push(Word::Many(vec![tk]));
            } else {
                words.push(Word::One(tk));
            }
        }
    }
    words
        .into_iter()
        .map(|w| match w {
            Word::Many(mut g) if g.len() == 1 => match g.pop() {
                Some(tk) => Word::One(tk),
                None => Word::Many(g),
            },
            other => other,
        })
        .collect()
}

fn token_context(ctx: TokenContext, ps: Option<&str>, token: &MToken) -> TokenContext {
    let mut vowel = ctx.future_vowel;
    if let Some(ps) = ps.filter(|p| !p.is_empty()) {
        let found = ps.chars().find(|&c| {
            VOWELS.contains(c) || CONSONANTS.contains(c) || NON_QUOTE_PUNCTS.contains(c)
        });
        if let Some(c) = found {
            vowel = if NON_QUOTE_PUNCTS.contains(c) {
                None
            } else {
                Some(VOWELS.contains(c))
            };
        }
    }
    let future_to = matches!(token.text.as_str(), "to" | "To")
        || (token.text == "TO" && matches!(token.tag.as_str(), "TO" | "IN"));
    TokenContext {
        future_vowel: vowel,
        future_to,
    }
}

fn resolve_tokens(tokens: &mut [MToken]) {
    let mut text = String::new();
    for (i, tk) in tokens.iter().enumerate() {
        text.push_str(&tk.text);
        if i + 1 < tokens.len() {
            text.push_str(&tk.whitespace);
        }
    }
    let kinds: std::collections::HashSet<u8> = text
        .chars()
        .filter(|c| !SUBTOKEN_JUNKS.contains(*c))
        .map(|c| {
            if c.is_alphabetic() {
                0
            } else if c.is_ascii_digit() {
                1
            } else {
                2
            }
        })
        .collect();
    let prespace = text.contains(' ') || text.contains('/') || kinds.len() > 1;
    let last = tokens.len().saturating_sub(1);
    for (i, tk) in tokens.iter_mut().enumerate() {
        if tk.phonemes.is_none() {
            if i == last
                && tk.text.chars().count() == 1
                && NON_QUOTE_PUNCTS.contains(tk.text.as_str())
            {
                tk.phonemes = Some(tk.text.clone());
            } else if tk.text.chars().all(|c| SUBTOKEN_JUNKS.contains(c)) {
                tk.phonemes = Some(String::new());
            }
        } else if i > 0 {
            tk.prespace = prespace;
        }
    }
    if prespace {
        return;
    }
    let mut indices: Vec<(bool, usize, usize)> = tokens
        .iter()
        .enumerate()
        .filter_map(|(i, tk)| {
            tk.phonemes
                .as_deref()
                .filter(|p| !p.is_empty())
                .map(|p| (p.contains(PRIMARY), stress_weight(p), i))
        })
        .collect();
    if let &[(_, _, first), (_, _, second)] = indices.as_slice()
        && tokens
            .get(first)
            .is_some_and(|t| t.text.chars().count() == 1)
    {
        demote(tokens, second);
        return;
    }
    let primaries = indices.iter().filter(|(b, _, _)| *b).count();
    if indices.len() < 2 || primaries <= indices.len().div_ceil(2) {
        return;
    }
    indices.sort();
    let half = indices.len() / 2;
    for &(_, _, i) in indices.iter().take(half) {
        demote(tokens, i);
    }
}

fn demote(tokens: &mut [MToken], i: usize) {
    if let Some(tk) = tokens.get_mut(i) {
        tk.phonemes = apply_stress_opt(tk.phonemes.take(), Some(-0.5));
    }
}

/// One token per spoken word; `unk` stands in for an unknown piece of one.
pub(crate) fn g2p(
    lexicon: &Lexicon,
    patterns: &Patterns,
    text: &str,
    tokenize: impl FnOnce(&str) -> Vec<MToken>,
    fallback: &dyn Fn(&str) -> Option<String>,
    unk: &str,
) -> Vec<MToken> {
    let (text, words, features) = preprocess(text, &patterns.link);
    let mut tokens = tokenize(&text);
    apply_features(&mut tokens, &words, &features);
    let tokens = fold_left(tokens, unk);
    let mut words = retokenize(tokens, &patterns.subtoken);

    let mut ctx = TokenContext::default();
    for w in words.iter_mut().rev() {
        match w {
            Word::One(tk) => {
                if tk.phonemes.is_none() {
                    tk.phonemes = lexicon.phonemize(tk, ctx);
                }
                if tk.phonemes.is_none() {
                    tk.phonemes = fallback(&tk.text);
                }
                ctx = token_context(ctx, tk.phonemes.as_deref(), tk);
            }
            Word::Many(group) => {
                let (mut left, mut right) = (0, group.len());
                let mut should_fallback = false;
                while left < right {
                    let span = group.get(left..right).unwrap_or_default();
                    let merged = if span
                        .iter()
                        .any(|t| t.alias.is_some() || t.phonemes.is_some())
                    {
                        None
                    } else {
                        Some(merge_tokens(span, None))
                    };
                    let ps = merged.as_ref().and_then(|tk| lexicon.phonemize(tk, ctx));
                    if let (Some(ps), Some(tk)) = (ps, merged.as_ref()) {
                        ctx = token_context(ctx, Some(&ps), tk);
                        for (k, x) in group.iter_mut().enumerate().take(right).skip(left) {
                            x.phonemes = Some(if k == left { ps.clone() } else { String::new() });
                        }
                        right = left;
                        left = 0;
                    } else if left + 1 < right {
                        left += 1;
                    } else {
                        right -= 1;
                        let Some(tk) = group.get_mut(right) else {
                            break;
                        };
                        if tk.phonemes.is_none() {
                            if tk.text.chars().all(|c| SUBTOKEN_JUNKS.contains(c)) {
                                tk.phonemes = Some(String::new());
                            } else {
                                should_fallback = true;
                                break;
                            }
                        }
                        left = 0;
                    }
                }
                if should_fallback {
                    let merged = merge_tokens(group, None);
                    let ps = fallback(&merged.text);
                    if let Some((first, rest)) = group.split_first_mut() {
                        first.phonemes = ps;
                        rest.iter_mut()
                            .for_each(|x| x.phonemes = Some(String::new()));
                    }
                } else {
                    resolve_tokens(group);
                }
            }
        }
    }

    words
        .into_iter()
        .map(|w| {
            let mut tk = match w {
                Word::One(tk) => tk,
                Word::Many(group) => merge_tokens(&group, Some(unk)),
            };
            // misaki's output notation: the flap as T, the glottal stop as t.
            tk.phonemes = tk.phonemes.map(|ps| ps.replace('ɾ', "T").replace('ʔ', "t"));
            tk
        })
        .collect()
}
