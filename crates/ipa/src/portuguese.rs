//! Brazilian Portuguese: spelling to sounds, stress, and links between words.

use crate::letters::{letters, number};
use crate::text::{Piece, Token};
use crate::{Part, Sound, Spoken, Stress};

/// Words said without stress: articles, clitics and short links.
const WEAK: &[&str] = &[
    "o", "a", "os", "as", "de", "do", "da", "dos", "das", "que", "um", "uns", "com", "se", "no",
    "na", "nos", "nas", "num", "dum", "por", "ao", "aos", "mas", "é", "ou", "eu", "meu", "meus",
    "teu", "teus", "seu", "seus", "foi", "fui", "me", "te", "tu", "vos", "ser", "ter", "são",
    "tem", "há", "vai", "vão", "sou", "vou", "faz", "nós", "vós", "és", "la", "lo", "las", "los",
];

/// Words said with a lighter stress than others.
const LIGHT: &[&str] = &[
    "uma", "umas", "numa", "duma", "para", "como", "não", "sua", "suas", "tua", "tuas", "era",
    "eram", "ela", "elas", "ele", "eles", "dela", "delas", "dele", "deles", "nela", "nelas",
    "nele", "neles", "onde", "até", "sempre", "nunca", "tinha", "tinham", "minha", "minhas",
    "nosso", "nossa", "nossos", "nossas", "esta", "estas", "este", "estes", "esse", "esses",
    "nesse", "nesses", "neste", "nesta", "aquela", "aquelas", "daquela", "daquelas", "àquela",
    "desde", "estava", "estavam", "está", "estão", "estou", "esteve", "foram", "pode", "podem",
    "podia", "podiam", "posso", "poder", "podemos", "pôde", "tenho", "teve", "tive", "tivera",
    "fazer", "fazem", "seria", "será", "serás", "qualquer", "foste", "você", "vocês", "temos",
];

/// Words said otherwise than spelled, sounds split by spaces.
const SAID: &[(&str, &str)] = &[
    ("e", "i"),
    ("_e", "ˌi"),
    ("à", "ˌaː"),
    ("ao", "a^ʊ"),
    ("aos", "a^ʊ s"),
    ("lhe", "ʎ ˈy"),
    ("lhes", "ʎ ˈy s"),
    ("às", "aː s"),
    ("há", "a"),
    ("muito", "m w ˈiŋ t ʊ"),
    ("muitos", "m w ˈiŋ t ʊ s"),
    ("muita", "m w ˈiŋ t æ"),
    ("muitas", "m w ˈiŋ t æ s"),
    ("sobre", "s ˈo b r y"),
    ("entre", "ˈe^ɪŋ t r y"),
    ("esse", "ˌe s i"),
    ("neste", "n ˌe s t^ʃ i"),
    ("sol", "s ˈɔ l"),
    ("sobretudo", "s o b ɾ e t ˈu d ʊ"),
    ("pra", "p ɾ ˈa"),
    ("vos", "v u s"),
];

/// Common words respelled with a dictionary's accents, for open or
/// closed vowels the spelling hides.
const RESPELT: &[(&str, &str)] = &[
    ("pela", "pêla"),
    ("pelas", "pêlas"),
    ("era", "éra"),
    ("eram", "éram"),
    ("eras", "éras"),
    ("ele", "êle"),
    ("eles", "êles"),
    ("dele", "dêle"),
    ("deles", "dêles"),
    ("nele", "nêle"),
    ("neles", "nêles"),
    ("aquele", "aquêle"),
    ("aqueles", "aquêles"),
    ("daquele", "daquêle"),
    ("mulher", "mulhér"),
    ("mulheres", "mulhéres"),
    ("porque", "pôrque"),
    ("daqueles", "daquêles"),
    ("dez", "déz"),
    ("quer", "quér"),
    ("qualquer", "qualquér"),
    ("quero", "quéro"),
    ("queres", "quéres"),
    ("copo", "cópo"),
    ("copos", "cópos"),
    ("modo", "módo"),
    ("modos", "módos"),
    ("roda", "róda"),
    ("rodas", "ródas"),
    ("toca", "tóca"),
    ("bosque", "bósque"),
    ("bosques", "bósques"),
    ("peça", "péça"),
    ("peças", "péças"),
    ("velha", "vélha"),
    ("velhas", "vélhas"),
    ("velho", "vélho"),
    ("velhos", "vélhos"),
    ("grosso", "grôsso"),
    ("torre", "tôrre"),
    ("maior", "maiór"),
    ("melhor", "melhór"),
    ("pior", "piór"),
    ("loja", "lója"),
    ("lojas", "lójas"),
    ("exceto", "excéto"),
    ("cerca", "cêrca"),
    ("naquele", "naquêle"),
    ("naqueles", "naquêles"),
    ("teve", "têve"),
    ("esteve", "estêve"),
    ("pode", "póde"),
    ("podem", "pódem"),
];

