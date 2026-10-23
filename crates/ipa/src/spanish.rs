//! Castilian Spanish: spelling to sounds, stress, and links between words.

use crate::letters::{letters, number};
use crate::text::{Piece, Token};
use crate::{Part, Sound, Spoken, Stress};

/// Words said without stress: articles, clitics and short links.
const WEAK: &[&str] = &[
    "el", "la", "lo", "los", "las", "le", "les", "me", "te", "se", "nos", "os", "mi", "mis", "tu",
    "tus", "su", "sus", "de", "del", "a", "al", "en", "con", "sin", "por", "y", "e", "o", "u",
    "que", "si", "quien", "tras",
];

/// Links said with a lighter stress than other words.
const LIGHT: &[&str] = &[
    "para", "como", "cuando", "donde", "pero", "porque", "desde", "sobre", "hasta", "entre",
    "aunque", "hacia", "ante", "bajo", "mientras", "cuanto", "cuanta", "cuantos", "cuantas",
    "nuestro", "nuestra", "nuestros", "nuestras", "vuestro", "vuestra", "vuestros", "vuestras",
];

/// Short forms read in full when a full stop follows.
const SHORT: &[(&str, &str)] = &[
    ("sr", "señor"),
    ("sra", "señora"),
    ("srta", "señorita"),
    ("dr", "doctor"),
    ("dra", "doctora"),
    ("ud", "usted"),
    ("uds", "ustedes"),
    ("etc", "etcétera"),
    ("pág", "página"),
];

/// Letters named when a word has no vowel to say.
const NAMES: &[(char, &str)] = &[
    ('b', "be"),
    ('c', "ce"),
    ('d', "de"),
    ('f', "efe"),
    ('g', "ge"),
    ('h', "hache"),
    ('j', "jota"),
    ('k', "ka"),
    ('l', "ele"),
    ('m', "eme"),
    ('n', "ene"),
    ('ñ', "eñe"),
    ('p', "pe"),
    ('q', "cu"),
    ('r', "erre"),
    ('s', "ese"),
    ('t', "te"),
    ('v', "uve"),
    ('w', "uve doble"),
    ('x', "equis"),
    ('z', "zeta"),
];

/// Sentences of `tokens` as said in Castilian.
pub(crate) fn read(tokens: &[Token<'_>]) -> Vec<Spoken> {
    let mut spoken = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let space = token.space;
        let said: Vec<String> = match token.piece {
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
            Piece::Number(text) => number_words(text),
            Piece::Word(text) => {
                let dot = tokens.get(at + 1).map(|t| t.piece) == Some(Piece::Mark('.'));
                spelled(text, dot)
            }
        };
        for (i, word) in said.iter().enumerate() {
            spoken.push(Spoken {
                part: Part::Word(word_sounds(word)),
                space: space || i > 0,
            });
        }
    }
    stress_before_pauses(&mut spoken);
    link(&mut spoken);
    spoken
}

/// A written word as the words said for it.
fn spelled(text: &str, dot: bool) -> Vec<String> {
    let word: String = letters(text).into_iter().filter(|&c| c != '\'').collect();
    if dot && let Some((_, full)) = SHORT.iter().find(|(short, _)| *short == word) {
        return vec![(*full).to_owned()];
    }
    if word.chars().any(is_vowel) {
        return vec![word];
    }
    let name = |c: char| NAMES.iter().find(|(l, _)| *l == c).map(|(_, n)| *n);
    let names = word.chars().filter_map(name);
    names
        .flat_map(str::split_whitespace)
        .map(str::to_owned)
        .collect()
}

fn is_vowel(c: char) -> bool {
    "aeiouáéíóúüàèìòùy".contains(c)
}

/// A spelled-out piece before its sounds are known.
#[derive(Clone, Copy)]
enum Piece2 {
    Vowel {
        sound: char,
        accent: bool,
        weak: Weak,
    },
    Consonant(&'static str),
    /// A written `h`, or a seam in a number: vowels stay apart.
    Apart,
}

/// What an unstressed `i` or `u` does before another vowel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Weak {
    /// Glides into it, as `bien`.
    Glides,
    /// Said in full but no syllable, after a leading `r` or `l`: `rueda`.
    Full,
    /// A syllable of its own, as `ü` or `y` alone.
    Never,
}

