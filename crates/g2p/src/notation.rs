//! A word's phonemes from [`crate::G2p::phonemize`], written for people.

use crate::Dialect;

const PRIMARY: char = 'ˈ';
const SECONDARY: char = 'ˌ';
const LONG: char = 'ː';
/// Every vowel in misaki's set, both dialects.
const VOWELS: &str = "AIWYOQəiuɑɔɛɜɪʊʌæaɒᵻᵊ";

/// One syllable as respelled, and whether it takes the main stress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beat {
    pub text: String,
    pub stressed: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Syllable {
    onset: Vec<char>,
    /// The vowel, a British centring glide after it, and any length mark.
    nucleus: Vec<char>,
    coda: Vec<char>,
    stress: Option<char>,
}

impl Syllable {
    /// The vowels alone, without the length mark.
    fn core(&self) -> Vec<char> {
        self.nucleus
            .iter()
            .copied()
            .filter(|&c| c != LONG)
            .collect()
    }

    fn long(&self) -> bool {
        self.nucleus.contains(&LONG)
    }
}

fn is_vowel(c: char) -> bool {
    VOWELS.contains(c)
}

/// Whether English lets `cs` begin a syllable.
fn is_onset(cs: &[char]) -> bool {
    match cs {
        [] => true,
        [c] => *c != 'ŋ',
        [a, b] => matches!(
            (a, b),
            ('p' | 'b' | 'k' | 'ɡ' | 'f' | 's', 'l')
                | ('p' | 'b' | 't' | 'd' | 'k' | 'ɡ' | 'f' | 'θ' | 'ʃ', 'ɹ')
                | ('t' | 'd' | 'k' | 'ɡ' | 's' | 'θ', 'w')
                | (
                    'p' | 'b'
                        | 'k'
                        | 'ɡ'
                        | 'm'
                        | 'f'
                        | 'v'
                        | 'h'
                        | 'n'
                        | 'l'
                        | 't'
                        | 'd'
                        | 's'
                        | 'z'
                        | 'θ',
                    'j'
                )
                | ('s', 'p' | 't' | 'k' | 'm' | 'n' | 'f')
        ),
        ['s', a, b] => matches!(
            (a, b),
            ('p', 'l' | 'ɹ' | 'j') | ('t', 'ɹ' | 'j') | ('k', 'l' | 'ɹ' | 'w' | 'j')
        ),
        _ => false,
    }
}

/// Consonants between two vowels, split as coda then the longest onset.
fn split(run: &[char]) -> (Vec<char>, Vec<char>) {
    for at in 0..=run.len() {
        if let (Some(coda), Some(onset)) = (run.get(..at), run.get(at..))
            && is_onset(onset)
        {
            return (coda.to_vec(), onset.to_vec());
        }
    }
    (run.to_vec(), Vec::new())
}

/// `word`'s syllables: one per vowel, the consonants shared out between.
fn syllables(word: &str, dialect: Dialect) -> Vec<Syllable> {
    let mut out: Vec<Syllable> = Vec::new();
    let mut run: Vec<char> = Vec::new();
    let mut stress = None;
    for c in word.chars() {
        if c == PRIMARY || c == SECONDARY {
            stress = Some(c);
            continue;
        }
        let last = out.last_mut().filter(|_| run.is_empty());
        if c == LONG {
            if let Some(s) = last {
                s.nucleus.push(c);
            }
            continue;
        }
        if !is_vowel(c) {
            run.push(c);
            continue;
        }
        // British /ɪə/, /ʊə/ and /ɛə/ glide within one syllable.
        let glide = dialect == Dialect::British && c == 'ə';
        if let Some(s) = last
            && glide
            && matches!(s.nucleus.as_slice(), ['ɪ'] | ['ʊ'] | ['ɛ'])
        {
            s.nucleus.push(c);
            continue;
        }
        let onset = match out.last_mut() {
            Some(prev) => {
                let (coda, onset) = split(&run);
                prev.coda.extend(coda);
                onset
            }
            None => run.clone(),
        };
        run.clear();
        out.push(Syllable {
            onset,
            nucleus: vec![c],
            coda: Vec::new(),
            stress: stress.take(),
        });
    }
    match out.last_mut() {
        Some(last) => last.coda.extend(run),
        None if !run.is_empty() => out.push(Syllable {
            onset: run,
            ..Syllable::default()
        }),
        None => {}
    }
    out
}

