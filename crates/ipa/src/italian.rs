//! Italian: spelling to sounds, stress, and the vowels' open or closed.

use crate::letters::{letters, number};
use crate::text::{Piece, Token};
use crate::{Part, Sound, Spoken, Stress};

/// Words said without stress: articles, clitics and short links.
const WEAK: &[&str] = &[
    "il", "lo", "la", "i", "gli", "le", "un", "di", "da", "in", "con", "su", "per", "tra", "fra",
    "e", "ed", "o", "od", "ma", "che", "non", "si", "mi", "ti", "ci", "vi", "ne", "se", "del",
    "al", "dal", "nel", "sul", "è", "ha", "a", "ho", "chi", "me", "te", "tu", "col", "ai", "mai",
    "de", "quel", "più", "fu", "ce", "ad", "li", "ve",
];

/// Words said with a lighter stress than others.
const LIGHT: &[&str] = &[
    "una", "della", "alla", "dalla", "sulla", "dello", "allo", "dallo", "nello", "sullo", "degli",
    "agli", "dagli", "negli", "sugli", "delle", "alle", "dalle", "sulle", "sono", "era", "hanno",
    "questo", "questa", "quella", "nostro", "lui", "lei", "loro", "noi", "come", "anche", "aveva",
    "suo", "sua", "suoi", "senza", "ogni", "mio", "mia", "buon", "sarebbe", "sarà", "sia",
    "questi", "queste", "quelle", "avete", "nessuno", "sue", "dei", "sei", "siete", "vuol",
    "verso", "mentre", "pure", "siamo", "avrebbe",
];