/// A word's sounds with its stress; `-mente` keeps both stresses.
fn word_sounds(word: &str) -> Vec<Sound> {
    let w = letters(word);
    let stem = w.len().checked_sub(5).and_then(|n| w.get(..n));
    if w.ends_with(&['m', 'e', 'n', 't', 'e'])
        && let Some(stem) = stem.filter(|s| s.len() > 3 && s.iter().any(|&c| is_vowel(c)))
    {
        let mut sounds = stressed(stem, false);
        let mut mente = stressed(&['m', 'e', 'n', 't', 'e'], false);
        close_e(&mut mente);
        sounds.extend(mente);
        return sounds;
    }
    let weak = WEAK.contains(&word);
    let mut sounds = stressed(&w, weak);
    if word == "ser"
        && let Some(last) = sounds.last_mut()
    {
        last.ipa = "r";
    }
    if LIGHT.contains(&word) {
        close_e(&mut sounds);
        for sound in &mut sounds {
            if sound.stress == Stress::Primary {
                sound.stress = Stress::Secondary;
            }
        }
    }
    sounds
}

/// Undoes [`open_e`] where espeak keeps `e` closed.
fn close_e(sounds: &mut [Sound]) {
    for sound in sounds.iter_mut().filter(|s| s.ipa == "ɛ") {
        sound.ipa = "e";
    }
}

/// `w` as sounds, stressed by the spelling unless `weak`.
fn stressed(w: &[char], weak: bool) -> Vec<Sound> {
    let pieces = spell(w);
    let (mut sounds, nuclei) = join_vowels(&pieces);
    assimilate(&mut sounds);
    if weak || nuclei.is_empty() {
        return sounds;
    }
    let open = w
        .last()
        .is_some_and(|&c| c != 'y' && is_vowel(c) || c == 'n' || c == 's');
    let counted: Vec<&Nucleus> = nuclei.iter().filter(|n| n.counts).collect();
    let accented = counted.iter().rposition(|n| n.accent);
    let count = counted.len();
    let primary = accented.unwrap_or(match (count, open) {
        (0 | 1, _) | (_, false) => count.saturating_sub(1),
        (_, true) => count - 2,
    });
    let Some(peak) = counted.get(primary).map(|n| n.at) else {
        return sounds;
    };
    let full = nuclei.iter().position(|n| n.at == peak).unwrap_or_default();
    let late = (primary + 3 < count).then(|| counted.get(count - 2).map(|n| n.at));
    for (i, nucleus) in nuclei.iter().enumerate() {
        let early = i + 1 < full && i % 2 == 0 && nucleus.counts;
        let lighter = early || late.flatten() == Some(nucleus.at);
        let Some(sound) = sounds.get_mut(nucleus.at) else {
            continue;
        };
        if nucleus.at == peak {
            sound.stress = Stress::Primary;
        } else if lighter {
            sound.stress = Stress::Secondary;
        }
    }
    open_e(&mut sounds);
    sounds
}

