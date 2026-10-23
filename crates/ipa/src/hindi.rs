//! Hindi: Devanagari to sounds, the silent `a`, and stress by weight.

use crate::text::{Piece, Token};
use crate::{Part, Sound, Spoken, Stress};

/// Words said with no stress: postpositions and helpers.
const UNSTRESSED: &[&str] = &[
    "है",
    "हैं",
    "हूँ",
    "हो",
    "था",
    "थी",
    "थे",
    "थीं",
    "से",
    "में",
    "के",
    "की",
    "का",
    "को",
    "और",
    "मैं",
    "तू",
    "वे",
    "ये",
    "वो",
    "क्या",
];

/// Words whose stress is only secondary: pronouns and the like.
const WEAK: &[&str] = &[
    "वह",
    "यह",
    "हम",
    "तुम",
    "आप",
    "मुझे",
    "तुझे",
    "हमे",
    "तुम्हें",
    "इन्हें",
    "उन्हे",
    "आपको",
    "मेरा",
    "मेरी",
    "मेरे",
    "तेरा",
    "हमारा",
    "हमारी",
    "हमारे",
    "तुम्हारा",
    "तुम्हारी",
    "तुम्हारे",
    "आपका",
    "आपकी",
    "आपके",
    "उसका",
    "उसकी",
    "उसके",
    "उनका",
    "उनकी",
    "उनके",
    "इसका",
    "इनका",
    "रहा",
    "रही",
    "रहे",
    "तक",
    "अगर",
    "यदि",
    "बिना",
];

const VIRAMA: char = '\u{94D}';
const NUKTA: char = '\u{93C}';
const ANUSVARA: char = '\u{902}';
const CHANDRABINDU: char = '\u{901}';
const VISARGA: char = '\u{903}';
const DHA_DOT: char = '\u{95D}';

/// A written vowel other than the `a` consonants carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Vowel {
    Aa,
    I,
    Ii,
    U,
    Uu,
    R,
    E,
    Ai,
    O,
    Au,
    /// `ॉ`, the open `o` of English words.
    Ao,
    Ae,
}

/// A letter or sign, as its part in the word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    /// A consonant; a nukta form as its precomposed letter.
    Consonant(char),
    /// A vowel; `true` when written alone, not as a sign.
    Vowel(Vowel, bool),
    /// The `a` a consonant carries; `true` for a written `अ`.
    Schwa(bool),
    Anusvara,
    Chandrabindu,
    Visarga,
    /// A last `य` after an `a`, said `ɛj` as a syllable of its own.
    Ay,
}

/// Text with the danda as a full stop and Devanagari digits as ASCII.
pub(crate) fn normalise(text: &str) -> String {
    text.chars()
        .filter_map(|ch| match ch {
            '।' | '॥' => Some('.'),
            '०'..='९' => char::from_u32(u32::from(ch) - 0x966 + u32::from(b'0')),
            '\u{200b}' => Some(' '),
            '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}' => None,
            _ => Some(ch),
        })
        .collect()
}

/// Sentences of `tokens` as said in Hindi.
pub(crate) fn read(tokens: &[Token<'_>]) -> Vec<Spoken> {
    let mut spoken = Vec::new();
    for token in tokens {
        let space = token.space;
        let words: Vec<Vec<Sound>> = match token.piece {
            Piece::Mark(mark) => {
                spoken.push(Spoken {
                    part: Part::Mark(mark),
                    space,
                });
                continue;
            }
            Piece::Break => {
                spoken.push(Spoken {
                    part: Part::Break,
                    space,
                });
                continue;
            }
            Piece::Number(text) => number_words(text)
                .iter()
                .map(|joined| joined.iter().flat_map(|w| word_sounds(w)).collect())
                .collect(),
            Piece::Word(text) => vec![word_sounds(text)],
        };
        for (i, sounds) in words.into_iter().enumerate() {
            if sounds.is_empty() {
                continue;
            }
            spoken.push(Spoken {
                part: Part::Word(sounds),
                space: space || i > 0,
            });
        }
    }
    spoken
}

/// Latin letters, said as written; Hindi text rarely has them.
const LATIN: [&str; 26] = [
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z",
];

/// A word's sounds with their stress.
fn word_sounds(word: &str) -> Vec<Sound> {
    let items = items(word);
    if items.is_empty() {
        let latin = word.chars().flat_map(char::to_lowercase);
        let index = |c: char| "abcdefghijklmnopqrstuvwxyz".find(c);
        let letters = latin.filter_map(index).filter_map(|i| LATIN.get(i));
        return letters.map(|&ipa| Sound::consonant(ipa)).collect();
    }
    let class = if UNSTRESSED.contains(&word) {
        Class::Unstressed
    } else if WEAK.contains(&word) {
        Class::Weak
    } else {
        Class::Full
    };
    Word::new(items).sounds(class)
}

/// How much stress a word takes in a sentence.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Full,
    Weak,
    Unstressed,
}