/// Common words respelled with a dictionary's accents, for stress
/// and open vowels the spelling hides.
const RESPELT: &[(&str, &str)] = &[
    ("albero", "àlbero"),
    ("angelo", "àngelo"),
    ("anima", "ànima"),
    ("aperto", "apèrto"),
    ("aveva", "avéva"),
    ("avrebbe", "avrèbbe"),
    ("bella", "bèlla"),
    ("belle", "bèlle"),
    ("belli", "bèlli"),
    ("bello", "bèllo"),
    ("ben", "bèn"),
    ("bene", "bène"),
    ("bestia", "bèstia"),
    ("camera", "càmera"),
    ("certa", "cèrta"),
    ("certo", "cèrto"),
    ("chiudere", "chiùdere"),
    ("conoscere", "conóscere"),
    ("corpo", "còrpo"),
    ("correre", "córrere"),
    ("cosa", "còsa"),
    ("cose", "còse"),
    ("credere", "crédere"),
    ("crescere", "créscere"),
    ("debole", "débole"),
    ("dentro", "déntro"),
    ("difficile", "diffìcile"),
    ("era", "èra"),
    ("erano", "èrano"),
    ("eri", "èri"),
    ("esser", "èsser"),
    ("essere", "èssere"),
    ("facile", "fàcile"),
    ("ferro", "fèrro"),
    ("festa", "fèsta"),
    ("forte", "fòrte"),
    ("giovane", "gióvane"),
    ("gloria", "glòria"),
    ("guerra", "guèrra"),
    ("lacrime", "làcrime"),
    ("lagrime", "làgrime"),
    ("leggere", "lèggere"),
    ("lettera", "lèttera"),
    ("letto", "lètto"),
    ("loro", "lòro"),
    ("macchina", "màcchina"),
    ("medesimo", "medésimo"),
    ("meglio", "mèglio"),
    ("memoria", "memòria"),
    ("mentre", "méntre"),
    ("mettere", "méttere"),
    ("mezza", "mèzza"),
    ("mezzo", "mèzzo"),
    ("modo", "mòdo"),
    ("morte", "mòrte"),
    ("muovere", "muòvere"),
    ("nascere", "nàscere"),
    ("noi", "nói"),
    ("nostra", "nòstra"),
    ("nostro", "nòstro"),
    ("notte", "nòtte"),
    ("nove", "nòve"),
    ("numero", "nùmero"),
    ("occhi", "òcchi"),
    ("occhio", "òcchio"),
    ("oggi", "òggi"),
    ("ordine", "órdine"),
    ("oro", "òro"),
    ("parola", "paròla"),
    ("parole", "paròle"),
    ("perdere", "pèrdere"),
    ("piangere", "piàngere"),
    ("piccola", "pìccola"),
    ("piccoli", "pìccoli"),
    ("piccolo", "pìccolo"),
    ("pinocchio", "pinòcchio"),
    ("poca", "pòca"),
    ("poche", "pòche"),
    ("pochi", "pòchi"),
    ("poco", "pòco"),
    ("porta", "pòrta"),
    ("porte", "pòrte"),
    ("possa", "pòssa"),
    ("posso", "pòsso"),
    ("posto", "pòsto"),
    ("povera", "pòvera"),
    ("poveri", "pòveri"),
    ("povero", "pòvero"),
    ("prendere", "prèndere"),
    ("presto", "prèsto"),
    ("prete", "prète"),
    ("propria", "pròpria"),
    ("proprio", "pròprio"),
    ("qualcosa", "qualcòsa"),
    ("ridere", "rìdere"),
    ("rispondere", "rispóndere"),
    ("roba", "ròba"),
    ("rompere", "rómpere"),
    ("sarebbe", "sarèbbe"),
    ("scegliere", "scégliere"),
    ("scrivere", "scrìvere"),
    ("secolo", "sècolo"),
    ("sedia", "sèdia"),
    ("sette", "sètte"),
    ("siete", "siéte"),
    ("simile", "sìmile"),
    ("so", "sò"),
    ("storia", "stòria"),
    ("subito", "sùbito"),
    ("tavola", "tàvola"),
    ("tavolo", "tàvolo"),
    ("termine", "tèrmine"),
    ("terra", "tèrra"),
    ("testa", "tèsta"),
    ("troppo", "tròppo"),
    ("ultima", "ùltima"),
    ("ultimo", "ùltimo"),
    ("utile", "ùtile"),
    ("vecchia", "vècchia"),
    ("vecchio", "vècchio"),
    ("vendere", "véndere"),
    ("verso", "vèrso"),
    ("virgola", "vìrgola"),
    ("vivere", "vìvere"),
    ("voi", "vói"),
    ("volta", "vòlta"),
    ("volte", "vòlte"),
    ("vorrei", "vorrèi"),
    ("vostra", "vòstra"),
    ("vostro", "vòstro"),
];

/// Endings that put the stress a syllable further back.
const EARLY: &[&str] = &[
    "ico", "ica", "ici", "iche", "abile", "ibile", "abili", "ibili", "evole", "issimo", "issima",
    "issimi", "issime", "vano", "ono", "ebbero",
];

/// Sentences of `tokens` as said in Italian.
pub(crate) fn read(tokens: &[Token<'_>]) -> Vec<Spoken> {
    let mut spoken = Vec::new();
    for token in tokens {
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
            Piece::Word(text) => vec![text.to_owned()],
        };
        for (i, word) in said.iter().enumerate() {
            spoken.push(Spoken {
                part: Part::Word(word_sounds(word)),
                space: space || i > 0,
            });
        }
    }
    stress_before_pauses(&mut spoken);
    spoken
}

fn is_vowel(c: char) -> bool {
    "aeiouàèéìíòóùú".contains(c)
}