/// Letters to pieces, digraphs and silent letters resolved.
fn spell(w: &[char]) -> Vec<Piece2> {
    let at = |k: usize| w.get(k).copied();
    let vowel = |k: usize| at(k).is_some_and(is_vowel);
    let mut pieces = Vec::with_capacity(w.len());
    let mut push = |p: Piece2| pieces.push(p);
    let c = Piece2::Consonant;
    let mut i = 0;
    while let Some(ch) = at(i) {
        let prev = i.checked_sub(1).and_then(at);
        let next = at(i + 1);
        let front = matches!(next, Some('e' | 'i' | 'é' | 'í'));
        let mut step = 1;
        let liquid = matches!(prev, Some('r' | 'l'))
            && i.checked_sub(2).and_then(at).is_none_or(|c| !is_vowel(c));
        let weak = if liquid { Weak::Full } else { Weak::Glides };
        let v = |sound, accent| Piece2::Vowel {
            sound,
            accent,
            weak,
        };
        match ch {
            'a' | 'à' | 'â' | 'ä' => push(v('a', false)),
            'á' => push(v('a', true)),
            'e' | 'è' | 'ê' | 'ë' => push(v('e', false)),
            'é' => push(v('e', true)),
            'i' | 'ì' | 'î' | 'ï' => push(v('i', false)),
            'í' => push(v('i', true)),
            'o' | 'ò' | 'ô' | 'ö' => push(v('o', false)),
            'ó' => push(v('o', true)),
            'u' | 'ù' | 'û' => {
                let silent = prev == Some('q')
                    || prev == Some('g') && matches!(next, Some('e' | 'i' | 'é' | 'í'));
                if !silent {
                    push(v('u', false));
                }
            }
            'ú' => push(v('u', true)),
            'ü' => push(Piece2::Vowel {
                sound: 'u',
                accent: false,
                weak: Weak::Never,
            }),
            'y' if w.len() == 1 => push(Piece2::Vowel {
                sound: 'i',
                accent: false,
                weak: Weak::Never,
            }),
            'y' if vowel(i + 1) => push(c("ʝ")),
            'y' if prev == Some('u') && next.is_none() => push(c("j")),
            'y' => push(v('i', false)),
            'h' | '-' => push(Piece2::Apart),
            'c' if next == Some('h') => {
                push(c("t^ʃ"));
                step = 2;
            }
            'c' if front => push(c("θ")),
            'c' | 'q' | 'k' => push(c("k")),
            'g' if front => push(c("x")),
            'g' => push(c("ɡ")),
            'j' => push(c("x")),
            'z' => push(c("θ")),
            'ñ' => push(c("ɲ")),
            'b' | 'v' => push(c("b")),
            'd' => push(c("d")),
            'f' => push(c("f")),
            'm' => push(c("m")),
            'n' => push(c("n")),
            'p' if i == 0 && next == Some('s') => {}
            'p' if next == Some('t') => push(c("pː")),
            'p' => push(c("p")),
            's' | 'ç' => push(c("s")),
            't' => push(c("t")),
            'w' => push(c("w")),
            'l' if next == Some('l') => {
                push(c("ʎ"));
                step = 2;
            }
            'l' => push(c("l")),
            'r' if next == Some('r') => {
                push(c("r"));
                step = 2;
            }
            'r' if i == 0 || matches!(prev, Some('l' | 'n' | 's')) => push(c("r")),
            'r' => push(c("ɾ")),
            'x' if i == 0 => push(c("s")),
            'x' => {
                push(c("k"));
                push(c("s"));
            }
            _ => {}
        }
        i += step;
    }
    pieces
}

/// A vowel said as a syllable's peak.
struct Nucleus {
    at: usize,
    accent: bool,
    /// False for [`Weak::Full`]: it counts for lighter stress only.
    counts: bool,
}

/// Vowels joined into syllables; an accented `e` stays closed as `é`.
fn join_vowels(pieces: &[Piece2]) -> (Vec<Sound>, Vec<Nucleus>) {
    let mut sounds: Vec<Sound> = Vec::with_capacity(pieces.len());
    let mut nuclei: Vec<Nucleus> = Vec::new();
    let mut apart = false;
    for (i, piece) in pieces.iter().enumerate() {
        let (sound, accent, kind) = match *piece {
            Piece2::Consonant(ipa) => {
                sounds.push(Sound::consonant(ipa));
                continue;
            }
            Piece2::Apart => {
                apart = true;
                continue;
            }
            Piece2::Vowel {
                sound,
                accent,
                weak,
            } => (sound, accent, weak),
        };
        let after_vowel = !apart && sounds.last().is_some_and(|s| s.vowel);
        apart = false;
        let weak = matches!(sound, 'i' | 'u') && !accent && kind != Weak::Never;
        let before_vowel = matches!(pieces.get(i + 1), Some(Piece2::Vowel { .. }));
        if weak && before_vowel && kind == Weak::Glides {
            sounds.push(Sound::consonant(if sound == 'i' { "j" } else { "w" }));
            continue;
        }
        if weak && before_vowel {
            nuclei.push(Nucleus {
                at: sounds.len(),
                accent,
                counts: false,
            });
            sounds.push(Sound::vowel(vowel_ipa(sound)));
            continue;
        }
        let last_at = sounds.len().checked_sub(1);
        let strong = sounds
            .last()
            .filter(|s| matches!(s.ipa, "a" | "e" | "é" | "o"));
        if weak && after_vowel && strong.is_some() {
            let stressed = nuclei
                .last()
                .is_some_and(|n| n.accent && Some(n.at) == last_at);
            match (stressed, sounds.last_mut()) {
                (false, Some(last)) => last.ipa = falling(last.ipa, sound),
                _ => sounds.push(Sound::consonant(vowel_ipa(sound))),
            }
            continue;
        }
        nuclei.push(Nucleus {
            at: sounds.len(),
            accent,
            counts: true,
        });
        let ipa = if sound == 'e' && accent {
            "é"
        } else {
            vowel_ipa(sound)
        };
        sounds.push(Sound::vowel(ipa));
    }
    (sounds, nuclei)
}