/// Short forms read in full when a full stop follows.
const SHORT: &[(&str, &str)] = &[
    ("sr", "senhor"),
    ("sra", "senhora"),
    ("srta", "senhorita"),
    ("dr", "doutor"),
    ("dra", "doutora"),
    ("etc", "etcétera"),
    ("pág", "página"),
];

/// Letters named when a word has no vowel to say.
const NAMES: &[(char, &str)] = &[
    ('b', "bê"),
    ('c', "cê"),
    ('d', "dê"),
    ('f', "éfe"),
    ('g', "gê"),
    ('h', "agá"),
    ('j', "jóta"),
    ('k', "cá"),
    ('l', "éle"),
    ('m', "ême"),
    ('n', "êne"),
    ('p', "pê"),
    ('q', "quê"),
    ('r', "érre"),
    ('s', "ésse"),
    ('t', "tê"),
    ('v', "vê"),
    ('w', "dáblio"),
    ('x', "xis"),
    ('z', "zê"),
];

/// Sentences of `tokens` as said in Brazil.
pub(crate) fn read(tokens: &[Token<'_>]) -> Vec<Spoken> {
    let mut spoken = Vec::new();
    let mut held = Vec::new();
    let mut stressable = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let space = token.space && !token.hyphen;
        let said: Vec<(String, bool)> = match token.piece {
            Piece::Mark(mark) => {
                spoken.push(Spoken {
                    part: Part::Mark(mark),
                    space,
                });
                held.push(false);
                stressable.push(None);
                continue;
            }
            Piece::Break => {
                spoken.push(Spoken {
                    part: Part::Break,
                    space,
                });
                held.push(false);
                stressable.push(None);
                continue;
            }
            Piece::Number(text) => number_words(text),
            Piece::Word(text) => {
                let dot = tokens.get(at + 1).map(|t| t.piece) == Some(Piece::Mark('.'));
                spelled(text, dot)
            }
        };
        let silent_h = matches!(token.piece, Piece::Word(t) if t.starts_with(['h', 'H']));
        let named = matches!(token.piece, Piece::Word(t) if !t.chars().any(|c| is_vowel(c.to_ascii_lowercase())));
        let several = said.len() > 1 && matches!(token.piece, Piece::Number(_));
        for (i, (word, joined)) in said.iter().enumerate() {
            let mut sounds = word_sounds(word);
            if several && word != "_e" {
                for sound in sounds.iter_mut().filter(|s| s.stress == Stress::Secondary) {
                    sound.stress = Stress::None;
                }
            }
            spoken.push(Spoken {
                part: Part::Word(sounds),
                space: if i == 0 { space } else { !joined },
            });
            held.push(i == 0 && (silent_h || named));
            let word = word.as_str();
            stressable.push(match () {
                () if AT_PAUSE.contains(&word) || token.hyphen && i == 0 => Some(true),
                () if LIGHT_AT_PAUSE.contains(&word) => Some(false),
                () => None,
            });
        }
    }
    stress_before_pauses(&mut spoken, &stressable);
    link(&mut spoken, &held);
    spoken
}

/// A written word as the words said, each joined to the last or not.
fn spelled(text: &str, dot: bool) -> Vec<(String, bool)> {
    let word: String = letters(text).into_iter().filter(|&c| c != '\'').collect();
    if dot && let Some((_, full)) = SHORT.iter().find(|(short, _)| *short == word) {
        return vec![((*full).to_owned(), false)];
    }
    if word.chars().any(is_vowel) {
        return vec![(word, false)];
    }
    let name = |c: char| NAMES.iter().find(|(l, _)| *l == c).map(|(_, n)| *n);
    word.chars()
        .filter_map(name)
        .map(|n| (n.to_owned(), false))
        .collect()
}

fn is_vowel(c: char) -> bool {
    "aeiouáàâãéêíóôõúüy".contains(c)
}

/// What an accent says about its vowel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    None,
    /// `á é í ó ú`: stressed, `e` and `o` open.
    Acute,
    /// `â ê ô`: stressed and closed.
    Circumflex,
    /// `ã õ`: nasal, stressed unless another accent is written.
    Tilde,
    /// `à`, `ü`: neither stress nor quality.
    Other,
}

/// The vowel a letter writes, and its accent.
fn vowel(c: char) -> Option<(char, Mark)> {
    let (base, mark) = match c {
        'a' => ('a', Mark::None),
        'á' => ('a', Mark::Acute),
        'â' => ('a', Mark::Circumflex),
        'ã' => ('a', Mark::Tilde),
        'à' => ('a', Mark::Other),
        'e' => ('e', Mark::None),
        'é' => ('e', Mark::Acute),
        'ê' => ('e', Mark::Circumflex),
        'i' | 'y' => ('i', Mark::None),
        'í' => ('i', Mark::Acute),
        'o' => ('o', Mark::None),
        'ó' => ('o', Mark::Acute),
        'ô' => ('o', Mark::Circumflex),
        'õ' => ('o', Mark::Tilde),
        'u' => ('u', Mark::None),
        'ú' => ('u', Mark::Acute),
        'ü' => ('u', Mark::Other),
        _ => return None,
    };
    Some((base, mark))
}

