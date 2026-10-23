//! misaki's English lexicon, branch for branch, for Kokoro learnt its quirks.

use std::collections::HashMap;

use fancy_regex::Regex;
use icu_normalizer::ComposingNormalizerBorrowed;

use crate::Error;
use crate::en::MToken;
use crate::numbers;
use crate::text::{
    PRIMARY, SECONDARY, apply_stress, apply_stress_opt, capitalize, drop_last, is_digit, is_lower,
    is_upper, isalpha, last_char, len, slice, tail,
};

const US_TAUS: &str = "AIOWYiuæɑəɛɪɹʊʌ";
const ORDINALS: [&str; 4] = ["st", "nd", "rd", "th"];
const CAP_STRESSES: (f64, f64) = (0.5, 2.0);

pub(crate) fn currency_units(symbol: &str) -> Option<(&'static str, &'static str)> {
    match symbol {
        "$" => Some(("dollar", "cent")),
        "£" => Some(("pound", "pence")),
        "€" => Some(("euro", "cent")),
        _ => None,
    }
}

fn symbol_word(word: &str) -> Option<&'static str> {
    match word {
        "%" => Some("percent"),
        "&" => Some("and"),
        "+" => Some("plus"),
        "@" => Some("at"),
        _ => None,
    }
}

fn add_symbol_word(word: &str) -> Option<&'static str> {
    match word {
        "." => Some("dot"),
        "/" => Some("slash"),
        _ => None,
    }
}

/// `ord(c) in LEXICON_ORDS`: an apostrophe, a hyphen or an ASCII letter.
fn is_lexicon_char(c: char) -> bool {
    c == '\'' || c == '-' || c.is_ascii_alphabetic()
}

/// The patterns the rules match words with.
struct Patterns {
    vs: Regex,
    number_suffix: Regex,
    not_letters: Regex,
    doubled_ing: Regex,
}

impl Patterns {
    fn new() -> Result<Self, Error> {
        let compile = |p: &str| Regex::new(p).map_err(|e| Error::Data(format!("pattern: {e}")));
        Ok(Self {
            vs: compile(r"(?i)^vs\.?$")?,
            number_suffix: compile(r"[a-z']+$")?,
            not_letters: compile(r"[^a-z]+")?,
            doubled_ing: compile(r"([bcdgklmnprstvxz])\1ing$|cking$")?,
        })
    }
}

/// One pronunciation, or one per tag (`DEFAULT`, `VERB`, even `"None"`).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub(crate) enum Entry {
    One(String),
    Tagged(HashMap<String, Option<String>>),
}

impl Entry {
    fn default_str(&self) -> Option<&str> {
        match self {
            Entry::One(s) => Some(s),
            Entry::Tagged(m) => m.get("DEFAULT").and_then(|v| v.as_deref()),
        }
    }