/// A consonant spelled as English writes the sound.
fn spelled(c: char, to: &mut String) {
    let s = match c {
        'j' => "y",
        'ɡ' => "g",
        'ŋ' => "ng",
        'ɹ' => "r",
        'ʃ' => "sh",
        'ʒ' => "zh",
        'ð' => "dh",
        'θ' => "th",
        'ʤ' => "j",
        'ʧ' => "ch",
        // The American tap in "butter" sounds nearer a d.
        'ɾ' => "d",
        'ʔ' => "t",
        other => {
            to.push(other);
            return;
        }
    };
    to.push_str(s);
}

/// A vowel spelled out; true when it took the r after it in.
fn spelled_vowel(s: &Syllable, dialect: Dialect) -> (String, bool) {
    let core = s.core();
    let rhotic = dialect == Dialect::American && s.coda.first() == Some(&'ɹ');
    let with_r = match core.as_slice() {
        _ if !rhotic => None,
        ['ɑ'] => Some("aar"),
        ['ɔ'] | ['O'] => Some("or"),
        ['ɛ'] | ['A'] => Some("air"),
        ['ɪ'] | ['i'] => Some("eer"),
        ['ʊ'] | ['u'] => Some("oor"),
        ['ɜ'] => Some("ur"),
        ['ə'] | ['ᵊ'] => Some("r"),
        ['I'] => Some("ire"),
        ['W'] => Some("owr"),
        _ => None,
    };
    if let Some(text) = with_r {
        return (text.to_owned(), true);
    }
    let text = match core.as_slice() {
        ['ɪ', 'ə'] => "eer",
        ['ʊ', 'ə'] => "oor",
        ['ɛ', 'ə'] => "air",
        ['ɛ'] if s.long() => "air",
        ['ɜ'] => "ur",
        ['ə'] | ['ʌ'] => "uh",
        // The muted schwa of "pixel" goes unwritten.
        ['ᵊ'] => "",
        ['i'] => "ee",
        ['u'] => "oo",
        ['ɑ'] => "aa",
        ['ɔ'] => "aw",
        ['ɛ'] => "e",
        ['ɪ'] | ['ᵻ'] => "i",
        ['ʊ'] => "u",
        ['æ'] | ['a'] => "a",
        ['ɒ'] => "o",
        ['A'] => "ay",
        ['I'] => "ai",
        ['W'] => "ow",
        ['Y'] => "oy",
        ['O'] | ['Q'] => "oh",
        other => return (other.iter().collect(), false),
    };
    (text.to_owned(), false)
}

/// One word's phonemes spelled as they sound, a beat per syllable.
pub fn easy(word: &str, dialect: Dialect) -> Vec<Beat> {
    let all = syllables(word, dialect);
    let one = all.len() == 1;
    let beat = |s: &Syllable| {
        let mut text = String::new();
        s.onset.iter().for_each(|&c| spelled(c, &mut text));
        let (vowel, took_r) = spelled_vowel(s, dialect);
        text.push_str(&vowel);
        let coda = s.coda.get(usize::from(took_r)..).unwrap_or_default();
        coda.iter().for_each(|&c| spelled(c, &mut text));
        if text.is_empty() {
            text.push_str("uh");
        }
        Beat {
            text,
            stressed: !one && s.stress == Some(PRIMARY),
        }
    };
    all.iter().map(beat).collect()
}

fn ipa_consonant(c: char, to: &mut String) {
    match c {
        'ʤ' => to.push_str("dʒ"),
        'ʧ' => to.push_str("tʃ"),
        'ɹ' => to.push('r'),
        'ɾ' | 'ʔ' => to.push('t'),
        other => to.push(other),
    }
}

fn ipa_vowel(c: char, stressed: bool, dialect: Dialect, to: &mut String) {
    let american = dialect == Dialect::American;
    let s = match c {
        'A' => "eɪ",
        'I' => "aɪ",
        'W' => "aʊ",
        'Y' => "ɔɪ",
        'O' => "oʊ",
        'Q' => "əʊ",
        'ᵊ' => "ə",
        'ᵻ' => "ɪ",
        'a' => "æ",
        // American entries leave length out; dictionaries write it.
        'ɑ' if american => "ɑː",
        'ɔ' if american => "ɔː",
        'ɜ' if american => "ɜː",
        'i' if american && stressed => "iː",
        'u' if american && stressed => "uː",
        other => {
            to.push(other);
            return;
        }
    };
    to.push_str(s);
}