/// The kind of vowel sound a nucleus is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Single,
    /// A vowel closing on `i` or `u`, as `ei`, `ou`.
    Falling(char),
    /// `ão`, `ãe`, `õe`.
    Nasal(char),
}

/// A syllable's vowel: where its letters lie and what they write.
#[derive(Clone, Copy)]
struct Nucleus {
    start: usize,
    end: usize,
    base: char,
    mark: Mark,
    kind: Kind,
    /// An unstressed `i` or `u` glided into the vowel after.
    glide: bool,
}

/// Where a nucleus stands against the stress.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Primary,
    Before,
    /// After the stress, not last.
    Between,
    Last,
    /// The only vowel of an unstressed word.
    Weak,
}

/// A word's sounds with its stress.
fn word_sounds(word: &str) -> Vec<Sound> {
    if let Some((_, said)) = SAID.iter().find(|(w, _)| *w == word) {
        return parse(said);
    }
    let spelt = RESPELT
        .iter()
        .find(|(w, _)| *w == word)
        .map_or(word, |(_, r)| *r);
    let w: Vec<char> = letters(spelt).into_iter().filter(|&c| c != '\'').collect();
    let light = LIGHT.contains(&word);
    let mut sounds = sounds(&w, WEAK.contains(&word), light);
    if light {
        for sound in &mut sounds {
            sound.stress = match sound.stress {
                Stress::Primary => Stress::Secondary,
                _ => Stress::None,
            };
        }
    }
    sounds
}

/// Sounds written out in [`SAID`].
fn parse(said: &'static str) -> Vec<Sound> {
    said.split(' ')
        .map(|ipa| {
            let (stress, ipa) = match ipa.strip_prefix('ˈ') {
                Some(rest) => (Stress::Primary, rest),
                None => match ipa.strip_prefix('ˌ') {
                    Some(rest) => (Stress::Secondary, rest),
                    None => (Stress::None, ipa),
                },
            };
            let vowel = ipa
                .chars()
                .next()
                .is_some_and(|c| "aeiouɐæɛɔyʊ".contains(c));
            Sound { ipa, vowel, stress }
        })
        .collect()
}

/// The vowels of `w` in syllables.
fn nuclei(w: &[char]) -> Vec<Nucleus> {
    let at = |k: usize| w.get(k).copied();
    let mut out: Vec<Nucleus> = Vec::new();
    let mut i = 0;
    while let Some(c) = at(i) {
        let Some((base, mark)) = vowel(c) else {
            i += 1;
            continue;
        };
        let prev = i.checked_sub(1).and_then(at);
        let next = at(i + 1).and_then(vowel);
        let after_qg = matches!(prev, Some('q' | 'g'));
        if base == 'u' && mark != Mark::Acute && after_qg && next.is_some() {
            i += 1;
            continue;
        }
        if base == 'i' && c == 'y' && next.is_some() && i > 0 {
            i += 1;
            continue;
        }
        let mut nucleus = Nucleus {
            start: i,
            end: i + 1,
            base,
            mark,
            kind: Kind::Single,
            glide: false,
        };
        match next {
            Some((second, Mark::None)) if mark == Mark::Tilde && "oe".contains(second) => {
                nucleus.kind = Kind::Nasal(second);
                nucleus.end = i + 2;
            }
            Some((second, Mark::None))
                if matches!(second, 'i' | 'u')
                    && second != base
                    && !hiatus(w, i + 1)
                    && at(i + 2) != Some('u') =>
            {
                nucleus.kind = Kind::Falling(second);
                nucleus.end = i + 2;
            }
            _ => {}
        }
        i = nucleus.end;
        out.push(nucleus);
    }
    out
}

/// Whether the `i` or `u` at `k` is its own syllable, as `cair`.
fn hiatus(w: &[char], k: usize) -> bool {
    let at = |k: usize| w.get(k).copied();
    let next = at(k + 1);
    let then = at(k + 2);
    if next == Some('n') && then == Some('h') {
        return true;
    }
    let closing = matches!(next, Some('l' | 'm' | 'n' | 'r' | 'z'));
    closing && then.is_none_or(|c| vowel(c).is_none())
}