    fn tagged(&self, tag: &str) -> Option<&str> {
        match self {
            Entry::Tagged(m) => m.get(tag).and_then(|v| v.as_deref()),
            Entry::One(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TokenContext {
    pub future_vowel: Option<bool>,
    pub future_to: bool,
}

pub(crate) struct Lexicon {
    british: bool,
    golds: HashMap<String, Entry>,
    silvers: HashMap<String, Entry>,
    patterns: Patterns,
}

impl Lexicon {
    /// The gold and silver lexicons, as misaki's JSON files.
    pub(crate) fn from_json(british: bool, gold: &str, silver: &str) -> Result<Self, Error> {
        let parse = |json: &str| {
            serde_json::from_str::<HashMap<String, Entry>>(json)
                .map_err(|e| Error::Data(format!("lexicon: {e}")))
        };
        Ok(Self {
            british,
            golds: grow_dictionary(parse(gold)?),
            silvers: grow_dictionary(parse(silver)?),
            patterns: Patterns::new()?,
        })
    }

    fn gold_str(&self, word: &str) -> Option<String> {
        self.golds
            .get(word)
            .and_then(Entry::default_str)
            .map(str::to_owned)
    }

    /// Letter-by-letter reading, used by the spell-out fallback.
    pub(crate) fn spell(&self, word: &str) -> Option<String> {
        self.get_nnp(word)
    }

    /// Reads "9:30" or "config.toml" in pieces: numbers, words, ":" and ".".
    pub(crate) fn compose_fallback(
        &self,
        word: &str,
        word_fallback: &dyn Fn(&str) -> Option<String>,
    ) -> Option<String> {
        #[derive(PartialEq, Clone, Copy)]
        enum Kind {
            Letters,
            Digits,
            Other,
        }
        let kind = |c: char| {
            if c.is_alphabetic() || c == '\'' {
                Kind::Letters
            } else if c.is_ascii_digit() {
                Kind::Digits
            } else {
                Kind::Other
            }
        };
        if word.chars().all(|c| kind(c) == Kind::Letters || c == '-') {
            return word_fallback(word);
        }
        let mut runs: Vec<(Kind, String)> = Vec::new();
        for c in word.chars() {
            match runs.last_mut() {
                Some((k, run)) if *k == kind(c) && *k != Kind::Other => run.push(c),
                _ => runs.push((kind(c), c.to_string())),
            }
        }
        let mut out = String::new();
        for (k, run) in runs {
            let ps = match k {
                Kind::Letters => self
                    .get_word(&run, "NN", None, TokenContext::default())
                    .or_else(|| word_fallback(&run)),
                Kind::Digits => {
                    let mut words = Vec::new();
                    if run.len() > 1 && run.starts_with('0') {
                        for d in run.chars() {
                            self.extend_digits(&mut words, &d.to_string(), false, "")?;
                        }
                    } else {
                        self.extend_digits(&mut words, &run, true, "")?;
                    }
                    Some(words.join(" "))
                }
                Kind::Other if run == ":" || run == "." => Some(run),
                Kind::Other => None,
            };
            out.push_str(&ps.unwrap_or_default());
        }
        Some(out)
    }

    fn get_nnp(&self, word: &str) -> Option<String> {
        let mut ps = String::new();
        for c in word.chars().filter(|c| c.is_alphabetic()) {
            let upper: String = c.to_uppercase().collect();
            match self.golds.get(&upper) {
                Some(Entry::One(p)) => ps.push_str(p),
                _ => return None,
            }
        }
        let ps = apply_stress(&ps, Some(0.0));
        Some(match ps.rfind(SECONDARY) {
            Some(i) => format!(
                "{}{PRIMARY}{}",
                slice(&ps, ..i),
                slice(&ps, i + SECONDARY.len_utf8()..)
            ),
            None => ps,
        })
    }

    fn get_special_case(
        &self,
        word: &str,
        tag: &str,
        stress: Option<f64>,
        ctx: TokenContext,
    ) -> Option<String> {
        if tag == "ADD"
            && let Some(w) = add_symbol_word(word)
        {
            return self.lookup(w, None, Some(-0.5), Some(ctx));
        }
        if let Some(w) = symbol_word(word) {
            return self.lookup(w, None, None, Some(ctx));
        }
        let stripped = word.trim_matches('.');
        if stripped.contains('.')
            && isalpha(&word.replace('.', ""))
            && word.split('.').map(len).max().unwrap_or(0) < 3
        {
            return self.get_nnp(word);
        }
        match word {
            "a" | "A" => return Some(if tag == "DT" { "ɐ" } else { "ˈA" }.to_owned()),
            "am" | "Am" | "AM" => {
                if tag.starts_with("NN") {
                    return self.get_nnp(word);
                } else if ctx.future_vowel.is_none()
                    || word != "am"
                    || stress.is_some_and(|s| s > 0.0)
                {
                    return self.gold_str("am");
                }
                return Some("ɐm".to_owned());
            }
            "an" | "An" | "AN" => {
                if word == "AN" && tag.starts_with("NN") {
                    return self.get_nnp(word);
                }
                return Some("ɐn".to_owned());
            }
            "I" if tag == "PRP" => return Some(format!("{SECONDARY}I")),
            "by" | "By" | "BY" if parent_tag(Some(tag)) == Some("ADV") => {
                return Some("bˈI".to_owned());
            }
            _ => {}
        }
        if matches!(word, "to" | "To") || (word == "TO" && matches!(tag, "TO" | "IN")) {
            return match ctx.future_vowel {
                None => self.gold_str("to"),
                Some(false) => Some("tə".to_owned()),
                Some(true) => Some("tʊ".to_owned()),
            };
        }
        if matches!(word, "in" | "In") || (word == "IN" && tag != "NNP") {
            let stress = if ctx.future_vowel.is_none() || tag != "IN" {
                "ˈ"
            } else {
                ""
            };
            return Some(format!("{stress}ɪn"));
        }
        if matches!(word, "the" | "The") || (word == "THE" && tag == "DT") {
            return Some(
                if ctx.future_vowel == Some(true) {
                    "ði"
                } else {
                    "ðə"
                }
                .to_owned(),
            );
        }
        if tag == "IN" && self.patterns.vs.is_match(word).unwrap_or(false) {
            return self.lookup("versus", None, None, Some(ctx));
        }
        if matches!(word, "used" | "Used" | "USED") {
            let used = self.golds.get("used")?;
            let key = if matches!(tag, "VBD" | "JJ") && ctx.future_to {
                "VBD"
            } else {
                "DEFAULT"
            };
            return used.tagged(key).map(str::to_owned);
        }
        None
    }

    fn is_known(&self, word: &str) -> bool {
        if self.golds.contains_key(word)
            || symbol_word(word).is_some()
            || self.silvers.contains_key(word)
        {
            return true;
        }
        if !isalpha(word) || !word.chars().all(is_lexicon_char) {
            return false;
        }
        if len(word) == 1 {
            return true;
        }
        if is_upper(word) && self.golds.contains_key(&word.to_lowercase()) {
            return true;
        }
        is_upper(tail(word))
    }

    fn lookup(
        &self,
        word: &str,
        tag: Option<&str>,
        stress: Option<f64>,
        ctx: Option<TokenContext>,
    ) -> Option<String> {
        let mut word = word.to_owned();
        let mut is_nnp = false;
        if is_upper(&word) && !self.golds.contains_key(&word) {
            word = word.to_lowercase();
            is_nnp = tag == Some("NNP");
        }
        let mut entry = self.golds.get(&word);
        if entry.is_none() && !is_nnp {
            entry = self.silvers.get(&word);
        }
        let ps: Option<String> = match entry {
            None => None,
            Some(Entry::One(p)) => Some(p.clone()),
            Some(Entry::Tagged(m)) => {
                let key: Option<String> =
                    if ctx.is_some_and(|c| c.future_vowel.is_none()) && m.contains_key("None") {
                        Some("None".to_owned())
                    } else if tag.is_some_and(|t| m.contains_key(t)) {
                        tag.map(str::to_owned)
                    } else {
                        parent_tag(tag).map(str::to_owned)
                    };
                // `ps.get(tag, ps['DEFAULT'])`: a present key wins even
                // when its value is null.
                match key.as_deref().and_then(|k| m.get(k)) {
                    Some(v) => v.clone(),
                    None => m.get("DEFAULT").cloned().flatten(),
                }
            }
        };
        if (ps.is_none() || (is_nnp && !ps.as_deref().is_some_and(|p| p.contains(PRIMARY))))
            && let Some(nnp) = self.get_nnp(&word)
        {
            return Some(nnp);
        }
        apply_stress_opt(ps, stress)
    }

    fn suffix_s(&self, stem: Option<String>) -> Option<String> {
        let stem = stem.filter(|s| !s.is_empty())?;
        let last = last_char(&stem)?;
        Some(if "ptkfθ".contains(last) {
            stem + "s"
        } else if "szʃʒʧʤ".contains(last) {
            stem + if self.british { "ɪz" } else { "ᵻz" }
        } else {
            stem + "z"
        })
    }

    fn stem_s(
        &self,
        word: &str,
        tag: Option<&str>,
        stress: Option<f64>,
        ctx: Option<TokenContext>,
    ) -> Option<String> {
        if len(word) < 3 || !word.ends_with('s') {
            return None;
        }
        let stem = if !word.ends_with("ss") && self.is_known(drop_last(word, 1)) {
            drop_last(word, 1).to_owned()
        } else if (word.ends_with("'s")
            || (len(word) > 4 && word.ends_with("es") && !word.ends_with("ies")))
            && self.is_known(drop_last(word, 2))
        {
            drop_last(word, 2).to_owned()
        } else if len(word) > 4
            && word.ends_with("ies")
            && self.is_known(&format!("{}y", drop_last(word, 3)))
        {
            format!("{}y", drop_last(word, 3))
        } else {
            return None;
        };
        self.suffix_s(self.lookup(&stem, tag, stress, ctx))
    }

    fn suffix_ed(&self, stem: Option<String>) -> Option<String> {
        let stem = stem.filter(|s| !s.is_empty())?;
        let last = last_char(&stem)?;
        Some(if "pkfθʃsʧ".contains(last) {
            stem + "t"
        } else if last == 'd' {
            stem + if self.british { "ɪd" } else { "ᵻd" }
        } else if last != 't' {
            stem + "d"
        } else if self.british || len(&stem) < 2 {
            stem + "ɪd"
        } else if stem
            .chars()
            .rev()
            .nth(1)
            .is_some_and(|c| US_TAUS.contains(c))
        {
            format!("{}ɾᵻd", drop_last(&stem, 1))
        } else {
            stem + "ᵻd"
        })
    }

    fn stem_ed(
        &self,
        word: &str,
        tag: Option<&str>,
        stress: Option<f64>,
        ctx: Option<TokenContext>,
    ) -> Option<String> {
        if len(word) < 4 || !word.ends_with('d') {
            return None;
        }
        let stem = if !word.ends_with("dd") && self.is_known(drop_last(word, 1)) {
            drop_last(word, 1)
        } else if len(word) > 4
            && word.ends_with("ed")
            && !word.ends_with("eed")
            && self.is_known(drop_last(word, 2))
        {
            drop_last(word, 2)
        } else {
            return None;
        };
        self.suffix_ed(self.lookup(stem, tag, stress, ctx))
    }

    fn suffix_ing(&self, stem: Option<String>) -> Option<String> {
        let stem = stem.filter(|s| !s.is_empty())?;
        let last = last_char(&stem)?;
        if self.british {
            if "əː".contains(last) {
                return None;
            }
        } else if len(&stem) > 1
            && last == 't'
            && stem
                .chars()
                .rev()
                .nth(1)
                .is_some_and(|c| US_TAUS.contains(c))
        {
            return Some(format!("{}ɾɪŋ", drop_last(&stem, 1)));
        }
        Some(stem + "ɪŋ")
    }

    fn stem_ing(
        &self,
        word: &str,
        tag: Option<&str>,
        stress: Option<f64>,
        ctx: Option<TokenContext>,
    ) -> Option<String> {
        if len(word) < 5 || !word.ends_with("ing") {
            return None;
        }
        let stem = if len(word) > 5 && self.is_known(drop_last(word, 3)) {
            drop_last(word, 3).to_owned()
        } else if self.is_known(&format!("{}e", drop_last(word, 3))) {
            format!("{}e", drop_last(word, 3))
        } else if len(word) > 5
            && self.patterns.doubled_ing.is_match(word).unwrap_or(false)
            && self.is_known(drop_last(word, 4))
        {
            drop_last(word, 4).to_owned()
        } else {
            return None;
        };
        self.suffix_ing(self.lookup(&stem, tag, stress, ctx))
    }

    fn get_word(
        &self,
        word: &str,
        tag: &str,
        stress: Option<f64>,
        ctx: TokenContext,
    ) -> Option<String> {
        if let Some(ps) = self.get_special_case(word, tag, stress, ctx) {
            return Some(ps);
        }
        let wl = word.to_lowercase();
        let tag_opt = Some(tag);
        let mut word = word.to_owned();
        if len(&word) > 1
            && isalpha(&word.replace('\'', ""))
            && word != wl
            && (tag != "NNP" || len(&word) > 7)
            && !self.golds.contains_key(&word)
            && !self.silvers.contains_key(&word)
            && (is_upper(&word) || is_lower(tail(&word)))
            && (self.golds.contains_key(&wl)
                || self.silvers.contains_key(&wl)
                || self.stem_s(&wl, tag_opt, stress, Some(ctx)).is_some()
                || self.stem_ed(&wl, tag_opt, stress, Some(ctx)).is_some()
                || self.stem_ing(&wl, tag_opt, stress, Some(ctx)).is_some())
        {
            word = wl;
        }
        if self.is_known(&word) {
            return self.lookup(&word, tag_opt, stress, Some(ctx));
        }
        if word.ends_with("s'") {
            let possessive = format!("{}'s", drop_last(&word, 2));
            if self.is_known(&possessive) {
                return self.lookup(&possessive, tag_opt, stress, Some(ctx));
            }
        }
        if word.ends_with('\'') && self.is_known(drop_last(&word, 1)) {
            return self.lookup(drop_last(&word, 1), tag_opt, stress, Some(ctx));
        }
        if let Some(s) = self.stem_s(&word, tag_opt, stress, Some(ctx)) {
            return Some(s);
        }
        if let Some(ed) = self.stem_ed(&word, tag_opt, stress, Some(ctx)) {
            return Some(ed);
        }
        self.stem_ing(&word, tag_opt, Some(stress.unwrap_or(0.5)), Some(ctx))
    }

    fn get_number(
        &self,
        word: &str,
        currency: Option<&str>,
        is_head: bool,
        num_flags: &str,
    ) -> Option<String> {
        let suffix = self
            .patterns
            .number_suffix
            .find(word)
            .ok()
            .flatten()
            .map(|m| m.as_str().to_owned());
        let mut word = match &suffix {
            Some(s) => drop_last(word, len(s)).to_owned(),
            None => word.to_owned(),
        };
        let suffix = suffix.as_deref();
        let mut result: Vec<String> = Vec::new();
        if let Some(rest) = word.strip_prefix('-') {
            result.push(self.lookup("minus", None, None, None)?);
            word = rest.to_owned();
        }
        let is_ordinal = suffix.is_some_and(|s| ORDINALS.contains(&s));
        let currency_units = currency.and_then(currency_units);

        if is_digit(&word) && is_ordinal {
            self.extend_words(
                &mut result,
                &numbers::ordinal(word.parse().ok()?),
                true,
                num_flags,
            )?;
        } else if result.is_empty()
            && len(&word) == 4
            && currency_units.is_none()
            && is_digit(&word)
        {
            self.extend_words(
                &mut result,
                &numbers::year(word.parse().ok()?),
                true,
                num_flags,
            )?;
        } else if !is_head && !word.contains('.') {
            let num = word.replace(',', "");
            if num.starts_with('0') || len(&num) > 3 {
                for d in num.chars() {
                    self.extend_digits(&mut result, &d.to_string(), false, num_flags)?;
                }
            } else if len(&num) == 3 && !num.ends_with("00") {
                self.extend_digits(&mut result, slice(&num, ..1), true, num_flags)?;
                if slice(&num, 1..2) == "0" {
                    result.push(self.lookup("O", None, Some(-2.0), None)?);
                    self.extend_digits(&mut result, slice(&num, 2..), false, num_flags)?;
                } else {
                    self.extend_digits(&mut result, slice(&num, 1..), false, num_flags)?;
                }
            } else {
                self.extend_digits(&mut result, &num, true, num_flags)?;
            }
        } else if word.matches('.').count() > 1 || !is_head {
            let mut first = true;
            for num in word.replace(',', "").split('.') {
                if num.is_empty() {
                } else if num.starts_with('0')
                    || (len(num) != 2 && num.chars().skip(1).any(|c| c != '0'))
                {
                    for d in num.chars() {
                        self.extend_digits(&mut result, &d.to_string(), false, num_flags)?;
                    }
                } else {
                    self.extend_digits(&mut result, num, first, num_flags)?;
                }
                first = false;
            }
        } else if let (Some((major, minor)), true) = (currency_units, is_currency(&word)) {
            let plain = word.replace(',', "");
            let mut pairs: Vec<(u64, &str)> = plain
                .split('.')
                .zip([major, minor])
                .map(|(n, unit)| Some((if n.is_empty() { 0 } else { n.parse().ok()? }, unit)))
                .collect::<Option<_>>()?;
            if pairs.len() > 1 {
                if pairs.get(1).is_some_and(|p| p.0 == 0) {
                    pairs.truncate(1);
                } else if pairs.first().is_some_and(|p| p.0 == 0) {
                    pairs.remove(0);
                }
            }
            for (i, (num, unit)) in pairs.into_iter().enumerate() {
                if i > 0 {
                    result.push(self.lookup("and", None, None, None)?);
                }
                self.extend_digits(&mut result, &num.to_string(), i == 0, num_flags)?;
                let unit_ps = if num != 1 && unit != "pence" {
                    self.stem_s(&format!("{unit}s"), None, None, None)
                } else {
                    self.lookup(unit, None, None, None)
                };
                result.push(unit_ps?);
            }
        } else {
            let words = if is_digit(&word) {
                numbers::cardinal(word.parse().ok()?)
            } else if !word.contains('.') {
                let n: u64 = word.replace(',', "").parse().ok()?;
                if is_ordinal {
                    numbers::ordinal(n)
                } else {
                    numbers::cardinal(n)
                }
            } else {
                let plain = word.replace(',', "");
                if let Some(frac) = plain.strip_prefix('.') {
                    let digits: Vec<String> = frac
                        .chars()
                        .map(|d| d.to_digit(10).map(|d| numbers::cardinal(d.into())))
                        .collect::<Option<_>>()?;
                    format!("point {}", digits.join(" "))
                } else {
                    numbers::decimal(&plain)?
                }
            };
            self.extend_words(&mut result, &words, true, num_flags)?;
        }
        if result.is_empty() {
            return None;
        }
        let joined = result.join(" ");
        match suffix {
            Some("s" | "'s") => self.suffix_s(Some(joined)),
            Some("ed" | "'d") => self.suffix_ed(Some(joined)),
            Some("ing") => self.suffix_ing(Some(joined)),
            _ => Some(joined),
        }
    }

    /// `extend_num(num)` with `escape=False`: spell the digits, then add.
    fn extend_digits(
        &self,
        result: &mut Vec<String>,
        digits: &str,
        first: bool,
        num_flags: &str,
    ) -> Option<()> {
        self.extend_words(
            result,
            &numbers::cardinal(digits.parse().ok()?),
            first,
            num_flags,
        )
    }

    /// `extend_num(words, escape=True)`: phonemize each word, dropping "and"
    /// unless the `&` flag keeps it.
    fn extend_words(
        &self,
        result: &mut Vec<String>,
        words: &str,
        first: bool,
        num_flags: &str,
    ) -> Option<()> {
        let splits: Vec<&str> = self
            .patterns
            .not_letters
            .split(words)
            .filter_map(Result::ok)
            .collect();
        for (i, w) in splits.iter().enumerate() {
            if *w != "and" || num_flags.contains('&') {
                if first && i == 0 && splits.len() > 1 && *w == "one" && num_flags.contains('a') {
                    result.push("ə".to_owned());
                } else {
                    let stress = if *w == "point" { Some(-2.0) } else { None };
                    result.push(self.lookup(w, None, stress, None)?);
                }
            } else if num_flags.contains('n')
                && let Some(last) = result.last_mut()
            {
                last.push_str("ən");
            }
        }
        Some(())
    }

    fn append_currency(&self, ps: String, currency: Option<&str>) -> String {
        let Some(currency) = currency.filter(|c| !c.is_empty()) else {
            return ps;
        };
        match currency_units(currency)
            .and_then(|(major, _)| self.stem_s(&format!("{major}s"), None, None, None))
        {
            Some(unit) => format!("{ps} {unit}"),
            None => ps,
        }
    }

    /// `Lexicon.__call__`: phonemes for one token, or `None` for the
    /// fallback to handle.
    pub(crate) fn phonemize(&self, tk: &MToken, ctx: TokenContext) -> Option<String> {
        let source = tk
            .alias
            .as_deref()
            .unwrap_or(&tk.text)
            .replace(['‘', '’'], "'");
        let word: String = ComposingNormalizerBorrowed::new_nfkc()
            .normalize(&source)
            .chars()
            .map(numeric_if_needed)
            .collect();
        let stress = if is_lower(&word) {
            None
        } else if is_upper(&word) {
            Some(CAP_STRESSES.1)
        } else {
            Some(CAP_STRESSES.0)
        };
        if let Some(ps) = self.get_word(&word, &tk.tag, stress, ctx) {
            return Some(apply_stress(
                &self.append_currency(ps, tk.currency.as_deref()),
                tk.stress,
            ));
        }
        if is_number(&word, tk.is_head) {
            let ps = self.get_number(&word, tk.currency.as_deref(), tk.is_head, &tk.num_flags)?;
            return Some(apply_stress(&ps, tk.stress));
        }
        None
    }
}

fn parent_tag(tag: Option<&str>) -> Option<&str> {
    let t = tag?;
    Some(if t.starts_with("VB") {
        "VERB"
    } else if t.starts_with("NN") {
        "NOUN"
    } else if t.starts_with("ADV") || t.starts_with("RB") {
        "ADV"
    } else if t.starts_with("ADJ") || t.starts_with("JJ") {
        "ADJ"
    } else {
        t
    })
}

/// Adds each lowercase key capitalised and each capitalised key lowercased.
fn grow_dictionary(d: HashMap<String, Entry>) -> HashMap<String, Entry> {
    let mut grown: HashMap<String, Entry> = HashMap::with_capacity(d.len() * 2);
    for (k, v) in &d {
        if len(k) < 2 {
            continue;
        }
        if is_lower(k) {
            let cap = capitalize(k);
            if *k != cap {
                grown.insert(cap, v.clone());
            }
        } else if *k == capitalize(&k.to_lowercase()) {
            grown.insert(k.to_lowercase(), v.clone());
        }
    }
    grown.extend(d);
    grown
}

/// misaki compares cents to the integer 0, never true; only length counts.
fn is_currency(word: &str) -> bool {
    match word.matches('.').count() {
        0 => true,
        1 => word.split('.').nth(1).is_some_and(|cents| len(cents) < 3),
        _ => false,
    }
}

/// `Lexicon.numeric_if_needed`: a digit NFKC left alone (Arabic-Indic,
/// Devanagari, ...) becomes its ASCII value.
fn numeric_if_needed(c: char) -> char {
    if c.is_ascii_digit() || !c.is_numeric() {
        return c;
    }
    // Unicode decimal digits come in contiguous 0-9 runs.
    const ZEROS: [u32; 12] = [
        0x660, 0x6F0, 0x7C0, 0x966, 0x9E6, 0xA66, 0xAE6, 0xB66, 0xBE6, 0xC66, 0xCE6, 0xD66,
    ];
    let code = c as u32;
    ZEROS
        .iter()
        .find(|&&z| (z..z + 10).contains(&code))
        .and_then(|z| char::from_digit(code - z, 10))
        .unwrap_or(c)
}

fn is_number(word: &str, is_head: bool) -> bool {
    if !word.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    let mut word = word;
    for s in ["ing", "'d", "ed", "'s", "st", "nd", "rd", "th", "s"] {
        if let Some(stripped) = word.strip_suffix(s) {
            word = stripped;
            break;
        }
    }
    word.chars()
        .enumerate()
        .all(|(i, c)| c.is_ascii_digit() || c == ',' || c == '.' || (is_head && i == 0 && c == '-'))
}
