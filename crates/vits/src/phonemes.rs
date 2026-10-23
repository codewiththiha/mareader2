//! misaki's phonemes respelled as espeak writes them, for voices trained on it.

/// The espeak voice a model's phonemes were written by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style {
    /// `en-us`: rhotic, long vowels marked.
    American,
    /// `en`: espeak's default English, non-rhotic.
    English,
    /// `en-gb-x-rp`: Received Pronunciation.
    Received,
}

impl Style {
    /// From a voice config's `espeak.voice`; `None` for languages not handled.
    pub fn from_espeak_voice(voice: &str) -> Option<Self> {
        match voice {
            "en-us" | "en_US" => Some(Self::American),
            "en" | "en-gb" | "en_GB" => Some(Self::English),
            "en-gb-x-rp" => Some(Self::Received),
            _ => None,
        }
    }

    /// Whether the British lexicon fits better than the American one.
    pub fn british(self) -> bool {
        self != Self::American
    }
}

#[derive(Clone, Copy)]
enum After {
    Any,
    /// The end of a word.
    End,
    In(&'static str),
    NotIn(&'static str),
    /// A vowel, a stress mark allowed before it.
    Vowel,
}

#[derive(Clone, Copy)]
struct Rule {
    from: &'static str,
    to: &'static str,
    after: After,
    after_stress_ok: bool,
}

const fn rule(from: &'static str, to: &'static str) -> Rule {
    Rule {
        from,
        to,
        after: After::Any,
        after_stress_ok: true,
    }
}

const fn when(from: &'static str, to: &'static str, after: After) -> Rule {
    Rule {
        from,
        to,
        after,
        after_stress_ok: true,
    }
}

const VOWELS: &str = "aeiouæɑɒɔəɛɜɪʊʌAIOWYQᵊᵻɚ";
const WORD_END: &str = " ,.!?;:—…";
const STRESS: &str = "ˈˌ";

// Add a rule only if it brings espeak's spelling of the lexicon closer.
const AMERICAN: &[Rule] = &[
    rule("ɜɹ", "ɜː"),
    rule("əɹ", "ɚ"),
    rule("ɑ", "ɑː"),
    when("ɔ", "ɔː", After::NotIn("ː")),
    rule("ˈi", "ˈiː"),
    rule("ˌi", "ˌiː"),
    rule("ˈu", "ˈuː"),
    rule("ˌu", "ˌuː"),
    when("ɚ", "ɚɹ", After::Vowel),
    rule("ˌʌ", "ʌ"),
    rule("nk", "ŋk"),
    rule("jəl", "jʊl"),
    rule("ləɾ", "lᵻɾ"),
    rule("səɾ", "sᵻɾ"),
    when("iː", "iːʲ", After::In("ɚɪᵻ")),
    when("i", "iʲ", After::In("ɚɪᵻ")),
    rule("ʲᵻ", "ʲɪ"),
];

const ENGLISH: &[Rule] = &[
    rule("ˌʌ", "ʌ"),
    rule("nk", "ŋk"),
    rule("ˈɪə", "ˈiə"),
    when("nɪs", "nəs", After::End),
    when("fʊl", "fəl", After::End),
    rule("ɛː", "eə"),
];

const RECEIVED: &[Rule] = &[
    when("ˈa", "ˈæ", After::NotIn("ɪʊ")),
    Rule {
        from: "i",
        to: "ɪ",
        after: After::End,
        after_stress_ok: false,
    },
    when("ə", "ɐ", After::End),
];

fn follows(after: After, rest: &[char]) -> bool {
    let next = rest.first().copied();
    match after {
        After::Any => true,
        After::End => next.is_none_or(|c| WORD_END.contains(c)),
        After::In(set) => next.is_some_and(|c| set.contains(c)),
        After::NotIn(set) => next.is_none_or(|c| !set.contains(c)),
        After::Vowel => {
            let skip = usize::from(next.is_some_and(|c| STRESS.contains(c)));
            rest.get(skip).is_some_and(|&c| VOWELS.contains(c))
        }
    }
}

/// One left-to-right pass, contexts read from the input as a regex would.
fn apply(text: Vec<char>, rule: &Rule) -> Vec<char> {
    let from: Vec<char> = rule.from.chars().collect();
    let mut out = Vec::with_capacity(text.len() + 8);
    let mut i = 0;
    while let Some(&c) = text.get(i) {
        let end = i + from.len();
        let hit = text.get(i..end) == Some(from.as_slice())
            && follows(rule.after, text.get(end..).unwrap_or_default())
            && (rule.after_stress_ok
                || i.checked_sub(1)
                    .and_then(|p| text.get(p))
                    .is_none_or(|&p| !STRESS.contains(p)));
        if hit {
            out.extend(rule.to.chars());
            i = end;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn respell(c: char, out: &mut String) {
    let s = match c {
        'A' => "eɪ",
        'I' => "aɪ",
        'O' => "oʊ",
        'Q' => "əʊ",
        'W' => "aʊ",
        'Y' => "ɔɪ",
        'ʤ' => "dʒ",
        'ʧ' => "tʃ",
        'ᵊ' => "ə",
        '—' => ",",
        '…' => ".",
        _ => {
            out.push(c);
            return;
        }
    };
    out.push_str(s);
}

/// misaki's phonemes for one sentence, in `style`'s spelling.
pub fn espeak_like(misaki: &str, style: Style) -> String {
    let rules: &[&[Rule]] = match style {
        Style::American => &[AMERICAN],
        Style::English => &[ENGLISH],
        Style::Received => &[ENGLISH, RECEIVED],
    };
    let text = rules
        .iter()
        .flat_map(|set| set.iter())
        .fold(misaki.chars().collect(), apply);
    let mut out = String::with_capacity(text.len() * 2);
    for c in text {
        respell(c, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected spellings come from the rules as measured against espeak.
    #[test]
    fn american_marks_length_and_r_colouring() {
        let misaki = "həlˈO wˈɜɹld wˈɔɾəɹ kəɹˈɪɹ ɹˈɛɡjələɹ θˈɪŋkɪŋ ɹˈɛdi kˈɛɹi Idˈiə \
                      hˈæpinəs, bjˈuɾəfəl kˈɑɹ fˈɛɹ stˈɑɹt sˈɪɾi sˈOfə bˈæŋk nˈAʧəɹ hˈɜɹid \
                      mˈɛɾᵊl ɪnˈʌf tˈY hˈWs ʤˈʌʤ ʧˈɜɹʧ.";
        let want = "həlˈoʊ wˈɜːld wˈɔːɾɚ kɚɹˈɪɹ ɹˈɛɡjʊlɚ θˈɪŋkɪŋ ɹˈɛdi kˈɛɹi aɪdˈiːə \
                    hˈæpinəs, bjˈuːɾəfəl kˈɑːɹ fˈɛɹ stˈɑːɹt sˈɪɾi sˈoʊfə bˈæŋk nˈeɪtʃɚ hˈɜːid \
                    mˈɛɾəl ɪnˈʌf tˈɔɪ hˈaʊs dʒˈʌdʒ tʃˈɜːtʃ.";
        assert_eq!(espeak_like(misaki, Style::American), want);
    }

    #[test]
    fn british_styles_differ_where_espeak_does() {
        let misaki = "həlˈQ wˈɜːld wˈɔːtə kəɹˈɪə ɹˈɛɡjʊlə θˈɪŋkɪŋ ɹˈɛdi kˈaɹi Idˈɪə hˈapɪnɪs, \
                      bjˈuːtɪfʊl kˈɑː fˈɛː stˈɑːt sˈɪti sˈQfə bˈaŋk nˈAʧə hˈʌɹɪd mˈɛtᵊl ɪnˈʌf \
                      tˈY hˈWs ʤˈʌʤ ʧˈɜːʧ.";
        let english = "həlˈəʊ wˈɜːld wˈɔːtə kəɹˈiə ɹˈɛɡjʊlə θˈɪŋkɪŋ ɹˈɛdi kˈaɹi aɪdˈiə hˈapɪnəs, \
                       bjˈuːtɪfəl kˈɑː fˈeə stˈɑːt sˈɪti sˈəʊfə bˈaŋk nˈeɪtʃə hˈʌɹɪd mˈɛtəl ɪnˈʌf \
                       tˈɔɪ hˈaʊs dʒˈʌdʒ tʃˈɜːtʃ.";
        let received = "həlˈəʊ wˈɜːld wˈɔːtɐ kəɹˈiɐ ɹˈɛɡjʊlɐ θˈɪŋkɪŋ ɹˈɛdɪ kˈæɹɪ aɪdˈiɐ hˈæpɪnəs, \
                        bjˈuːtɪfəl kˈɑː fˈeɐ stˈɑːt sˈɪtɪ sˈəʊfɐ bˈæŋk nˈeɪtʃɐ hˈʌɹɪd mˈɛtəl ɪnˈʌf \
                        tˈɔɪ hˈaʊs dʒˈʌdʒ tʃˈɜːtʃ.";
        assert_eq!(espeak_like(misaki, Style::English), english);
        assert_eq!(espeak_like(misaki, Style::Received), received);
    }

    #[test]
    fn voices_name_their_style() {
        assert_eq!(Style::from_espeak_voice("en"), Some(Style::English));
        assert_eq!(Style::from_espeak_voice("en-us"), Some(Style::American));
        assert_eq!(
            Style::from_espeak_voice("en-gb-x-rp"),
            Some(Style::Received)
        );
        assert_eq!(Style::from_espeak_voice("de"), None);
        assert!(!Style::American.british());
    }
}