/// `word` as letters and signs.
fn items(word: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let mut chars = word.chars().peekable();
    while let Some(ch) = chars.next() {
        if is_consonant(ch) {
            let mut c = ch;
            if chars.peek() == Some(&NUKTA) {
                chars.next();
                c = with_nukta(c);
            }
            out.push(Item::Consonant(c));
            let sign = chars.peek().copied().and_then(vowel_sign);
            if chars.peek() == Some(&VIRAMA) {
                chars.next();
            } else if let Some(vowel) = sign {
                chars.next();
                out.push(Item::Vowel(vowel, false));
            } else {
                out.push(Item::Schwa(false));
            }
            continue;
        }
        let item = match ch {
            'अ' => Item::Schwa(true),
            ANUSVARA => Item::Anusvara,
            CHANDRABINDU => Item::Chandrabindu,
            VISARGA => Item::Visarga,
            'ॐ' => {
                out.push(Item::Vowel(Vowel::O, true));
                Item::Consonant('म')
            }
            _ => match vowel_letter(ch) {
                Some(vowel) => Item::Vowel(vowel, true),
                None => continue,
            },
        };
        out.push(item);
    }
    let n = out.len();
    let ay = [Item::Consonant('य'), Item::Schwa(false)];
    if n >= 3 && out.ends_with(&ay) && matches!(out.get(n - 3), Some(Item::Schwa(_))) {
        out.truncate(n - 2);
        out.push(Item::Ay);
    }
    out
}

fn is_consonant(ch: char) -> bool {
    ('क'..='ह').contains(&ch) || ('\u{958}'..='\u{95f}').contains(&ch)
}

fn with_nukta(c: char) -> char {
    match c {
        'क' => '\u{958}',
        'ख' => '\u{959}',
        'ग' => '\u{95a}',
        'ज' => '\u{95b}',
        'ड' => '\u{95c}',
        'ढ' => DHA_DOT,
        'फ' => '\u{95e}',
        'य' => '\u{95f}',
        _ => c,
    }
}

fn vowel_sign(ch: char) -> Option<Vowel> {
    Some(match ch {
        'ा' => Vowel::Aa,
        'ि' => Vowel::I,
        'ी' => Vowel::Ii,
        'ु' => Vowel::U,
        'ू' => Vowel::Uu,
        'ृ' | 'ॄ' => Vowel::R,
        'े' => Vowel::E,
        'ै' => Vowel::Ai,
        'ो' => Vowel::O,
        'ौ' => Vowel::Au,
        'ॉ' => Vowel::Ao,
        'ॅ' => Vowel::Ae,
        _ => return None,
    })
}

fn vowel_letter(ch: char) -> Option<Vowel> {
    Some(match ch {
        'आ' => Vowel::Aa,
        'इ' => Vowel::I,
        'ई' => Vowel::Ii,
        'उ' => Vowel::U,
        'ऊ' => Vowel::Uu,
        'ऋ' | 'ॠ' => Vowel::R,
        'ए' => Vowel::E,
        'ऐ' => Vowel::Ai,
        'ओ' => Vowel::O,
        'औ' => Vowel::Au,
        'ऑ' => Vowel::Ao,
        'ऍ' => Vowel::Ae,
        _ => return None,
    })
}