/// Which nucleus takes the stress.
fn stressed(w: &[char], nuclei: &[Nucleus]) -> usize {
    let marked = |m: Mark| nuclei.iter().rposition(|n| n.mark == m);
    if let Some(at) = marked(Mark::Acute).or(marked(Mark::Circumflex)) {
        return at;
    }
    if let Some(at) = marked(Mark::Tilde) {
        return at;
    }
    let last = nuclei.len().saturating_sub(1);
    let stem = match w {
        [stem @ .., 's'] => stem,
        _ => w,
    };
    let penult = matches!(
        stem,
        [.., 'a' | 'e' | 'o'] | [.., 'a' | 'e', 'm'] | [.., 'e', 'n']
    );
    let falling = nuclei
        .last()
        .is_some_and(|n| matches!(n.kind, Kind::Falling(_)) && n.end == stem.len());
    if penult && !falling {
        last.saturating_sub(1)
    } else {
        last
    }
}

/// Whether two consonants come before the letter at `start`.
fn n_start_cluster(w: &[char], start: usize) -> bool {
    let consonant = |k: Option<usize>| {
        k.and_then(|k| w.get(k))
            .is_some_and(|&c| vowel(c).is_none())
    };
    consonant(start.checked_sub(1)) && consonant(start.checked_sub(2))
}

/// `w` as sounds, stressed by the spelling unless `weak`.
fn sounds(w: &[char], weak: bool, light: bool) -> Vec<Sound> {
    let mut nuclei = nuclei(w);
    if nuclei.is_empty() {
        return Vec::new();
    }
    let primary = stressed(w, &nuclei);
    for k in 0..nuclei.len() {
        let next_start = nuclei.get(k + 1).map(|n| n.start);
        let cluster = n_start_cluster(w, nuclei.get(k).map_or(0, |n| n.start));
        let next_closes = nuclei
            .get(k + 1)
            .is_some_and(|n| matches!(n.kind, Kind::Falling(_)) || n.base == 'i' && cluster);
        let Some(n) = nuclei.get_mut(k) else {
            continue;
        };
        let unmarked = n.mark == Mark::None && n.kind == Kind::Single;
        let adjacent = next_start == Some(n.end);
        n.glide = k != primary && unmarked && adjacent && matches!(n.base, 'i' | 'u');
        n.glide &= k > primary || n.base == 'u' && n.start > 0 && next_closes;
    }
    let last = nuclei.len() - 1;
    let role = |k: usize| match k {
        _ if weak => Role::Weak,
        k if k == primary => Role::Primary,
        k if k < primary => Role::Before,
        k if k == last => Role::Last,
        _ => Role::Between,
    };
    let mut out: Vec<Sound> = Vec::with_capacity(w.len() + 2);
    let mut syllable_of: Vec<Option<usize>> = Vec::new();
    let mut i = 0;
    let mut k = 0;
    while i < w.len() {
        if let Some(n) = nuclei.get(k).filter(|n| n.start == i).copied() {
            let at = out.len();
            vowel_sounds(w, &n, role(k), &mut out);
            for sound in out.iter_mut().skip(at) {
                if sound.vowel && k == primary && !weak {
                    sound.stress = Stress::Primary;
                }
            }
            syllable_of.resize(out.len(), None);
            if let Some(slot) = syllable_of.get_mut(at) {
                *slot = (!n.glide).then_some(k);
            }
            i = n.end;
            k += 1;
            continue;
        }
        i += consonant(w, i, &mut out);
        syllable_of.resize(out.len(), None);
    }
    affricate(&mut out);
    if !weak && !light {
        secondary(&mut out, &syllable_of, &nuclei, primary);
    }
    out
}

/// The sounds of nucleus `n`, said as its `role` has it.
fn vowel_sounds(w: &[char], n: &Nucleus, role: Role, out: &mut Vec<Sound>) {
    let at = |k: usize| w.get(k).copied();
    let after = at(n.end);
    let then = at(n.end + 1);
    let onset = |c: Option<char>| c.is_some_and(|c| vowel(c).is_some() || c == 'h');
    let nasal_onset = matches!(after, Some('m' | 'n')) && onset(then);
    let nasal_coda = matches!(after, Some('m' | 'n')) && !onset(then);
    let final_s = after.is_none() || after == Some('s') && then.is_none();
    let open = role == Role::Primary && n.mark == Mark::None && open_vowel(w, n);
    let ipa: &'static str = match n.kind {
        Kind::Nasal('o') => "ɐ̃^ʊ̃",
        Kind::Nasal(_) if n.base == 'a' => "ɐ̃y",
        Kind::Nasal(_) => "o\u{303}j",
        Kind::Falling(second) => falling(n.base, second, n.mark == Mark::Acute),
        Kind::Single if n.glide => {
            if n.base == 'i' {
                "j"
            } else {
                "w"
            }
        }
        Kind::Single => match (n.base, role) {
            ('a', _) if n.mark == Mark::Tilde => "ɐ̃",
            ('a', _) if nasal_coda => "ɐ̃",
            ('a', Role::Primary) if nasal_onset || after == Some('n') => "ɐ̃",
            ('a', Role::Last) if final_s => "æ",
            ('a', Role::Before | Role::Between) if after == Some('n') && then == Some('h') => "ɐ̃",
            ('a', Role::Before | Role::Between) if nasal_onset => "æ",
            ('a', Role::Between) => "æ",
            ('a', _) => "a",
            ('e', _) if nasal_coda => "e^ɪ",
            ('e', Role::Last | Role::Weak) if final_s && n.mark == Mark::None => "y",
            ('e', _) if n.mark == Mark::Acute || open => "ɛ",
            ('e', _) => "e",
            ('i', _) => "i",
            ('o', _) if nasal_coda => "o",
            ('o', Role::Last | Role::Weak) if final_s && n.mark == Mark::None => "ʊ",
            ('o', _) if n.mark == Mark::Acute || open => "ɔ",
            ('o', _) => "o",
            ('u', _) if nasal_coda || after == Some('n') && then == Some('h') => "u\u{303}",
            _ => "u",
        },
    };
    let glide = n.glide;
    out.push(Sound {
        ipa,
        vowel: !glide,
        stress: Stress::None,
    });
}