/// One word in IPA as dictionaries write it, stress before its syllable.
pub fn ipa(word: &str, dialect: Dialect) -> String {
    let all = syllables(word, dialect);
    let one = all.len() == 1;
    let mut out = String::new();
    for s in &all {
        if let Some(mark) = s.stress.filter(|_| !one) {
            out.push(mark);
        }
        s.onset.iter().for_each(|&c| ipa_consonant(c, &mut out));
        let stressed = one || s.stress.is_some();
        for &c in &s.nucleus {
            ipa_vowel(c, stressed, dialect, &mut out);
        }
        s.coda.iter().for_each(|&c| ipa_consonant(c, &mut out));
    }
    // A length mark the data gave on top of one added stays single.
    out.replace("ːː", "ː")
}

/// The phonemes as the voices read them, diphthongs written out in IPA.
pub fn raw(word: &str) -> String {
    let mut out = String::new();
    for c in word.chars() {
        match c {
            'ʤ' => out.push_str("dʒ"),
            'ʧ' => out.push_str("tʃ"),
            'A' => out.push_str("eɪ"),
            'I' => out.push_str("aɪ"),
            'Y' => out.push_str("ɔɪ"),
            'O' => out.push_str("oʊ"),
            'Q' => out.push_str("əʊ"),
            'W' => out.push_str("aʊ"),
            'ᵊ' => out.push('ə'),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;
    use Dialect::{American as Us, British as Uk};

    fn spelled(word: &str, dialect: Dialect) -> String {
        let beats = easy(word, dialect);
        let text = |b: &Beat| match b.stressed {
            true => b.text.to_uppercase(),
            false => b.text.clone(),
        };
        beats.iter().map(text).collect::<Vec<_>>().join("·")
    }

    #[test]
    fn yacht_reads_as_people_would_say_it() {
        assert_eq!(spelled("jˈɒt", Uk), "yot");
        assert_eq!(spelled("jˈɑt", Us), "yaat");
        assert_eq!(ipa("jˈɒt", Uk), "jɒt");
        assert_eq!(ipa("jˈɑt", Us), "jɑːt");
        assert_eq!(raw("jˈɑt"), "jˈɑt");
    }

    #[test]
    fn syllables_split_before_the_longest_onset_and_stress_is_bold() {
        assert_eq!(spelled("bˈʌɾəɹ", Us), "BUH·dr");
        assert_eq!(spelled("wˈɔːtə", Uk), "WAW·tuh");
        assert_eq!(spelled("pˈɪksᵊl", Us), "PIK·sl");
        assert_eq!(spelled("kˈɑɹ", Us), "kaar");
        assert_eq!(spelled("hˈɪə", Uk), "heer");
        assert_eq!(spelled("ˈizi", Us), "EE·zee");
        assert_eq!(spelled("kjˈut", Us), "kyoot");
        assert_eq!(spelled("pɹənˌʌnsiˈAʃən", Us), "pruh·nuhn·see·AY·shuhn");
        assert_eq!(spelled("ɪkstɹˈim", Us), "ik·STREEM");
    }

    #[test]
    fn dictionary_ipa_moves_stress_to_the_syllable_and_writes_length() {
        assert_eq!(ipa("bˈʌɾəɹ", Us), "ˈbʌtər");
        assert_eq!(ipa("wˈɔːtə", Uk), "ˈwɔːtə");
        assert_eq!(ipa("ˈizi", Us), "ˈiːzi");
        assert_eq!(ipa("kˈɑɹ", Us), "kɑːr");
        assert_eq!(ipa("pɹənˌʌnsiˈAʃən", Us), "prəˌnʌnsiˈeɪʃən");
        assert_eq!(ipa("ʤˈʌmp", Us), "dʒʌmp");
        assert_eq!(ipa("ɡˈQ", Uk), "ɡəʊ");
        assert_eq!(ipa("kˈat", Uk), "kæt");
        assert_eq!(raw("ʤˈʌmp"), "dʒˈʌmp");
    }

    #[test]
    fn odd_input_never_panics() {
        for word in ["", "ˈ", "ː", "st", "ŋŋ", "ˈːə", "❓"] {
            let _ = (easy(word, Us), ipa(word, Uk), raw(word));
        }
        assert_eq!(spelled("st", Us), "st");
    }
}