/// A consonant's own sound.
fn consonant_ipa(c: char) -> &'static str {
    match c {
        'क' => "k",
        'ख' => "kʰ",
        'ग' => "ɡ",
        'घ' => "ɡʰ",
        'ङ' => "ŋ",
        'च' => "c",
        'छ' => "cʰ",
        'ज' => "ɟ",
        'झ' => "ɟʰ",
        'ञ' => "ɲ",
        'ट' => "ʈ",
        'ठ' => "ʈʰ",
        'ड' => "ɖ",
        'ढ' => "ɖʰ",
        'ण' => "ɳ",
        'त' => "t",
        'थ' => "tʰ",
        'द' => "d",
        'ध' => "dʰ",
        'न' | 'ऩ' => "n",
        'प' => "p",
        'फ' => "pʰ",
        'ब' => "b",
        'भ' => "bʰ",
        'म' => "m",
        'य' | '\u{95f}' => "j",
        'र' | 'ऱ' => "ɾ",
        'ल' | 'ळ' | 'ऴ' => "l",
        'व' => "ʋ",
        'श' => "ʃ",
        'ष' => "ʂ",
        'स' => "s",
        'ह' => "h",
        '\u{958}' => "q",
        '\u{959}' => "x",
        '\u{95a}' => "ɣ",
        '\u{95b}' => "z",
        '\u{95c}' => "r.",
        DHA_DOT => "r.h",
        '\u{95e}' => "f",
        _ => "",
    }
}

/// A stop said long when doubled or before `य`, written with `ː`.
fn long_stop(c: char) -> Option<&'static str> {
    Some(match c {
        'क' => "kː",
        'प' => "pː",
        'त' => "tː",
        'च' => "cː",
        'ज' => "ɟː",
        'द' => "dː",
        _ => return None,
    })
}

/// The nasal said for anusvara before `c`.
fn nasal_before(c: char) -> &'static str {
    match c {
        'क' | 'ख' | 'ग' | 'घ' | '\u{958}' | '\u{959}' | '\u{95a}' => "ŋ",
        'च' | 'छ' | 'ज' | 'झ' | 'य' => "ɲ",
        'ट' | 'ठ' | 'ड' | 'ढ' | 'ण' => "ɳ",
        'प' | 'फ' | 'ब' | 'भ' | 'म' => "m",
        _ => "n",
    }
}

/// Pairs said as one long consonant, as `त्त` or `त्थ`.
fn one_sound(first: char, then: char) -> bool {
    first == then && long_stop(first).is_some() || matches!((first, then), ('त', 'थ') | ('द', 'ध'))
}

fn is_nasal(c: char) -> bool {
    matches!(c, 'न' | 'म' | 'ण' | 'ङ' | 'ञ')
}

/// Whether a final `a` is said after the conjunct `first`+`last`.
fn final_cluster_keeps(first: char, last: char) -> bool {
    match last {
        'य' | 'र' => true,
        'ण' => true,
        'न' | 'म' | 'ल' => first != 'र' && !(is_nasal(first) && first != last),
        _ => false,
    }
}

/// The sound of a vowel with no nasal; `last` ends the word.
fn plain(vowel: Vowel, alone: bool, last: bool) -> &'static str {
    match vowel {
        Vowel::Aa => "aː",
        Vowel::I => "ɪ",
        Vowel::Ii if last => "i",
        Vowel::Ii => "iː",
        Vowel::U => "ʊ",
        Vowel::Uu if alone => "u",
        Vowel::Uu => "uː",
        Vowel::R => "ɪ",
        Vowel::E => "eː",
        Vowel::Ai => "ɛː",
        Vowel::O => "oː",
        Vowel::Au => "ɔː",
        Vowel::Ao => "ɔ",
        Vowel::Ae => "ɛ",
    }
}

/// The nasal vowel; espeak writes the tilde apart from the letter.
fn nasal(vowel: Vowel) -> &'static str {
    match vowel {
        Vowel::Aa => "a\u{303}",
        Vowel::I | Vowel::Ii | Vowel::R => "i\u{303}",
        Vowel::U | Vowel::Uu => "u\u{303}",
        Vowel::E => "e\u{303}ː",
        Vowel::Ai | Vowel::Ae => "ɛ\u{303}",
        Vowel::O => "o\u{303}",
        Vowel::Au | Vowel::Ao => "ɔ\u{303}",
    }
}