/// A vowel closing on `i` or `u` in one syllable.
fn falling(first: char, then: char, acute: bool) -> &'static str {
    match (first, then, acute) {
        ('a', 'i', _) => "a^ɪ",
        ('a', _, _) => "a^ʊ",
        ('e', 'i', true) => "ɛ^ɪ",
        ('e', 'i', false) => "e^ɪ",
        ('e', _, true) => "ɛ^ʊ",
        ('e', _, false) => "e^ʊ",
        ('o', 'i', true) => "ɔ^ɪ",
        ('o', 'i', false) => "o^ɪ",
        ('o', _, _) => "ow",
        ('u', _, _) => "u^ɪ",
        _ => "i^ʊ",
    }
}

/// Whether a stressed plain `e` or `o` is open, as `certo`, `porta`.
fn open_vowel(w: &[char], n: &Nucleus) -> bool {
    let rest = w.get(n.end..).unwrap_or_default();
    let split = rest
        .iter()
        .position(|&c| vowel(c).is_some())
        .unwrap_or(rest.len());
    let (cluster, end) = rest.split_at(split);
    let cluster: String = cluster.iter().collect();
    let end: String = end.iter().collect();
    let (cluster, end) = (cluster.as_str(), end.as_str());
    let masculine = end == "o";
    let feminine = matches!(end, "a" | "as");
    let plural = end == "os";
    if cluster.starts_with(['m', 'n']) {
        return false;
    }
    let liquid_then = |first: char| cluster.len() > 1 && cluster.starts_with(first);
    match n.base {
        'e' => match cluster {
            "rç" | "lh" | "rd" => false,
            _ if liquid_then('r') || liquid_then('l') => true,
            "r" | "ss" if strong_past(w, n, end) => true,
            "ss" => feminine || masculine || plural,
            "dr" | "v" => feminine || end.starts_with('e'),
            "g" => feminine,
            "ct" => true,
            "st" => !end.starts_with('e'),
            "l" => feminine || end.starts_with('e') || end.is_empty(),
            "t" => end.starts_with('e') || feminine,
            "c" => end == "e",
            _ => false,
        },
        'o' => match cluster {
            "rq" | "rç" | "d" | "ç" | "c" | "lh" if !(cluster == "lh" && plural) => false,
            "lh" => true,
            "l" => feminine || end.is_empty() || end.starts_with('e'),
            _ if liquid_then('r') || liquid_then('l') => !masculine,
            "r" => feminine,
            "s" | "v" | "st" => !masculine,
            "ss" | "br" | "g" | "ch" | "z" => true,
            _ => false,
        },
        _ => false,
    }
}

/// Whether `-era` or `-esse` is open, as `fizera`, `tivesse`.
fn strong_past(w: &[char], n: &Nucleus, end: &str) -> bool {
    let before = n.start.checked_sub(1).and_then(|k| w.get(k)).copied();
    let ending = matches!(end, "a" | "as" | "am" | "e" | "es" | "em");
    ending && matches!(before, Some('z' | 'v' | 'x' | 'i' | 'p' | 'd'))
}