/// A word's sounds; a part cut short by an apostrophe joins unstressed.
fn word_sounds(word: &str) -> Vec<Sound> {
    let mut w = letters(word);
    while w.last() == Some(&'\'') {
        w.pop();
    }
    let mut parts = w.split(|&c| c == '\'').peekable();
    let mut sounds = Vec::new();
    while let Some(part) = parts.next() {
        let next = parts.peek();
        let text: String = part.iter().collect();
        let soft = next.is_some_and(|n| matches!(n.first(), Some('e' | 'i' | 'è' | 'é')));
        if soft && matches!(text.as_str(), "c" | "g") {
            let ipa = if text == "c" { "t^ʃ" } else { "d^ʒ" };
            sounds.push(Sound::consonant(ipa));
            continue;
        }
        let elided = !sounds.is_empty() && text != "è";
        let weak = next.is_some() || !elided && WEAK.contains(&text.as_str());
        let respelt = RESPELT.binary_search_by_key(&text.as_str(), |(word, _)| word);
        let mut said = match respelt.ok().and_then(|at| RESPELT.get(at)) {
            Some((_, spelt)) => stressed(&spelt.chars().collect::<Vec<_>>(), weak),
            None => stressed(part, weak),
        };
        if !elided && LIGHT.contains(&text.as_str()) {
            for sound in said.iter_mut().filter(|s| s.vowel) {
                if sound.stress == Stress::Primary {
                    sound.stress = Stress::Secondary;
                }
                if sound.ipa == "u" {
                    sound.ipa = "ʊ";
                }
            }
        }
        sounds.extend(said);
    }
    sounds
}

/// A spelled-out piece before its sounds are known.
#[derive(Clone, Copy)]
enum Piece2 {
    Vowel { sound: char, accent: Option<char> },
    Consonant(&'static str),
}

/// `w` as sounds, stressed by the spelling unless `weak`.
fn stressed(w: &[char], weak: bool) -> Vec<Sound> {
    let pieces = spell(w);
    let (mut sounds, nuclei) = join_vowels(&pieces, w);
    if nuclei.is_empty() {
        return sounds;
    }
    let count = nuclei.len();
    let accented = nuclei.iter().rposition(|n| n.accent.is_some());
    let text: String = w.iter().collect();
    let early = count > 2 && EARLY.iter().any(|end| text.ends_with(end));
    let consonant_end = w.last().is_some_and(|&c| c.is_alphabetic() && !is_vowel(c));
    let primary = match (accented, count) {
        (Some(at), _) => at,
        (None, 1) => 0,
        (None, _) if consonant_end => count - 1,
        (None, _) if early => count - 3,
        (None, _) => count - 2,
    };
    if !weak {
        for (i, nucleus) in nuclei.iter().enumerate() {
            if let Some(sound) = sounds.get_mut(nucleus.at) {
                if i == primary {
                    sound.stress = Stress::Primary;
                }
                sound.ipa = quality(sound.ipa, nucleus, i == primary, count - i);
            }
        }
    }
    short_gn(&mut sounds);
    lax_end(&mut sounds);
    sounds
}

/// A vowel said as a syllable's peak.
struct Nucleus {
    at: usize,
    accent: Option<char>,
    /// What comes before: `j` or `w` glides open the vowel after.
    after_glide: bool,
    /// Whether `n` or `m` and another consonant follow.
    nasal: bool,
    /// Whether an `i` or `u` closes its syllable, as in `poi`.
    falling: bool,
    ending: Ending,
}

/// Endings whose stressed `e` is known.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// `-mento`, `-mente`: closed.
    Mento,
    /// `-eva`, `-evano`: open, as espeak says them.
    Eva,
    /// A last `-er`, cut from `-ere`: open.
    Er,
    Other,
}