/// A sound of the word, and the syllable it is the vowel of.
#[derive(Clone, Copy)]
struct Segment {
    ipa: &'static str,
    syllable: Option<usize>,
}

/// A vowel of the word, said or silent.
#[derive(Clone, Copy)]
struct Syllable {
    item: usize,
    silent: bool,
    long: bool,
}

/// Placeholder for an `a` said `ʌ` stressed, `ə` not.
const SCHWA: &str = "ə";

struct Word {
    items: Vec<Item>,
    /// Per item: whether an `a` is said.
    said: Vec<Said>,
}

impl Word {
    fn new(items: Vec<Item>) -> Self {
        let vowels = items
            .iter()
            .filter(|i| matches!(i, Item::Vowel(..) | Item::Schwa(_) | Item::Ay))
            .count();
        let said = (0..items.len())
            .map(|i| match items.get(i) {
                Some(Item::Schwa(false)) => silent(&items, i, vowels),
                _ => Said::Yes,
            })
            .collect();
        Self { items, said }
    }

    fn at(&self, i: usize) -> Option<Item> {
        self.items.get(i).copied()
    }

    /// `ओं` or `एँ` ending a word after `आ`: said `w` or `j`.
    fn glide(&self, i: usize) -> Option<&'static str> {
        let after_aa = matches!(
            i.checked_sub(1).and_then(|p| self.at(p)),
            Some(Item::Vowel(Vowel::Aa, _))
        );
        if i + 2 != self.items.len() || !after_aa {
            return None;
        }
        match (self.at(i), self.at(i + 1)) {
            (Some(Item::Vowel(Vowel::O, true)), Some(Item::Anusvara)) => Some("w"),
            (Some(Item::Vowel(Vowel::E, true)), Some(Item::Chandrabindu)) => Some("j"),
            _ => None,
        }
    }

    fn sounds(&self, class: Class) -> Vec<Sound> {
        let (segments, syllables) = self.segments();
        let stress = self.stress(&syllables, class);
        let mut out = Vec::with_capacity(segments.len());
        for segment in &segments {
            let Some(k) = segment.syllable else {
                out.push(Sound::consonant(segment.ipa));
                continue;
            };
            let level = stress.get(k).copied().unwrap_or(Stress::None);
            let mut ipa = segment.ipa;
            let at = syllables.get(k).map_or(0, |s| s.item);
            let weak = self.said.get(at) == Some(&Said::Unstressed);
            if weak && level != Stress::Primary {
                continue;
            }
            if ipa == SCHWA {
                let alone = self.items.len() == 2;
                let first = at == 0 && self.at(0) == Some(Item::Schwa(true));
                let open = level == Stress::Primary && !alone || first;
                ipa = match self.at(1) {
                    Some(Item::Consonant('ह')) if first => "ɛ",
                    _ if open => "ʌ",
                    _ => "ə",
                };
            } else if ipa == "iː" && level == Stress::None {
                ipa = "i";
            }
            let mut sound = Sound::vowel(ipa);
            sound.stress = level;
            out.push(sound);
        }
        out
    }

    /// The word's sounds in order, and its vowels.
    fn segments(&self) -> (Vec<Segment>, Vec<Syllable>) {
        let mut segments = Vec::new();
        let mut syllables: Vec<Syllable> = Vec::new();
        let mut skip = false;
        let consonant = |ipa| Segment {
            ipa,
            syllable: None,
        };
        for (i, item) in self.items.iter().enumerate() {
            if std::mem::take(&mut skip) {
                continue;
            }
            let next = self.at(i + 1);
            let then = self.at(i + 2);
            match *item {
                Item::Consonant(c) => {
                    let (ipa, merged) = self.consonant(i, c);
                    skip = merged;
                    segments.push(consonant(ipa));
                }
                Item::Schwa(_) => {
                    let silent = self.said.get(i) == Some(&Said::No);
                    let k = syllables.len();
                    syllables.push(Syllable {
                        item: i,
                        silent,
                        long: false,
                    });
                    if !silent {
                        segments.push(Segment {
                            ipa: SCHWA,
                            syllable: Some(k),
                        });
                    }
                }
                Item::Vowel(_, _) if self.glide(i).is_some() => {
                    segments.extend(self.glide(i).map(consonant));
                    skip = true;
                }
                Item::Ay => {
                    let k = syllables.len();
                    syllables.push(Syllable {
                        item: i,
                        silent: false,
                        long: false,
                    });
                    segments.push(Segment {
                        ipa: "ɛ",
                        syllable: Some(k),
                    });
                    segments.push(consonant("j"));
                }
                Item::Vowel(vowel, alone) => {
                    let last = i + 1 == self.items.len();
                    let before_consonant = then.is_some();
                    let (ipa, coda) = match next {
                        Some(Item::Chandrabindu) if before_consonant && alone => {
                            (plain(vowel, alone, false), Some("n"))
                        }
                        Some(Item::Chandrabindu) if before_consonant => (nasal(vowel), Some("n")),
                        Some(Item::Anusvara | Item::Chandrabindu) => (nasal(vowel), None),
                        _ => (plain(vowel, alone, last), None),
                    };
                    if vowel == Vowel::R {
                        segments.push(consonant("ɾ"));
                    }
                    let k = syllables.len();
                    syllables.push(Syllable {
                        item: i,
                        silent: false,
                        long: ipa.contains('ː'),
                    });
                    segments.push(Segment {
                        ipa,
                        syllable: Some(k),
                    });
                    if matches!(next, Some(Item::Anusvara | Item::Chandrabindu)) {
                        segments.extend(coda.map(consonant));
                        skip = true;
                    }
                }
                Item::Anusvara => {
                    let ipa = match next {
                        Some(Item::Consonant(c)) => nasal_before(c),
                        _ => "n",
                    };
                    segments.push(consonant(ipa));
                }
                Item::Chandrabindu => segments.push(consonant("n")),
                Item::Visarga => segments.push(consonant("h")),
            }
        }
        (segments, syllables)
    }

    /// The sound of consonant `c` at `i`, and whether it took the next.
    fn consonant(&self, i: usize, c: char) -> (&'static str, bool) {
        let prev = i.checked_sub(1).and_then(|p| self.at(p));
        let next = match self.at(i + 1) {
            Some(Item::Consonant(n)) => Some(n),
            _ => None,
        };
        match (prev, c) {
            (Some(Item::Consonant('क')), 'ष') => return ("ʃ", false),
            (Some(Item::Consonant('ज')), 'ञ') => return ("j", false),
            _ => {}
        }
        let Some(n) = next else {
            return (consonant_ipa(c), false);
        };
        let ipa = match (c, n) {
            _ if c == n && long_stop(c).is_some() => return (long_stop(c).unwrap_or(""), true),
            ('त', 'थ') => return ("tʰː", true),
            ('द', 'ध') => return ("dʰː", true),
            ('च', 'छ') => "cʰ",
            ('ट', 'ठ') => "ʈʰ",
            ('ड', 'ढ') => "ɖʰ",
            ('ट', 'य') => "ʈʈ",
            (_, 'य') if long_stop(c).is_some() && c != 'ज' && c != 'द' => {
                long_stop(c).unwrap_or("")
            }
            ('ज', 'ञ') => "ɡ",
            ('र', _) if "मनणयवलशषसह".contains(n) => "r",
            _ => consonant_ipa(c),
        };
        (ipa, false)
    }

    /// Stress per syllable, silent ones included.
    fn stress(&self, syllables: &[Syllable], class: Class) -> Vec<Stress> {
        let n = syllables.len();
        let mut stress = vec![Stress::None; n];
        if n == 0 || class == Class::Unstressed {
            return stress;
        }
        let weights: Vec<u8> = (0..n).map(|k| self.weight(syllables, k)).collect();
        let body = weights.get(..n - 1).unwrap_or_default();
        let mut best = (0, 0);
        for (k, &w) in body.iter().enumerate() {
            if w >= best.1 {
                best = (k, w);
            }
        }
        let last = weights.last().copied().unwrap_or(0);
        let primary = if n == 1 || (best.1 == 0 && last < 2) {
            0
        } else if last == 2 && best.1 < 2 {
            n - 1
        } else {
            best.0
        };
        if let Some(s) = stress.get_mut(primary) {
            *s = Stress::Primary;
        }
        for k in 0..n {
            if k == primary {
                continue;
            }
            let prev = k == 0 || stress.get(k - 1) == Some(&Stress::None);
            let next = stress.get(k + 1).is_none_or(|s| *s == Stress::None);
            if prev
                && next
                && let Some(s) = stress.get_mut(k)
            {
                *s = Stress::Secondary;
            }
        }
        if class == Class::Weak
            && let Some(s) = stress.get_mut(primary)
        {
            *s = Stress::Secondary;
        }
        stress
    }

    /// One for a long vowel, one for a closed syllable.
    fn weight(&self, syllables: &[Syllable], k: usize) -> u8 {
        let Some(syllable) = syllables.get(k) else {
            return 0;
        };
        if syllable.silent {
            return 0;
        }
        let end = syllables.get(k + 1).map_or(self.items.len(), |s| s.item);
        let mut coda = 0;
        for i in syllable.item + 1..end {
            coda += match self.at(i) {
                Some(Item::Consonant(_) | Item::Visarga) => 1,
                Some(Item::Anusvara | Item::Chandrabindu) => {
                    u8::from(matches!(self.at(i + 1), Some(Item::Consonant(_))))
                }
                Some(Item::Vowel(..)) => 1,
                _ => 0,
            };
        }
        if matches!(self.at(end), Some(Item::Vowel(Vowel::R, _))) {
            coda += 1;
        }
        let last = k + 1 == syllables.len();
        let closed = if last { coda >= 1 } else { coda >= 2 };
        u8::from(syllable.long) + u8::from(closed)
    }
}