/// The consonant at `i` onto `out`; how many letters it took.
fn consonant(w: &[char], i: usize, out: &mut Vec<Sound>) -> usize {
    let at = |k: usize| w.get(k).copied();
    let is_v = |k: usize| at(k).and_then(vowel).is_some();
    let Some(ch) = at(i) else {
        return 1;
    };
    let prev = i.checked_sub(1).and_then(at);
    let next = at(i + 1);
    let front = next
        .and_then(vowel)
        .is_some_and(|(b, _)| matches!(b, 'e' | 'i'));
    let mut push = |ipa: &'static str| out.push(Sound::consonant(ipa));
    if next == Some(ch) && !matches!(ch, 'r' | 's' | 'c') {
        return 1;
    }
    match ch {
        'b' => push("b"),
        'c' if next == Some('k') => {}
        'c' if next == Some('h') => {
            push("ʃ");
            return 2;
        }
        'c' if next == Some('c')
            && at(i + 2)
                .and_then(vowel)
                .is_some_and(|(b, _)| "ei".contains(b)) =>
        {
            push("k");
        }
        'c' if next == Some('c') => return 1,
        'c' if front => push("s"),
        'c' | 'k' => push("k"),
        'ç' => push("s"),
        'd' => push("d"),
        'f' => push("f"),
        'g' | 'q' => {
            let u = next == Some('u') && is_v(i + 2);
            let soft = at(i + 2)
                .and_then(vowel)
                .is_some_and(|(b, _)| matches!(b, 'e' | 'i'));
            match (ch, u, front) {
                ('g', false, true) => push("ʒ"),
                ('g', _, _) => push("ɡ"),
                _ => push("k"),
            }
            if u {
                if !soft || next == Some('ü') {
                    push("w");
                }
                return 2;
            }
            if next == Some('ü') {
                push("w");
                return 2;
            }
        }
        'h' => {}
        'j' => push("ʒ"),
        'l' if next == Some('h') => {
            push(if i == 0 { "ʎ" } else { "lj" });
            return 2;
        }
        'l' if is_v(i + 1) => push("l"),
        'l' => coda_l(out, next),
        'm' | 'n' if next == Some('h') && ch == 'n' => {
            push("ɲ");
            return 2;
        }
        'm' | 'n' if is_v(i + 1) || next == Some('h') => push(if ch == 'm' { "m" } else { "n" }),
        'm' | 'n' => coda_nasal(out, ch, next),
        'p' if next == Some('h') => {
            push("f");
            return 2;
        }
        'p' => push("p"),
        'r' if next == Some('r') => {
            push("x");
            return 2;
        }
        'r' if i == 0 || matches!(prev, Some('n' | 'l' | 's')) => push("x"),
        'r' if prev.is_some_and(|c| "bcdfgkptv".contains(c)) => push("r"),
        'r' if is_v(i + 1) => push("ɾ"),
        'r' if next.is_none() => push("r"),
        'r' => push("ɾə"),
        's' if next == Some('s') => {
            push("s");
            return 2;
        }
        's' if next == Some('c') && front_at(w, i + 2) => {
            push("s");
            return 2;
        }
        's' if next == Some('ç') => {
            push("s");
            return 2;
        }
        's' if i > 0 && prev.and_then(vowel).is_some() && is_v(i + 1) => push("z"),
        's' if next.is_some_and(|c| "bdglmnrvz".contains(c)) => push("z"),
        's' => push("s"),
        't' if next == Some('h') => {
            push("t");
            return 2;
        }
        't' => push("t"),
        'v' => push("v"),
        'w' => push("w"),
        'y' => push("j"),
        'x' if i == 1 && prev == Some('e') && is_v(i + 1) => push("z"),
        'x' if next == Some('c') && front_at(w, i + 2) => {}
        'x' if next.is_none() => {
            push("k");
            push("s");
        }
        'x' if prev.is_some_and(|c| vowel(c).is_some_and(|(b, _)| b == 'e')) && !is_v(i + 1) => {
            push("s");
        }
        'x' => push("ʃ"),
        'z' if next.is_none() => push("s"),
        'z' => push("z"),
        _ => {}
    }
    1
}

fn front_at(w: &[char], k: usize) -> bool {
    w.get(k)
        .copied()
        .and_then(vowel)
        .is_some_and(|(b, _)| matches!(b, 'e' | 'i'))
}

/// `l` closing a syllable glides the vowel before it into `ʊ`.
fn coda_l(out: &mut Vec<Sound>, next: Option<char>) {
    let last = next.is_none();
    let glided = out.last_mut().filter(|s| s.vowel).and_then(|sound| {
        let stressed = sound.stress == Stress::Primary;
        if stressed && next == Some('t') && sound.ipa == "ɔ" {
            return None;
        }
        let ipa = match sound.ipa {
            "a" | "æ" => "a^ʊ",
            "e" if stressed && last => "ɛ^ʊ",
            "e" | "y" => "e^ʊ",
            "ɛ" => "ɛ^ʊ",
            "i" => "i^ʊ",
            "o" | "ʊ" => "ow",
            "ɔ" => "ɔw",
            "u" => "uw",
            _ => return None,
        };
        sound.ipa = ipa;
        Some(())
    });
    if glided.is_none() {
        out.push(Sound::consonant("l"));
    }
}

/// `m` or `n` closing a syllable nasalises the vowel before it.
fn coda_nasal(out: &mut Vec<Sound>, ch: char, next: Option<char>) {
    let last = next.is_none() || next == Some('s');
    if let Some(sound) = out.last_mut()
        && sound.ipa == "ɐ̃"
        && next.is_none()
    {
        sound.ipa = "ɐ̃^ʊ̃";
        return;
    }
    let labial = ch == 'm' || matches!(next, Some('p' | 'b'));
    out.push(Sound::consonant(if labial && !last { "m" } else { "ŋ" }));
}