/// Letters to pieces, digraphs and silent letters resolved.
fn spell(w: &[char]) -> Vec<Piece2> {
    let at = |k: usize| w.get(k).copied();
    let vowel = |k: usize| at(k).is_some_and(is_vowel);
    let front = |k: usize| matches!(at(k), Some('e' | 'i' | 'è' | 'é' | 'ì' | 'í'));
    let mut pieces = Vec::with_capacity(w.len());
    let mut push = |p: Piece2| pieces.push(p);
    let c = Piece2::Consonant;
    let mut i = 0;
    while let Some(ch) = at(i) {
        let prev = i.checked_sub(1).and_then(at);
        let next = at(i + 1);
        let double = next == Some(ch);
        let mut step = if double { 2 } else { 1 };
        // `i` after these only softens them, before another vowel.
        let soft_i = |k: usize| at(k) == Some('i') && vowel(k + 1);
        let v = |sound, accent| Piece2::Vowel { sound, accent };
        match ch {
            'a' | 'e' | 'i' | 'o' | 'u' => {
                step = 1;
                push(v(ch, None));
            }
            'à' | 'á' => push(v('a', Some('à'))),
            'è' => push(v('e', Some('è'))),
            'é' => push(v('e', Some('é'))),
            'ì' | 'í' | 'î' => push(v('i', Some('ì'))),
            'ò' => push(v('o', Some('ò'))),
            'ó' => push(v('o', Some('ó'))),
            'ù' | 'ú' => push(v('u', Some('ù'))),
            'c' if next == Some('q') => {}
            'c' | 'g' => {
                let hard = if ch == 'c' { "k" } else { "ɡ" };
                let soft = if ch == 'c' { "t^ʃ" } else { "d^ʒ" };
                let k = if double { i + 2 } else { i + 1 };
                let long = |s: &'static str| match (double, s) {
                    (false, s) => s,
                    (true, "k") => "kː",
                    (true, "ɡ") => "ɡː",
                    (true, "t^ʃ") => "t^ʃː",
                    _ => "d^ʒː",
                };
                if ch == 'g' && !double && next == Some('n') {
                    push(c(if i == 0 { "ɲ" } else { "ɲɲ" }));
                    step = 2;
                } else if ch == 'g' && !double && next == Some('l') && at(i + 2) == Some('i') {
                    push(c("ʎ"));
                    step = if vowel(i + 3) { 3 } else { 2 };
                } else if at(k) == Some('h') {
                    push(c(long(hard)));
                    step = k + 1 - i;
                } else if front(k) {
                    push(c(long(soft)));
                    step = if soft_i(k) { k + 1 - i } else { k - i };
                } else if ch == 'g' && at(k) == Some('u') && vowel(k + 1) {
                    push(c(long(hard)));
                    push(c("w"));
                    step = k + 1 - i;
                } else {
                    push(c(long(hard)));
                    step = k - i;
                }
            }
            'q' => {
                push(c(if prev == Some('c') { "kː" } else { "k" }));
                if next == Some('u') {
                    push(c("w"));
                    step = 2;
                }
            }
            's' if next == Some('c') && front(i + 2) => {
                push(c("ʃ"));
                step = if soft_i(i + 2) { 3 } else { 2 };
            }
            's' if double => push(c("s^s")),
            's' => {
                let voiced_next =
                    matches!(next, Some('b' | 'd' | 'g' | 'l' | 'm' | 'n' | 'r' | 'v'));
                let between = i > 0 && prev.is_some_and(is_vowel) && vowel(i + 1);
                push(c(if voiced_next || between { "z" } else { "s" }));
            }
            'z' if double => push(c("t^sː")),
            'z' if i == 0 => push(c("d^z")),
            'z' => push(c("t^s")),
            'p' | 'b' | 't' | 'd' | 'k' => {
                let one = match ch {
                    'p' => ("p", "pː"),
                    'b' => ("b", "bː"),
                    't' => ("t", "tː"),
                    'd' => ("d", "dː"),
                    _ => ("k", "kː"),
                };
                push(c(if double { one.1 } else { one.0 }));
            }
            'f' => push(c(if double { "ff" } else { "f" })),
            'l' => push(c(if double { "ll" } else { "l" })),
            'm' => push(c(if double { "mm" } else { "m" })),
            'n' => push(c(if double { "nn" } else { "n" })),
            'v' => push(c(if double { "vv" } else { "v" })),
            'r' => push(c(if double { "rɾ" } else { "r" })),
            'h' => {}
            'j' => push(c("j")),
            'w' => push(c("w")),
            'x' => {
                push(c("k"));
                push(c("s"));
            }
            'y' => push(v('i', None)),
            _ => {}
        }
        i += step;
    }
    pieces
}