/// Whether an `a` is said.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Said {
    Yes,
    No,
    /// Unsaid unless it takes the main stress: before a cluster.
    Unstressed,
}

/// Whether the `a` at `i` goes unsaid.
fn silent(items: &[Item], i: usize, vowels: usize) -> Said {
    let said = |unsaid: bool| if unsaid { Said::No } else { Said::Yes };
    let at = |k: usize| items.get(k).copied();
    let Some(Item::Consonant(c)) = i.checked_sub(1).and_then(at) else {
        return Said::Yes;
    };
    let before = i.checked_sub(2).and_then(at);
    if i + 1 == items.len() {
        return said(
            vowels > 1
                && match before {
                    Some(Item::Consonant(first)) => !final_cluster_keeps(first, c),
                    _ => true,
                },
        );
    }
    if c == 'ह' || c == DHA_DOT {
        return Said::Yes;
    }
    let open_before = match before {
        Some(Item::Vowel(..) | Item::Schwa(_)) => true,
        Some(Item::Anusvara) => matches!(i.checked_sub(3).and_then(at), Some(Item::Vowel(..))),
        Some(Item::Consonant(first)) if one_sound(first, c) => {
            matches!(
                i.checked_sub(3).and_then(at),
                Some(Item::Vowel(..) | Item::Schwa(_))
            )
        }
        _ => false,
    };
    let mut k = i + 1;
    while let Some(Item::Consonant(_) | Item::Anusvara | Item::Chandrabindu) = at(k) {
        k += 1;
    }
    let full_vowel = matches!(at(k), Some(Item::Vowel(..) | Item::Ay));
    match k - i {
        _ if !open_before || !full_vowel => Said::Yes,
        1 if at(k) == Some(Item::Ay) => Said::No,
        1 if matches!(at(k), Some(Item::Vowel(_, true))) => Said::No,
        1 => Said::Yes,
        2 => Said::No,
        _ => Said::Unstressed,
    }
}