/// `t` and `d` turn into affricates before an `i` sound.
fn affricate(sounds: &mut [Sound]) {
    for i in 0..sounds.len() {
        let front = sounds
            .get(i + 1)
            .is_some_and(|s| s.ipa.starts_with(['i', 'j', 'y', 'ĩ']));
        if let Some(sound) = sounds.get_mut(i).filter(|_| front) {
            sound.ipa = match sound.ipa {
                "t" => "t^ʃ",
                "d" => "d^ʒ",
                ipa => ipa,
            };
        }
    }
}

/// Lighter stress on a long word's first syllable, then every second.
fn secondary(
    sounds: &mut [Sound],
    nucleus_of: &[Option<usize>],
    nuclei: &[Nucleus],
    primary: usize,
) {
    let syllable = |k: usize| nuclei.iter().take(k).filter(|n| !n.glide).count();
    let count = syllable(nuclei.len());
    let primary = syllable(primary);
    if count < 3 || primary == 0 {
        return;
    }
    let lighter = |k: usize| k == 0 || k.is_multiple_of(2) && k + 1 < primary;
    for (sound, k) in sounds.iter_mut().zip(nucleus_of) {
        let Some(k) = *k else {
            continue;
        };
        let k = syllable(k);
        if k < primary && lighter(k) && sound.vowel {
            sound.stress = Stress::Secondary;
        }
    }
}

/// Unstressed words that take stress before a pause.
const AT_PAUSE: &[&str] = &[
    "eu", "tu", "nós", "vós", "ser", "ter", "faz", "vai", "vou", "sou", "tem", "são", "vão", "e",
    "meu", "teu", "seu",
];

/// Light words that stay light before a pause: verbs and `uma`.
const LIGHT_AT_PAUSE: &[&str] = &[
    "uma", "umas", "numa", "duma", "era", "eram", "estava", "estavam", "está", "estão", "estou",
    "esteve", "foram", "pode", "podem", "podia", "podiam", "posso", "poder", "podemos", "pôde",
    "tenho", "teve", "tive", "tivera", "fazer", "fazem", "seria", "será", "serás", "tinha",
    "tinham", "foste", "temos",
];

/// A light word, or one of [`AT_PAUSE`], takes full stress before a pause.
fn stress_before_pauses(spoken: &mut [Spoken], stressable: &[Option<bool>]) {
    for i in 0..spoken.len() {
        let pause = !matches!(spoken.get(i + 1).map(|s| &s.part), Some(Part::Word(_)));
        let Some(Part::Word(sounds)) = spoken.get_mut(i).map(|s| &mut s.part) else {
            continue;
        };
        if !pause || sounds.iter().any(|s| s.stress == Stress::Primary) {
            continue;
        }
        let lighter = sounds.iter().position(|s| s.stress == Stress::Secondary);
        let wanted = stressable.get(i).copied().flatten();
        if wanted == Some(false) || lighter.is_none() && wanted != Some(true) {
            continue;
        }
        if let [sound] = sounds.as_mut_slice()
            && sound.ipa == "i"
        {
            sound.ipa = "ɛ";
        }
        let at = lighter.or(sounds.iter().position(|s| s.vowel));
        if let Some(sound) = at.and_then(|at| sounds.get_mut(at)) {
            sound.stress = Stress::Primary;
        }
    }
}

/// Sounds changed by the next word: `s` voiced, `r` tapped, vowels glide.
fn link(spoken: &mut [Spoken], held: &[bool]) {
    for p in 0..spoken.len() {
        let joined = spoken.get(p + 1).is_some_and(|s| !s.space);
        let next = match spoken.get(p + 1).map(|s| &s.part) {
            _ if held.get(p + 1) == Some(&true) => None,
            Some(Part::Word(sounds)) => sounds.first().copied(),
            _ => None,
        };
        let Some(Part::Word(sounds)) = spoken.get_mut(p).map(|s| &mut s.part) else {
            continue;
        };
        let before_vowel = next.is_some_and(|s| s.vowel) && !joined;
        let voiced = next.is_some_and(|s| {
            s.vowel
                || matches!(
                    s.ipa,
                    "b" | "d"
                        | "d^ʒ"
                        | "ɡ"
                        | "v"
                        | "z"
                        | "ʒ"
                        | "m"
                        | "n"
                        | "ɲ"
                        | "l"
                        | "ʎ"
                        | "r"
                        | "ɾ"
                        | "w"
                        | "j"
                        | "x"
                )
        });
        let count = sounds.len();
        let liquid = count >= 2
            && sounds
                .get(count - 2)
                .is_some_and(|s| matches!(s.ipa, "l" | "r" | "ɾ"));
        let que = count == 2 && sounds.first().is_some_and(|s| s.ipa == "k");
        let article = count == 1;
        let full = sounds.iter().any(|s| s.stress == Stress::Primary);
        let light = !full && sounds.iter().any(|s| s.stress == Stress::Secondary);
        let affricate = count >= 2
            && sounds
                .get(count - 2)
                .is_some_and(|s| matches!(s.ipa, "t^ʃ" | "d^ʒ"));
        let held_e = light && !affricate;
        let after_r = count >= 2
            && sounds
                .get(count - 2)
                .is_some_and(|s| matches!(s.ipa, "r" | "ɾ"));
        let Some(last) = sounds.last_mut() else {
            continue;
        };
        last.ipa = match last.ipa {
            "s" | "z" if voiced => "z",
            "z" => "s",
            "r" if before_vowel => "ɾ",
            "y" if last.stress != Stress::None => "y",
            "y" if before_vowel && !que && liquid && (!held_e || after_r) => "i",
            "y" if before_vowel && !que && !held_e => "j",
            "ʊ" if before_vowel && article => "u",
            "ʊ" if before_vowel && full => "w",
            ipa => ipa,
        };
        if matches!(last.ipa, "j" | "w") {
            last.vowel = false;
            if last.stress != Stress::None {
                last.stress = Stress::None;
            }
        }
    }
}