/// Vowels joined into syllables: sounds, and each nucleus.
fn join_vowels(pieces: &[Piece2], w: &[char]) -> (Vec<Sound>, Vec<Nucleus>) {
    let mut sounds: Vec<Sound> = Vec::with_capacity(pieces.len());
    let mut nuclei: Vec<Nucleus> = Vec::new();
    let last = pieces.len().saturating_sub(1);
    let ends = |tail: &[char]| w.ends_with(tail);
    let final_ia = ends(&['i', 'a']) || ends(&['i', 'o']);
    let vowels = pieces
        .iter()
        .filter(|p| matches!(p, Piece2::Vowel { .. }))
        .count();
    let ends_in_vowel = matches!(pieces.last(), Some(Piece2::Vowel { .. }));
    for (i, piece) in pieces.iter().enumerate() {
        let (sound, accent) = match *piece {
            Piece2::Consonant(ipa) => {
                sounds.push(Sound::consonant(ipa));
                continue;
            }
            Piece2::Vowel { sound, accent } => (sound, accent),
        };
        let before_vowel = matches!(pieces.get(i + 1), Some(Piece2::Vowel { .. }));
        let after_vowel = sounds.last().is_some_and(|s| s.vowel);
        let weak = matches!(sound, 'i' | 'u') && accent.is_none();
        let after_consonant = i > 0 && !after_vowel;
        let two = vowels == 2 && ends_in_vowel && i + 1 == last;
        let final_ui = sound == 'u' && i + 1 == last && w.ends_with(&['u', 'i']);
        let accented_next = matches!(
            pieces.get(i + 1),
            Some(Piece2::Vowel {
                accent: Some(_),
                ..
            })
        );
        if weak && before_vowel && two && accented_next && sound == 'u' {
            sounds.push(Sound::consonant("ʊ"));
            continue;
        }
        if weak && before_vowel && (two && !(sound == 'i' && accented_next) || final_ui) {
            nuclei.push(Nucleus {
                at: sounds.len(),
                accent,
                after_glide: false,
                nasal: false,
                falling: false,
                ending: Ending::Other,
            });
            sounds.push(Sound::vowel(if sound == 'i' { "i" } else { "u" }));
            continue;
        }
        if weak && before_vowel && sound == 'i' && i + 1 == last && final_ia && after_consonant {
            sounds.push(Sound::consonant("i"));
            continue;
        }
        let after_z = matches!(sounds.last().map(|s| s.ipa), Some("t^s" | "t^sː" | "d^z"));
        let full_i = sound == 'i' && after_z && !(i + 1 == last && w.ends_with(&['e']));
        let full_u = sound == 'u' && after_consonant;
        if weak && before_vowel && (full_i || full_u) {
            sounds.push(Sound::consonant(if full_i { "i" } else { "ʊ" }));
            continue;
        }
        if weak && before_vowel && (sound == 'i' || after_consonant || i == 0) {
            sounds.push(Sound::consonant(if sound == 'i' { "j" } else { "w" }));
            continue;
        }
        if weak && after_vowel && !before_vowel {
            let after_a = sounds.last().is_some_and(|s| s.ipa == "a");
            let ipa = match sound {
                'i' if after_a => "j",
                'i' => "i",
                _ => "u",
            };
            sounds.push(Sound::consonant(ipa));
            continue;
        }
        let glide = sounds
            .len()
            .checked_sub(1)
            .and_then(|k| sounds.get(k))
            .map(|s| s.ipa);
        let before_glide = sounds
            .len()
            .checked_sub(2)
            .and_then(|k| sounds.get(k))
            .map(|s| s.ipa);
        let after_glide = glide == Some("j")
            || glide == Some("ʊ")
            || glide == Some("w") && !matches!(before_glide, Some("k" | "kː" | "ɡ"));
        let rest = pieces.get(i + 1..).unwrap_or_default();
        let nasal = matches!(rest.first(), Some(Piece2::Consonant("n" | "m")))
            && matches!(rest.get(1), Some(Piece2::Consonant(_)));
        let falling = matches!(
            rest.first(),
            Some(Piece2::Vowel {
                sound: 'i' | 'u',
                accent: None
            })
        ) && !matches!(rest.get(1), Some(Piece2::Vowel { .. }));
        let ending = match rest {
            [
                Piece2::Consonant("n"),
                Piece2::Consonant("t"),
                Piece2::Vowel { .. },
            ] if glide == Some("m") => Ending::Mento,
            [
                Piece2::Consonant("v"),
                Piece2::Vowel { sound: 'a', .. },
                tail @ ..,
            ] if tail.len() <= 2 => Ending::Eva,
            [Piece2::Consonant("r")] => Ending::Er,
            _ => Ending::Other,
        };
        nuclei.push(Nucleus {
            at: sounds.len(),
            accent,
            after_glide,
            nasal,
            falling,
            ending,
        });
        sounds.push(Sound::vowel(match sound {
            'a' => "a",
            'e' => "e",
            'i' => "i",
            'o' => "o",
            _ => "u",
        }));
    }
    (sounds, nuclei)
}