fn vowel_ipa(sound: char) -> &'static str {
    match sound {
        'a' => "a",
        'e' => "e",
        'i' => "i",
        'o' => "o",
        _ => "u",
    }
}

/// A strong vowel closing on `i` or `u` in one syllable.
fn falling(first: &str, then: char) -> &'static str {
    match (first, then) {
        ("a", 'i') => "a^ɪ",
        ("e" | "é", 'i') => "e^ɪ",
        ("o", 'i') => "o^ɪ",
        ("a", _) => "a^ʊ",
        ("e" | "é", _) => "e^ʊ",
        _ => "o^ʊ",
    }
}

/// `n` takes the place of the consonant after it.
fn assimilate(sounds: &mut [Sound]) {
    for i in 1..sounds.len() {
        let next = sounds.get(i).map(|s| s.ipa);
        let cluster = matches!(sounds.get(i + 1).map(|s| s.ipa), Some("ɾ" | "l"));
        if let (Some(sound), Some(next)) = (sounds.get_mut(i - 1), next) {
            sound.ipa = match (sound.ipa, next) {
                ("n", "x") => "ŋ",
                ("n", "f") if cluster => "m",
                (ipa, next) => nasal(ipa, next),
            };
        }
    }
}

fn nasal(ipa: &'static str, next: &str) -> &'static str {
    match (ipa, next) {
        ("n", "b" | "p") => "m",
        ("n", "ɡ") => "ŋ",
        _ => ipa,
    }
}

/// A stressed `e` opens before `n` and another consonant.
fn open_e(sounds: &mut [Sound]) {
    for i in 0..sounds.len() {
        let closed = matches!(sounds.get(i + 1).map(|s| s.ipa), Some("n" | "ŋ"))
            && sounds
                .get(i + 2)
                .is_some_and(|s| !s.vowel && !matches!(s.ipa, "j" | "w"));
        if let Some(sound) = sounds.get_mut(i) {
            let open = sound.stress == Stress::Primary && closed;
            sound.ipa = match sound.ipa {
                "é" => "e",
                "e" if open => "ɛ",
                ipa => ipa,
            };
        }
    }
}

/// A weak or light word before a pause takes full stress, but `y`.
fn stress_before_pauses(spoken: &mut [Spoken]) {
    for i in 0..spoken.len() {
        let pause = !matches!(spoken.get(i + 1).map(|s| &s.part), Some(Part::Word(_)));
        let Some(Part::Word(sounds)) = spoken.get_mut(i).map(|s| &mut s.part) else {
            continue;
        };
        let y = sounds.len() == 1 && sounds.iter().all(|s| s.ipa == "i");
        if !pause || y {
            continue;
        }
        if sounds.iter().any(|s| s.stress == Stress::Primary) {
            continue;
        }
        let lighter = sounds.iter().position(|s| s.stress == Stress::Secondary);
        let at = lighter.or(sounds.iter().position(|s| s.vowel));
        if let Some(sound) = at.and_then(|at| sounds.get_mut(at)) {
            sound.stress = Stress::Primary;
        }
    }
}

/// Sounds that change with the word before or after.
fn link(spoken: &mut [Spoken]) {
    let mut before: Option<&'static str> = None;
    for p in 0..spoken.len() {
        let next_first = match spoken.get(p + 1).map(|s| &s.part) {
            Some(Part::Word(sounds)) => sounds.first().copied(),
            _ => None,
        };
        let Some(Part::Word(sounds)) = spoken.get_mut(p).map(|s| &mut s.part) else {
            before = None;
            continue;
        };
        let count = sounds.len();
        if let (Some(last), Some(next)) = (sounds.last_mut(), next_first) {
            last.ipa = nasal(last.ipa, next.ipa);
        }
        for i in 0..count {
            let after = sounds.get(i + 1).map(|s| s.ipa);
            let Some(sound) = sounds.get_mut(i) else {
                continue;
            };
            let held = before.is_none_or(|b| matches!(b, "m" | "n" | "ɲ" | "ŋ"))
                || matches!(after, Some("p" | "pː" | "t" | "k" | "m" | "n" | "ɲ"))
                || i == 0 && sound.ipa == "d" && after == Some("w");
            sound.ipa = match sound.ipa {
                "d" if i + 1 == count => match next_first.is_some_and(|s| s.vowel) {
                    true => "ð",
                    false => "d",
                },
                "b" | "d" | "ɡ" if held => sound.ipa,
                "b" => "β",
                "d" => "ð",
                "ɡ" => "ɣ",
                ipa => ipa,
            };
            before = Some(sound.ipa);
        }
    }
}

const ONES: [&str; 30] = [
    "cero",
    "uno",
    "dos",
    "tres",
    "cuatro",
    "cinco",
    "seis",
    "siete",
    "ocho",
    "nueve",
    "diez",
    "once",
    "doce",
    "trece",
    "catorce",
    "quince",
    "dieciséis",
    "diecisiete",
    "dieciocho",
    "diecinueve",
    "veinte",
    "veinti-uno",
    "veintidós",
    "veintitrés",
    "veinticuatro",
    "veinticinco",
    "veintiséis",
    "veintisiete",
    "veintiocho",
    "veintinueve",
];

const TENS: [&str; 10] = [
    "",
    "",
    "",
    "treinta",
    "cuarenta",
    "cincuenta",
    "sesenta",
    "setenta",
    "ochenta",
    "noventa",
];

const HUNDREDS: [&str; 10] = [
    "",
    "ciento",
    "doscientos",
    "trescientos",
    "cuatrocientos",
    "quinientos",
    "seiscientos",
    "setecientos",
    "ochocientos",
    "novecientos",
];

fn pick(table: &[&'static str], i: u64) -> &'static str {
    let i = usize::try_from(i).unwrap_or(usize::MAX);
    table.get(i).copied().unwrap_or_default()
}

/// Digits as the words said for them.
fn number_words(text: &str) -> Vec<String> {
    let written = number(text);
    let mut words = match written.whole {
        Some(n) => cardinal(n),
        None => digits(text.split([',', '.']).next().unwrap_or_default()),
    };
    if let Some(fraction) = written.fraction {
        words.push("coma".into());
        let short = fraction.len() <= 2 && !fraction.starts_with('0');
        match fraction.parse::<u64>() {
            Ok(n) if short => words.extend(cardinal(n)),
            _ => words.extend(digits(fraction)),
        }
    }
    words
}

fn digits(text: &str) -> Vec<String> {
    let digit = |b: u8| pick(&ONES, u64::from(b - b'0')).to_owned();
    text.bytes().filter(u8::is_ascii_digit).map(digit).collect()
}

/// `n` in words, below a million million.
fn cardinal(n: u64) -> Vec<String> {
    if n == 0 {
        return vec!["cero".into()];
    }
    let mut words = Vec::new();
    let millions = n / 1_000_000;
    match millions {
        0 => {}
        1 => words.extend(["un".to_owned(), "millón".to_owned()]),
        _ => {
            words.extend(thousands(millions, true));
            words.push("millones".into());
        }
    }
    words.extend(thousands(n % 1_000_000, false));
    words
}

/// `n` below a million; `before_noun` shortens a last `uno`.
fn thousands(n: u64, before_noun: bool) -> Vec<String> {
    let mut words = Vec::new();
    match n / 1000 {
        0 => {}
        1 => words.push("mil".into()),
        k => {
            words.extend(hundreds(k, true));
            words.push("mil".into());
        }
    }
    words.extend(hundreds(n % 1000, before_noun));
    words
}

fn hundreds(n: u64, before_noun: bool) -> Vec<String> {
    if n == 100 {
        return vec!["cien".into()];
    }
    let mut words: Vec<String> = Vec::new();
    if n >= 100 {
        words.push(pick(&HUNDREDS, n / 100).into());
    }
    match n % 100 {
        0 => {}
        r @ 1..30 => words.push(pick(&ONES, r).into()),
        r => {
            words.push(pick(&TENS, r / 10).into());
            if r % 10 > 0 {
                words.push("y".into());
                words.push(pick(&ONES, r % 10).into());
            }
        }
    }
    if before_noun && let Some(last) = words.last_mut() {
        match last.as_str() {
            "uno" => *last = "un".into(),
            "veinti-uno" => *last = "veinti-ún".into(),
            _ => {}
        }
    }
    words
}