const ONES: [&str; 20] = [
    "zero",
    "úm",
    "dois",
    "três",
    "quatro",
    "cinco",
    "seis",
    "sete",
    "oito",
    "nove",
    "dez",
    "onze",
    "doze",
    "treze",
    "catorze",
    "quinze",
    "dezesseis",
    "dezessete",
    "dezoito",
    "dezenove",
];

const TENS: [&str; 10] = [
    "",
    "",
    "vinte",
    "trinta",
    "quarenta",
    "cinquenta",
    "sessenta",
    "setenta",
    "oitenta",
    "noventa",
];

/// Hundreds, `+` joining the parts said as two words.
const HUNDREDS: [&str; 10] = [
    "",
    "cento",
    "duzentos",
    "trezentos",
    "quatrocentos",
    "quinhentos",
    "seiscentos",
    "séte+centos",
    "ôito+centos",
    "nóve+centos",
];

fn pick(table: &[&'static str], i: u64) -> &'static str {
    let i = usize::try_from(i).unwrap_or(usize::MAX);
    table.get(i).copied().unwrap_or_default()
}

/// Words said for digits, each with whether it joins the one before.
type Said = Vec<(String, bool)>;

fn push(words: &mut Said, text: &str, joined: bool) {
    for (i, part) in text.split('+').enumerate() {
        words.push((part.to_owned(), joined || i > 0));
    }
}

/// Digits as the words said for them.
fn number_words(text: &str) -> Said {
    let written = number(text);
    let mut words = Said::new();
    match written.whole {
        Some(n) => cardinal(n, &mut words),
        None => digits(
            text.split([',', '.']).next().unwrap_or_default(),
            &mut words,
        ),
    }
    if let Some(fraction) = written.fraction {
        push(&mut words, "vírgula", false);
        let short = fraction.len() <= 2 && !fraction.starts_with('0');
        match fraction.parse::<u64>() {
            Ok(n) if short => cardinal(n, &mut words),
            _ => digits(fraction, &mut words),
        }
    }
    words
}

fn digits(text: &str, words: &mut Said) {
    for b in text.bytes().filter(u8::is_ascii_digit) {
        push(words, pick(&ONES, u64::from(b - b'0')), false);
    }
}

/// `n` in words, below a million million.
fn cardinal(n: u64, words: &mut Said) {
    if n == 0 {
        push(words, "zero", false);
        return;
    }
    let millions = n / 1_000_000;
    match millions {
        0 => {}
        1 => push(words, "úm+milhão", false),
        _ => {
            hundreds(millions, words);
            push(words, "milhões", false);
        }
    }
    let rest = n % 1_000_000;
    match rest / 1000 {
        0 => {}
        1 => push(words, "mil", false),
        k => {
            hundreds(k, words);
            push(words, "mil", false);
        }
    }
    let low = rest % 1000;
    if low > 0 && low < 100 && n >= 1000 {
        push(words, "e", false);
    }
    hundreds(low, words);
}

fn hundreds(n: u64, words: &mut Said) {
    if n == 100 {
        push(words, "cem", false);
        return;
    }
    let rest = n % 100;
    if n >= 100 {
        let hundred = pick(&HUNDREDS, n / 100);
        push(words, hundred, false);
        if rest > 0 {
            push(words, if hundred == "cento" { "e" } else { "_e" }, true);
        }
    }
    match rest {
        0 => {}
        r @ 1..20 => push(words, pick(&ONES, r), false),
        r => {
            push(words, pick(&TENS, r / 10), false);
            if r % 10 > 0 {
                push(words, "e", true);
                push(words, pick(&ONES, r % 10), true);
            }
        }
    }
}