/// A vowel's sound: stressed `e` and `o` open or close.
fn quality(ipa: &'static str, nucleus: &Nucleus, stressed: bool, from_end: usize) -> &'static str {
    match (ipa, nucleus.accent) {
        (_, Some('è')) => return "ɛ",
        (_, Some('ò')) => return "ɔ",
        (_, Some(_)) => return ipa,
        _ => {}
    }
    if !stressed {
        return ipa;
    }
    let open =
        nucleus.after_glide || from_end >= 3 || nucleus.falling || nucleus.ending == Ending::Er;
    match ipa {
        "e" if nucleus.ending == Ending::Mento => "e",
        "e" if open || nucleus.nasal || nucleus.ending == Ending::Eva => "ɛ",
        "o" if open && !nucleus.nasal => "ɔ",
        ipa => ipa,
    }
}

/// Short `gn` before the stress, tapped `r` before a last one, `ŋg`.
fn short_gn(sounds: &mut [Sound]) {
    let count = sounds.len();
    for i in 0..count {
        let before = i
            .checked_sub(1)
            .and_then(|k| sounds.get(k))
            .is_some_and(|s| s.vowel);
        let after = sounds
            .get(i + 1)
            .is_some_and(|s| s.vowel && s.stress == Stress::Primary);
        let next_g = sounds
            .get(i + 1)
            .is_some_and(|s| matches!(s.ipa, "ɡ" | "ɡː"));
        let last_stressed = i + 2 == count
            && sounds
                .get(i + 1)
                .is_some_and(|s| s.vowel && s.stress != Stress::None);
        if let Some(sound) = sounds.get_mut(i) {
            sound.ipa = match sound.ipa {
                "ɲɲ" if after => "ɲ",
                "n" if next_g => "ŋ",
                "r" if before && last_stressed => "ɾ",
                ipa => ipa,
            };
        }
    }
}

/// A last unstressed `i` is lax, and every unstressed `u`.
fn lax_end(sounds: &mut [Sound]) {
    let after_vowel = sounds.len() > 1
        && sounds
            .get(sounds.len() - 2)
            .is_some_and(|s| s.vowel || matches!(s.ipa, "i" | "u"));
    let alone = sounds.len() == 1;
    if let Some(last) = sounds.last_mut()
        && !alone
        && last.vowel
        && last.ipa == "i"
        && last.stress == Stress::None
        && !after_vowel
    {
        last.ipa = "ɪ";
    }
    let unstressed_u = |s: &&mut Sound| s.vowel && s.ipa == "u" && s.stress == Stress::None;
    for sound in sounds.iter_mut().filter(unstressed_u) {
        sound.ipa = "ʊ";
    }
}

/// A weak word before a pause takes full stress.
fn stress_before_pauses(spoken: &mut [Spoken]) {
    for i in 0..spoken.len() {
        let pause = !matches!(spoken.get(i + 1).map(|s| &s.part), Some(Part::Word(_)));
        let Some(Part::Word(sounds)) = spoken.get_mut(i).map(|s| &mut s.part) else {
            continue;
        };
        if !pause || sounds.iter().any(|s| s.stress == Stress::Primary) {
            continue;
        }
        let lighter = sounds.iter().position(|s| s.stress == Stress::Secondary);
        let at = lighter.or(sounds.iter().rposition(|s| s.vowel));
        let alone = sounds.len() == 1;
        if let Some(sound) = at.and_then(|at| sounds.get_mut(at)) {
            sound.stress = Stress::Primary;
            sound.ipa = match (sound.ipa, alone) {
                ("e", true) => "eː",
                ("o", true) => "oː",
                ("a", true) => "aː",
                ("ɪ", _) => "i",
                ("ʊ", _) => "u",
                (ipa, _) => ipa,
            };
        }
    }
}

const ONES: [&str; 20] = [
    "zero",
    "uno",
    "due",
    "tre",
    "quattro",
    "cinque",
    "sei",
    "sette",
    "otto",
    "nove",
    "dieci",
    "undici",
    "dodici",
    "tredici",
    "quattordici",
    "quindici",
    "sedici",
    "diciassette",
    "diciotto",
    "diciannove",
];

const TENS: [&str; 10] = [
    "",
    "",
    "venti",
    "trenta",
    "quaranta",
    "cinquanta",
    "sessanta",
    "settanta",
    "ottanta",
    "novanta",
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
        words.push("virgola".into());
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

/// `n` in words, a word for each group of three digits.
fn cardinal(n: u64) -> Vec<String> {
    if n == 0 {
        return vec!["zero".into()];
    }
    let mut words = Vec::new();
    let millions = n / 1_000_000;
    match millions {
        0 => {}
        1 => words.extend(["un".to_owned(), "milione".to_owned()]),
        m => {
            words.extend(thousands(m));
            words.push("milioni".into());
        }
    }
    words.extend(thousands(n % 1_000_000));
    words
}

fn thousands(n: u64) -> Vec<String> {
    let mut words = Vec::new();
    match n / 1000 {
        0 => {}
        1 => words.push("mille".into()),
        k => {
            words.extend(hundreds(k));
            words.push("mila".into());
        }
    }
    words.extend(hundreds(n % 1000));
    words
}

/// `n` below a thousand: the hundreds, then the rest, each a word.
fn hundreds(n: u64) -> Vec<String> {
    let mut words = Vec::new();
    match n / 100 {
        0 => {}
        1 => words.push("cento".into()),
        h => words.push(format!("{}cento", pick(&ONES, h))),
    }
    let rest = n % 100;
    let unit = rest % 10;
    if rest >= 20 {
        let tens = pick(&TENS, rest / 10);
        // `venti` and `uno` make `ventuno`; `tre` and `sei` end stressed.
        let joins = matches!(unit, 1 | 8);
        let mut word = String::from(if joins {
            tens.get(..tens.len() - 1).unwrap_or(tens)
        } else {
            tens
        });
        // The seam keeps `s` in `ventisei` from voicing.
        if unit > 0 {
            word.push('-');
        }
        match unit {
            0 => {}
            3 => word.push_str("tré"),
            6 => word.push_str("sèi"),
            u => word.push_str(pick(&ONES, u)),
        }
        words.push(word);
    } else if rest > 0 {
        words.push(pick(&ONES, rest).into());
    }
    words
}