/// Numbers below a hundred, as written.
const ONES: [&str; 100] = [
    "शून्य",
    "एक",
    "दो",
    "तीन",
    "चार",
    "पाँच",
    "छह",
    "सात",
    "आठ",
    "नौ",
    "दस",
    "ग्यारह",
    "बारह",
    "तेरह",
    "चौदह",
    "पंद्रह",
    "सोलह",
    "सत्रह",
    "अठारह",
    "उन्नीस",
    "बीस",
    "इक्कीस",
    "बाईस",
    "तेईस",
    "चौबीस",
    "पच्चीस",
    "छब्बीस",
    "सत्ताईस",
    "अट्ठाईस",
    "उनतीस",
    "तीस",
    "इकतीस",
    "बत्तीस",
    "तैंतीस",
    "चौंतीस",
    "पैंतीस",
    "छत्तीस",
    "सैंतीस",
    "अड़तीस",
    "उनतालीस",
    "चालीस",
    "इकतालीस",
    "बयालीस",
    "तैंतालीस",
    "चवालीस",
    "पैंतालीस",
    "छियालीस",
    "सैंतालीस",
    "अड़तालीस",
    "उनचास",
    "पचास",
    "इक्यावन",
    "बावन",
    "तिरपन",
    "चौवन",
    "पचपन",
    "छप्पन",
    "सत्तावन",
    "अट्ठावन",
    "उनसठ",
    "साठ",
    "इकसठ",
    "बासठ",
    "तिरसठ",
    "चौंसठ",
    "पैंसठ",
    "छियासठ",
    "सड़सठ",
    "अड़सठ",
    "उनहत्तर",
    "सत्तर",
    "इकहत्तर",
    "बहत्तर",
    "तिहत्तर",
    "चौहत्तर",
    "पचहत्तर",
    "छिहत्तर",
    "सतहत्तर",
    "अठहत्तर",
    "उनासी",
    "अस्सी",
    "इक्यासी",
    "बयासी",
    "तिरासी",
    "चौरासी",
    "पचासी",
    "छियासी",
    "सत्तासी",
    "अट्ठासी",
    "नवासी",
    "नब्बे",
    "इक्यानबे",
    "बानबे",
    "तिरानबे",
    "चौरानबे",
    "पचानबे",
    "छियानबे",
    "सत्तानबे",
    "अट्ठानबे",
    "निन्यानबे",
];

fn one(n: u64) -> &'static str {
    let i = usize::try_from(n).unwrap_or(usize::MAX);
    ONES.get(i).copied().unwrap_or_default()
}

/// Digits as words; each inner list is said as one word.
fn number_words(text: &str) -> Vec<Vec<&'static str>> {
    let (whole, fraction) = match text.split_once('.') {
        Some((w, f)) if !f.contains('.') => (w, Some(f)),
        _ => (text, None),
    };
    let digits: String = whole.chars().filter(char::is_ascii_digit).collect();
    let plain = !(digits.len() > 1 && digits.starts_with('0'));
    let value = digits
        .parse::<u64>()
        .ok()
        .filter(|v| plain && *v < 1_000_000_000_000);
    let mut words = match value {
        Some(n) => cardinal(n),
        None => each_digit(&digits),
    };
    if let Some(fraction) = fraction {
        words.push(vec!["दशमलव"]);
        words.extend(each_digit(fraction));
    }
    words
}

fn each_digit(text: &str) -> Vec<Vec<&'static str>> {
    let digit = |b: u8| vec![one(u64::from(b - b'0'))];
    text.bytes().filter(u8::is_ascii_digit).map(digit).collect()
}

/// `n` in words, grouped in lakhs and crores.
fn cardinal(n: u64) -> Vec<Vec<&'static str>> {
    if n == 0 {
        return vec![vec![one(0)]];
    }
    let mut words = Vec::new();
    if n >= 10_000_000 {
        words.extend(cardinal(n / 10_000_000));
        words.push(vec!["करोड़"]);
    }
    for (size, name) in [(100_000, "लाख"), (1000, "हजार")] {
        let count = n / size % 100;
        if count > 0 {
            words.push(vec![one(count)]);
            words.push(vec![name]);
        }
    }
    let hundreds = n / 100 % 10;
    if hundreds > 0 {
        words.push(vec![one(hundreds), "सौ"]);
    }
    if !n.is_multiple_of(100) {
        words.push(vec![one(n % 100)]);
    }
    words
}
