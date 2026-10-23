//! French, as `fr-fr` in espeak-ng.

use crate::letters::{self, letters};
use crate::text::{Piece, Token};
use crate::{Part, Sound, Spoken, Stress};

/// Never stressed: articles, clitics, most prepositions.
const UNSTRESSED: &[&str] = &[
    "ai", "as", "au", "aux", "ce", "celle", "celles", "ces", "cet", "cette", "ceux", "chaque",
    "chez", "comme", "contre", "dans", "de", "des", "du", "dès", "eh", "en", "est", "eurent",
    "eût", "furent", "ils", "la", "les", "leur", "leurs", "ma", "me", "mes", "mon", "ne", "ni",
    "nos", "notre", "on", "ont", "par", "pour", "sa", "sans", "se", "ses", "son", "sur", "ta",
    "te", "toute", "très", "vos", "votre", "y", "à", "car", "dont", "entre", "ou", "tes", "ton",
    "quant", "quand", "quel", "quelle", "quels", "quelles",
];

/// Unstressed, but stressed before a pause.
const AT_PAUSE: &[&str] = &[
    "a", "autre", "autres", "eux", "elle", "elles", "es", "et", "eu", "eut", "faut", "guère",
    "ici", "il", "je", "le", "lors", "mais", "non", "nous", "où", "pas", "peu", "plus", "près",
    "que", "qui", "sommes", "sous", "tant", "tel", "telle", "tous", "tout", "toutes", "trop", "tu",
    "un", "une", "uns", "vers", "vous", "ça", "êtes",
];

/// Lightly stressed, fully before a pause.
const LIGHT: &[&str] = &[
    "afin",
    "autour",
    "quoique",
    "plusieurs",
    "autant",
    "parmi",
    "presque",
    "puisque",
    "quelque",
    "quelques",
    "tandis",
    "ainsi",
    "alors",
    "après",
    "assez",
    "aussi",
    "avant",
    "avec",
    "beaucoup",
    "cela",
    "comment",
    "depuis",
    "encore",
    "enfin",
    "jamais",
    "loin",
    "lorsque",
    "lui",
    "mieux",
    "moi",
    "mois",
    "oui",
    "point",
    "pourquoi",
    "pourtant",
    "quelqu'un",
    "quoi",
    "rien",
    "soit",
    "suis",
    "toi",
    "toujours",
    "voilà",
    "voici",
];

/// Words said apart from the rules, sounds split by spaces.
const SAID: &[(&str, &str)] = &[
    ("est", "ɛ"),
    ("es", "ɛ"),
    ("et", "e"),
    ("ai", "e"),
    ("les", "l e"),
    ("des", "d e"),
    ("mes", "m e"),
    ("tes", "t e"),
    ("ses", "s e"),
    ("ces", "s e"),
    ("eu", "y"),
    ("eus", "y"),
    ("eut", "y"),
    ("eût", "y"),
    ("eurent", "y ʁ"),
    ("eussent", "y s"),
    ("monsieur", "m ə s j ø"),
    ("messieurs", "m e s j ø"),
    ("femme", "f a m"),
    ("femmes", "f a m"),
    ("hier", "j ɛ ʁ"),
    ("fils", "f i s"),
    ("six", "s i"),
    ("dix", "d i"),
    ("sept", "s ɛ t"),
    ("huit", "y i t"),
    ("oeil", "œ j"),
    ("œil", "œ j"),
    ("clef", "k l e"),
    ("gentil", "ʒ ɑ̃ t i"),
    ("second", "s ə ɡ ɔ̃"),
    ("seconde", "s ə ɡ ɔ̃ d"),
    ("pays", "p ɛ i"),
    ("paysan", "p ɛ i z ɑ̃"),
    ("paysans", "p ɛ i z ɑ̃"),
    ("août", "u"),
    ("oignon", "ɔ ɲ ɔ̃"),
    ("aujourd'hui", "o ʒ u ʁ d y i"),
    ("quelqu'un", "k ɛ l k œ̃"),
    ("ennemi", "ɛ n m i"),
    ("ennemis", "ɛ n m i"),
    ("tous", "t u"),
    ("plus", "p l y"),
    ("jeune", "ʒ ø n"),
    ("médecin", "m e d ə s ɛ̃"),
    ("votre", "v o t ʁ"),
    ("grosse", "ɡ ʁ o s"),
    ("grosses", "ɡ ʁ o s"),
    ("notre", "n o t ʁ"),
    ("soixante", "s w a s ɑ̃ t"),
    ("dessus", "d ə s y"),
    ("dessous", "d ə s u"),
    ("jeunes", "ʒ ø n"),
];

/// Spellings read by the rules as another, where only the vowel differs.
const RESPELT: &[(&str, &str)] = &[
    ("faisait", "fesait"),
    ("faisais", "fesais"),
    ("faisaient", "fesaient"),
    ("faisant", "fesant"),
    ("faisons", "fesons"),
];

/// Short forms read as their words before a full stop.
const SHORT: &[(&str, &str)] = &[
    ("m", "monsieur"),
    ("mm", "messieurs"),
    ("mme", "madame"),
    ("mlle", "mademoiselle"),
    ("dr", "docteur"),
    ("st", "saint"),
    ("ste", "sainte"),
];

/// Letter names, for words with no vowel.
const NAMES: &[(char, &str)] = &[
    ('b', "b e"),
    ('c', "s e"),
    ('d', "d e"),
    ('f', "ɛ f"),
    ('g', "ʒ e"),
    ('h', "a ʃ"),
    ('j', "ʒ i"),
    ('k', "k a"),
    ('l', "ɛ l"),
    ('m', "ɛ m"),
    ('n', "ɛ n"),
    ('p', "p e"),
    ('q', "k y"),
    ('r', "ɛ ʁ"),
    ('s', "ɛ s"),
    ('t', "t e"),
    ('v', "v e"),
    ('w', "d u b l ə v e"),
    ('x', "i k s"),
    ('z', "z ɛ d"),
];

/// Words ending in a nasal that link with `n`.
const NASAL_LINK: &[&str] = &[
    "un", "aucun", "chacun", "on", "en", "mon", "ton", "son", "bien", "rien", "bon",
];

/// Words that start with an h that blocks linking.
const ASPIRATE: &[&str] = &[
    "hach",
    "haie",
    "hain",
    "haïr",
    "hais",
    "hait",
    "halt",
    "hamac",
    "hameau",
    "hanche",
    "hangar",
    "hant",
    "happ",
    "harang",
    "harc",
    "hard",
    "hareng",
    "hargn",
    "haricot",
    "harn",
    "harp",
    "hasard",
    "hât",
    "haus",
    "haut",
    "havre",
    "henn",
    "hériss",
    "hernie",
    "héron",
    "héros",
    "hêtre",
    "heurt",
    "hibou",
    "hideu",
    "hiérarch",
    "hiss",
    "hoch",
    "holl",
    "homard",
    "hongr",
    "honte",
    "honteu",
    "hoquet",
    "horde",
    "hors",
    "hotte",
    "houill",
    "houl",
    "houp",
    "housse",
    "houx",
    "hublot",
    "hue",
    "huit",
    "hurl",
    "hutte",
    "hussard",
];

/// `-ent` verb forms said as nouns would be, and nouns said as verbs.
const ENT_NOUNS: &[&str] = &[
    "accent",
    "absent",
    "présent",
    "argent",
    "agent",
    "urgent",
    "content",
    "mécontent",
    "talent",
    "violent",
    "souvent",
    "parent",
    "serpent",
    "torrent",
    "comment",
    "ardent",
    "accident",
    "évident",
    "prudent",
    "imprudent",
    "président",
    "excellent",
    "insolent",
    "innocent",
    "décent",
    "récent",
    "différent",
    "indifférent",
    "permanent",
    "imminent",
    "continent",
    "adolescent",
    "intelligent",
    "négligent",
    "client",
    "patient",
    "impatient",
    "orient",
    "couvent",
    "équivalent",
    "fréquent",
    "conséquent",
    "éloquent",
    "sergent",
    "pendent",
    "transparent",
    "apparent",
    "vincent",
    "laurent",
    "clément",
    "florent",
    "auvent",
    "paravent",
    "impudent",
    "strident",
    "pénitent",
    "impertinent",
    "compétent",
    "succulent",
    "opulent",
    "turbulent",
    "indolent",
    "éminent",
    "inconvénient",
    "régent",
];

/// `-ment` words that are verbs, their ending silent.
const MENT_VERBS: &[&str] = &[
    "aiment",
    "dorment",
    "endorment",
    "ferment",
    "enferment",
    "renferment",
    "referment",
    "fument",
    "forment",
    "transforment",
    "nomment",
    "expriment",
    "estiment",
    "allument",
    "calment",
    "animent",
    "affirment",
    "priment",
    "sèment",
    "arment",
    "charment",
    "alarment",
    "consument",
    "informent",
    "abîment",
    "riment",
    "suppriment",
    "impriment",
];

/// Final consonants said that the rules would leave silent.
const SAID_FINAL: &[&str] = &[
    "hélas", "jadis", "sens", "ours", "mars", "os", "maïs", "atlas", "vis", "lys", "iris", "oasis",
    "tennis", "bus", "virus", "cactus", "omnibus", "autobus", "gratis", "albatros", "ibis",
    "cassis", "net", "dot", "ouest", "brut", "rapt", "christ", "déficit", "contact", "direct",
    "correct", "intact", "tact", "exact", "strict", "compact", "verdict", "district", "index",
    "lynx", "sphinx", "larynx", "thorax", "phénix", "silex", "codex", "félix", "max", "but", "cap",
    "cep", "hop", "stop", "slip", "zigzag", "gag", "grog", "joug", "sud", "david", "madrid",
    "est-ce", "mœurs",
];

/// Final consonants the rules would say that are silent.
const SILENT_FINAL: &[&str] = &[
    "blanc",
    "franc",
    "flanc",
    "banc",
    "tronc",
    "jonc",
    "tabac",
    "estomac",
    "porc",
    "caoutchouc",
    "croc",
    "accroc",
    "escroc",
    "clerc",
    "fusil",
    "outil",
    "sourcil",
    "persil",
    "nombril",
    "respect",
    "aspect",
    "suspect",
    "instinct",
    "distinct",
];

/// `-er` said `ɛʁ` though longer than one syllable.
const OPEN_ER: &[&str] = &[
    "hiver", "enfer", "amer", "cancer", "éther", "super", "laser", "envers", "univers", "divers",
    "travers", "pervers", "revers", "fier", "cuiller", "mer", "fer", "cher",
];

/// `ill` said `il`.
const ILL_L: &[&str] = &["vill", "mill", "tranquill", "distill", "oscill"];

/// `text` read aloud, as espeak-ng writes it.
pub(crate) fn read(tokens: &[Token<'_>]) -> Vec<Spoken> {
    let mut words: Vec<Said> = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let space = token.space && !token.hyphen;
        match token.piece {
            Piece::Mark(mark) => words.push(Said::mark(Part::Mark(mark), space)),
            Piece::Break => words.push(Said::mark(Part::Break, space)),
            Piece::Number(text) => number(text, space, &mut words),
            Piece::Word(text) => {
                let dot = tokens.get(at + 1).map(|t| t.piece) == Some(Piece::Mark('.'));
                let pause = tokens
                    .get(at + 1)
                    .is_none_or(|t| matches!(t.piece, Piece::Mark(_) | Piece::Break));
                let w = letters(text);
                let name = text.starts_with(char::is_uppercase) && at > 0;
                let lone_t = token.hyphen && w == ['t'];
                if token.hyphen && w == ['c', 'e'] {
                    words.push(Said::sounds(vec![Sound::consonant("s")], false));
                    continue;
                }
                if lone_t {
                    words.push(Said::sounds(vec![Sound::consonant("t")], false));
                    continue;
                }
                let said = spelled(&w, dot);
                let count = said.len();
                for (i, word) in said.into_iter().enumerate() {
                    let last = pause && i + 1 == count;
                    let mut said = say(&word, last, name);
                    said.space = if i == 0 { space } else { true };
                    words.push(said);
                }
            }
        }
    }
    stress_after_de(&mut words);
    link(&mut words);
    words.into_iter().map(|w| w.spoken).collect()
}

/// A part as said, and what it lends the next word.
struct Said {
    spoken: Spoken,
    space: bool,
    /// The consonant said before a vowel, as `z` in `les amis`.
    latent: Option<&'static str>,
    /// Starts with a vowel a consonant may link to.
    open: bool,
    /// The consonant lent across a hyphen, as `t` in `dit-il`.
    joined: Option<&'static str>,
    /// The word as written, lowercased.
    text: String,
}

impl Said {
    fn mark(part: Part, space: bool) -> Self {
        Self {
            spoken: Spoken { part, space },
            space,
            latent: None,
            open: false,
            joined: None,
            text: String::new(),
        }
    }

    fn sounds(sounds: Vec<Sound>, space: bool) -> Self {
        Self::mark(Part::Word(sounds), space)
    }
}

/// `w` as the words said: itself, or its short form, or its letters.
fn spelled(w: &[char], dot: bool) -> Vec<Vec<char>> {
    let text: String = w.iter().collect();
    let always = matches!(text.as_str(), "mme" | "mlle" | "mm");
    if (dot || always)
        && let Some((_, long)) = SHORT.iter().find(|(s, _)| *s == text)
    {
        return vec![long.chars().collect()];
    }
    if w.iter().any(|&c| is_vowel(c)) || w.len() > 3 {
        return vec![w.to_vec()];
    }
    w.iter().map(|&c| vec![c]).collect()
}

fn is_vowel(c: char) -> bool {
    "aeiouyéèêëàâîïôûùüœæ".contains(c)
}

/// One word said, stressed by its class; `last` before a pause.
fn say(w: &[char], last: bool, name: bool) -> Said {
    let text: String = w.iter().collect();
    let base = text.rsplit('\'').next().unwrap_or(&text);
    let (prefix, body) = match text.rfind('\'') {
        Some(at) if !SAID.iter().any(|(s, _)| *s == text) => (
            text.get(..at).unwrap_or_default(),
            text.get(at + 1..).unwrap_or_default(),
        ),
        _ => ("", text.as_str()),
    };
    let mut sounds = elided(prefix);
    let body_w: Vec<char> = body.chars().collect();
    sounds.extend(word_sounds(&body_w));
    let class = if LIGHT.contains(&text.as_str()) || LIGHT.contains(&base) {
        Stress::Secondary
    } else if UNSTRESSED.contains(&base) || UNSTRESSED.contains(&text.as_str()) {
        Stress::None
    } else if AT_PAUSE.contains(&base) {
        if last { Stress::Primary } else { Stress::None }
    } else {
        Stress::Primary
    };
    let class = if last && class == Stress::Secondary {
        Stress::Primary
    } else {
        class
    };
    let full = sounds.iter().rposition(|s| s.vowel && s.ipa != "ə");
    let at = full.or_else(|| sounds.iter().rposition(|s| s.vowel));
    if let Some(sound) = at.and_then(|k| sounds.get_mut(k)) {
        sound.stress = class;
    }
    for sound in &mut sounds {
        if sound.stress != Stress::Primary {
            sound.ipa = match sound.ipa {
                "aː" => "a",
                "oː" => "o",
                "yː" => "y",
                other => other,
            };
        }
    }
    let open = prefix.is_empty() && starts_open(&body_w);
    Said {
        spoken: Spoken {
            part: Part::Word(sounds),
            space: true,
        },
        space: true,
        latent: latent(&body_w, name),
        open,
        joined: match body_w.last() {
            Some('s' | 'x' | 'z') => Some("z"),
            Some('t' | 'd') => Some("t"),
            _ => latent(&body_w, false),
        },
        text,
    }
}

/// The sounds of an elided word before an apostrophe, as `l'` or `qu'`.
fn elided(prefix: &str) -> Vec<Sound> {
    let said = match prefix {
        "" => return Vec::new(),
        "l" => "l",
        "d" => "d",
        "j" => "ʒ",
        "qu" => "k",
        "c" => "s",
        "m" => "m",
        "n" => "n",
        "s" => "s",
        "t" => "t",
        "jusqu" => "ʒ y s k",
        "lorsqu" => "l o ʁ s k",
        "puisqu" => "p y i s k",
        "quoiqu" => "k w a k",
        "presqu" => "p ʁ ɛ s k",
        "quelqu" => "k ɛ l k",
        other => return word_sounds(&other.chars().collect::<Vec<_>>()),
    };
    parse(said)
}

/// Whether `w` starts with a vowel sound a consonant may link to.
fn starts_open(w: &[char]) -> bool {
    let text: String = w.iter().collect();
    match w.first() {
        Some('h') => !ASPIRATE.iter().any(|a| text.starts_with(a)),
        Some(&c) => is_vowel(c) && text != "oui" && text != "onze",
        None => false,
    }
}

/// Words whose silent final consonant stays silent before a vowel.
const NO_LINK: &[&str] = &[
    "et", "temps", "corps", "alors", "voix", "mieux", "mis", "pris", "assis", "surpris", "appris",
    "compris", "permis", "promis", "remis", "admis", "soumis", "dit", "nuit", "fut", "peut",
    "debout", "lit", "fort", "comment", "vers", "mort", "instant", "moment", "devant", "point",
];

/// The consonant `w` says before a vowel, where its spelling lends one.
fn latent(w: &[char], name: bool) -> Option<&'static str> {
    let text: String = w.iter().collect();
    let text = text.as_str();
    if NASAL_LINK.contains(&text) {
        return Some("n");
    }
    if text == "tous" {
        return Some("s");
    }
    if matches!(text, "premier" | "dernier" | "léger") {
        return Some("ʁ");
    }
    let small = [UNSTRESSED, AT_PAUSE, LIGHT]
        .iter()
        .any(|l| l.contains(&text));
    if NO_LINK.contains(&text) || SAID_FINAL.contains(&text) || name && !small {
        return None;
    }
    let verb =
        text.ends_with("ait") || text.ends_with("ont") || text.ends_with("ent") && verb_ent(text);
    match w.last()? {
        's' | 'x' | 'z' => Some("z"),
        't' | 'd'
            if small
                || verb
                || matches!(text, "quand" | "grand" | "eut" | "mit" | "vit" | "fit") =>
        {
            Some("t")
        }
        _ => None,
    }
}

/// `said` split at spaces into sounds; `ˈ` before a vowel stresses it.
fn parse(said: &'static str) -> Vec<Sound> {
    said.split(' ')
        .map(|s| {
            let (stress, s) = match s.strip_prefix('ˈ') {
                Some(rest) => (Stress::Primary, rest),
                None => (Stress::None, s),
            };
            let vowel = s.chars().next().is_some_and(|c| "aeiouyøœɛɔəɑ".contains(c));
            Sound {
                ipa: s,
                vowel,
                stress,
            }
        })
        .collect()
}

/// The sounds of one word, unstressed.
fn word_sounds(w: &[char]) -> Vec<Sound> {
    let text: String = w.iter().collect();
    if let Some((_, said)) = SAID.iter().find(|(s, _)| *s == text) {
        return parse(said);
    }
    if let [c] = w
        && let Some((_, said)) = NAMES.iter().find(|(n, _)| n == c)
    {
        return parse(said);
    }
    let respelt;
    let w = match RESPELT.iter().find(|(s, _)| *s == text) {
        Some((_, r)) => {
            respelt = r.chars().collect::<Vec<_>>();
            respelt.as_slice()
        }
        None => w,
    };
    let segs = segments(w);
    drop_schwas(segs)
}

/// A sound, and whether it is a schwa that may drop.
#[derive(Clone, Copy)]
struct Seg {
    sound: Sound,
    schwa: bool,
}

/// Inner schwas after one consonant that follows a vowel drop.
fn drop_schwas(mut segs: Vec<Seg>) -> Vec<Sound> {
    let mut k = 0;
    while k < segs.len() {
        let drop = segs.get(k).is_some_and(|s| s.schwa)
            && k >= 2
            && segs.get(k - 1).is_some_and(|s| !s.sound.vowel)
            && segs.get(k - 2).is_some_and(|s| s.sound.vowel)
            && segs.get(k + 1).is_some_and(|s| !s.sound.vowel)
            && segs
                .get(k + 2)
                .is_some_and(|s| s.sound.vowel || matches!(s.sound.ipa, "j" | "w" | "y"));
        if drop {
            segs.remove(k);
        } else {
            k += 1;
        }
    }
    segs.into_iter().map(|s| s.sound).collect()
}

/// Where the said letters end, and the vowel of a final `e` they leave.
fn ending(w: &[char]) -> usize {
    let text: String = w.iter().collect();
    let n = w.len();
    let groups = |k: usize| vowel_groups(w.get(..k).unwrap_or_default());
    if text.ends_with("ent") && n > 4 && groups(n - 3) >= 1 && verb_ent(&text) {
        return n - 3;
    }
    if text.ends_with("es") && groups(n - 2) >= 1 && !SAID_FINAL.contains(&text.as_str()) {
        return n - 2;
    }
    if text.ends_with("sque") || text.ends_with("lque") {
        return n;
    }
    if text.ends_with("sques") || text.ends_with("lques") {
        return n - 1;
    }
    if text.ends_with('e') && groups(n - 1) >= 1 {
        return n - 1;
    }
    if SAID_FINAL.contains(&text.as_str()) {
        return n;
    }
    let mut end = n;
    while end > 0 && silent_final(w, end - 1, &text) {
        end -= 1;
    }
    end
}

/// Vowel letter runs in `w`, as syllables are counted.
fn vowel_groups(w: &[char]) -> usize {
    let mut groups = 0;
    let mut in_vowel = false;
    let mut prev = ' ';
    for (k, &c) in w.iter().enumerate() {
        let glide_y = c == 'y' && w.get(k + 1).is_some_and(|&n| is_vowel(n)) && k > 0;
        let v = is_vowel(c) && !(c == 'u' && matches!(prev, 'q' | 'g')) && !glide_y;
        prev = c;
        if v && !in_vowel {
            groups += 1;
        }
        in_vowel = v;
    }
    groups
}

/// Whether the `-ent` ending of `text` is a silent verb ending.
fn verb_ent(text: &str) -> bool {
    if ENT_NOUNS.contains(&text) {
        return false;
    }
    if text.ends_with("ment") {
        return MENT_VERBS.contains(&text);
    }
    if text.ends_with("ient") && !text.ends_with("aient") {
        let before = text.chars().rev().nth(4);
        return !matches!(before, Some('v' | 't'));
    }
    true
}

/// Whether the letter at `k`, the last still said, is silent.
fn silent_final(w: &[char], k: usize, text: &str) -> bool {
    let Some(&c) = w.get(k) else {
        return false;
    };
    let one = vowel_groups(w) <= 1;
    match c {
        'z' => k > 0 && w.get(k - 1).is_some_and(|&c| is_vowel(c)),
        'g' => k > 0 && w.get(k - 1) == Some(&'n') || k + 1 < w.len(),
        's' | 'x' | 't' | 'd' | 'p' => true,
        'b' => text == "plomb",
        'r' => {
            k > 0
                && (k + 1 == w.len() || k + 2 == w.len() && w.last() == Some(&'s'))
                && w.get(k - 1) == Some(&'e')
                && !one
                && !OPEN_ER.contains(&text)
        }
        'c' | 'l' => {
            let single = text.strip_suffix('s').unwrap_or(text);
            SILENT_FINAL.contains(&single) && k + 2 >= w.len()
        }
        'f' => text == "clef" || text == "nerf",
        _ => false,
    }
}

/// Whether letter `k` of `w` closes a nasal vowel.
fn nasal(w: &[char], k: usize) -> bool {
    let Some(&c) = w.get(k) else {
        return false;
    };
    if !matches!(c, 'n' | 'm') {
        return false;
    }
    match w.get(k + 1) {
        None => true,
        Some(&'h') => !w.get(k + 2).is_some_and(|&c| is_vowel(c)),
        Some(&next) => !is_vowel(next) && !matches!(next, 'n' | 'm'),
    }
}

/// The sounds of `w` by letter rules.
fn segments(w: &[char]) -> Vec<Seg> {
    let end = ending(w);
    let mut out: Vec<Seg> = Vec::new();
    let mut i = 0;
    while i < end {
        let Some(&c) = w.get(i) else {
            break;
        };
        let step = if is_vowel(c) {
            vowel(w, i, end, &mut out)
        } else {
            consonant(w, i, end, &mut out)
        };
        i += step.max(1);
    }
    out
}

fn push(out: &mut Vec<Seg>, said: &'static str) {
    for sound in parse(said) {
        out.push(Seg {
            sound,
            schwa: false,
        });
    }
}

fn push_glide(out: &mut Vec<Seg>, said: &'static str) {
    out.push(Seg {
        sound: Sound::consonant(said),
        schwa: false,
    });
}

fn starts(w: &[char], i: usize, s: &str) -> bool {
    s.chars().enumerate().all(|(k, c)| w.get(i + k) == Some(&c))
}

/// Whether a said vowel follows letter `k`, before `end`.
fn vowel_next(w: &[char], k: usize, end: usize) -> bool {
    k + 1 < end && w.get(k + 1).is_some_and(|&c| is_vowel(c))
}

/// One vowel or vowel spelling at `i`; returns the letters used.
fn vowel(w: &[char], i: usize, end: usize, out: &mut Vec<Seg>) -> usize {
    let Some(&c) = w.get(i) else {
        return 1;
    };
    let prev = i.checked_sub(1).and_then(|p| w.get(p)).copied();
    match c {
        'a' | 'à' => a_vowel(w, i, end, out),
        'e' => e_vowel(w, i, end, out, prev),
        'é' => {
            push(out, "e");
            1
        }
        'è' | 'ê' | 'ë' => {
            push(out, "ɛ");
            1
        }
        'i' | 'î' | 'ï' | 'y' => i_vowel(w, i, end, out, prev),
        'o' | 'ô' => o_vowel(w, i, end, out),
        'â' | 'û' => {
            push(out, if c == 'â' { "aː" } else { "yː" });
            1
        }
        'œ' => {
            if w.get(i + 1) == Some(&'u') {
                eu(w, i + 2, end, out);
                2
            } else {
                push(out, "e");
                1
            }
        }
        'u' | 'ù' | 'ü' => {
            if c == 'u' && nasal(w, i + 1) && i + 1 < end.max(i + 2) {
                let latin = w.get(i + 1) == Some(&'m') && i + 2 == w.len() && w.len() > 4;
                if latin && !starts(w, 0, "parfum") {
                    push(out, "ɔ m");
                } else {
                    push(out, "œ̃");
                }
                return 2;
            }
            if c == 'ù' {
                push(out, "u");
            } else if vowel_next(w, i, end) {
                push_glide(out, "y");
            } else {
                push(out, "y");
            }
            1
        }
        _ => {
            push(out, "a");
            1
        }
    }
}

fn a_vowel(w: &[char], i: usize, end: usize, out: &mut Vec<Seg>) -> usize {
    let next = w.get(i + 1).copied();
    if w.get(i) != Some(&'a') {
        push(out, "a");
        return 1;
    }
    match next {
        Some('i' | 'î') if nasal(w, i + 2) => {
            push(out, "ɛ̃");
            3
        }
        Some('i') if starts(w, i + 2, "ll") => {
            push(out, "a");
            push_glide(out, "j");
            4
        }
        Some('i') if w.get(i + 2) == Some(&'l') && i + 3 == w.len() => {
            push(out, "a");
            push_glide(out, "j");
            3
        }
        Some('i' | 'î') => {
            let final_ai = i + 2 == w.len() && vowel_groups(w) > 1;
            push(out, if final_ai { "e" } else { "ɛ" });
            2
        }
        Some('y') if vowel_next(w, i + 1, w.len()) => {
            push(out, "ɛ");
            push_glide(out, "j");
            2
        }
        Some('y') => {
            push(out, "ɛ");
            2
        }
        Some('u') => {
            push(out, "o");
            2
        }
        Some('n' | 'm') if nasal(w, i + 1) && i + 1 < end.max(i + 2) => {
            push(out, "ɑ̃");
            2
        }
        _ => {
            push(out, "a");
            1
        }
    }
}

fn e_vowel(w: &[char], i: usize, end: usize, out: &mut Vec<Seg>, prev: Option<char>) -> usize {
    let next = w.get(i + 1).copied();
    if starts(w, i, "eau") {
        push(out, "o");
        return 3;
    }
    if next == Some('u') {
        eu(w, i + 2, end, out);
        return 2;
    }
    if matches!(prev, Some('j' | 'g')) && next.is_some_and(|c| "aoôâ".contains(c)) {
        return 1;
    }
    if matches!(next, Some('i' | 'î')) {
        if nasal(w, i + 2) {
            push(out, "ɛ̃");
            return 3;
        }
        if starts(w, i + 2, "ll") || w.get(i + 2) == Some(&'l') && i + 3 == w.len() {
            push(out, "ɛ");
            push_glide(out, "j");
            return if starts(w, i + 2, "ll") { 4 } else { 3 };
        }
        push(out, "ɛ");
        return 2;
    }
    if next == Some('y') {
        push(out, "ɛ");
        if vowel_next(w, i + 1, w.len()) {
            push_glide(out, "j");
        }
        return 2;
    }
    if i == 0 && (starts(w, 0, "enn") || starts(w, 0, "emm")) {
        push(out, "ɑ̃");
        return 2;
    }
    if starts(w, i, "emment") {
        push(out, "a");
        return 1;
    }
    if matches!(next, Some('n' | 'm')) && nasal(w, i + 1) && i + 1 < end.max(i + 2) {
        let ience = matches!(w.get(i + 2), Some('c'));
        let ien = matches!(prev, Some('i' | 'y' | 'é')) && !ience;
        let latin = i + 2 == w.len() && w.len() > 4 && !ien;
        push(out, if ien || latin { "ɛ̃" } else { "ɑ̃" });
        return 2;
    }
    if i + 1 >= end {
        let one = vowel_groups(w) <= 1;
        let said = match w.get(end) {
            Some('t') => "ɛ",
            Some('r' | 'z' | 'd') => "e",
            Some('s') if one => "e",
            _ => "ə",
        };
        if said == "ə" {
            out.push(Seg {
                sound: Sound::vowel("ə"),
                schwa: false,
            });
        } else {
            push(out, said);
        }
        return 1;
    }
    let c1 = next.unwrap_or(' ');
    let c2 = w.get(i + 2).copied();
    if c1 == 'x' {
        push(out, "ɛ");
        return 1;
    }
    let qu = prev == Some('u') && i >= 2 && matches!(w.get(i - 2), Some('q' | 'g'));
    let after_vowel = prev.is_some_and(is_vowel) && prev != Some('e') && !qu;
    if after_vowel && !is_vowel(c1) && c2.is_some_and(is_vowel) {
        return 1;
    }
    let final_c = i + 2 >= end;
    let initial_double =
        i == 0 && Some(c1) == c2 && vowel_groups(w.get(..end).unwrap_or_default()) >= 2;
    if initial_double && !matches!(c1, 'n' | 'm') {
        push(out, "e");
        return 1;
    }
    let closed = if final_c {
        true
    } else if Some(c1) == c2 {
        !(i == 1 && matches!(w.first(), Some('d' | 'r')) && c1 == 's')
    } else {
        let digraph = matches!((c1, c2), ('c' | 'p' | 't', Some('h')) | ('g', Some('n')));
        let cluster = "pbtdkcgfv".contains(c1) && matches!(c2, Some('r' | 'l'));
        !digraph && !cluster && c2.is_some_and(|c| !is_vowel(c))
    };
    if closed {
        push(out, "ɛ");
    } else {
        out.push(Seg {
            sound: Sound::vowel("ə"),
            schwa: true,
        });
    }
    1
}

/// `eu` before letter `k`.
fn eu(w: &[char], k: usize, end: usize, out: &mut Vec<Seg>) {
    let next = w.get(k).copied();
    let open = k >= end
        || next == Some('s') && w.get(k + 1).is_some_and(|&c| is_vowel(c))
        || matches!(next, Some('t' | 'n' | 'z' | 'x'));
    let later = vowel_groups(w.get(k + 1..end).unwrap_or_default()) > 0;
    let said = if open || later && next != Some('i') {
        "ø"
    } else if matches!(next, Some('r' | 'l' | 'f' | 'i')) {
        "œ"
    } else {
        "ø"
    };
    push(out, said);
}

fn i_vowel(w: &[char], i: usize, end: usize, out: &mut Vec<Seg>, prev: Option<char>) -> usize {
    let Some(&c) = w.get(i) else {
        return 1;
    };
    let text: String = w.iter().collect();
    if matches!(c, 'i' | 'y') && nasal(w, i + 1) && i + 1 < end.max(i + 2) {
        push(out, "ɛ̃");
        return 2;
    }
    if c == 'i' && starts(w, i + 1, "ll") {
        let after_vowel = prev.is_some_and(is_vowel);
        if after_vowel {
            push_glide(out, "j");
            return 3;
        }
        let plain = ILL_L
            .iter()
            .any(|s| text.starts_with(s) || s.len() > 4 && text.contains(s));
        if plain || i == 0 || text.ends_with("ville") || text.ends_with("villes") {
            push(out, "i");
            return 1;
        }
        push(out, "i");
        push_glide(out, "j");
        return 3;
    }
    if c == 'i' && prev.is_some_and(is_vowel) && w.get(i + 1) == Some(&'l') && i + 2 == w.len() {
        push_glide(out, "j");
        return 2;
    }
    let glide = matches!(c, 'i' | 'y') && vowel_next(w, i, end);
    let cluster = i >= 2
        && w.get(i - 1).is_some_and(|&c| matches!(c, 'r' | 'l'))
        && w.get(i - 2).is_some_and(|&c| "pbtdkcgfv".contains(c));
    if glide && !cluster {
        push_glide(out, "j");
    } else {
        push(out, "i");
    }
    1
}

fn o_vowel(w: &[char], i: usize, end: usize, out: &mut Vec<Seg>) -> usize {
    let next = w.get(i + 1).copied();
    if w.get(i) == Some(&'ô') {
        push(out, "oː");
        return 1;
    }
    match next {
        Some('i' | 'î') if nasal(w, i + 2) => {
            push(out, "w ɛ̃");
            3
        }
        Some('i' | 'î') => {
            push_glide(out, "w");
            push(out, "a");
            2
        }
        Some('y') => {
            push_glide(out, "w");
            push(out, "a");
            if vowel_next(w, i + 1, w.len()) {
                push_glide(out, "j");
            }
            2
        }
        Some('u' | 'ù' | 'û') => {
            let glide = vowel_next(w, i + 1, end) && !starts(w, i + 2, "ill");
            if glide {
                push_glide(out, "w");
            } else {
                push(out, "u");
            }
            if starts(w, i + 2, "ill") || w.get(i + 2) == Some(&'i') && w.get(i + 3) == Some(&'l') {
                push_glide(out, "j");
                return if starts(w, i + 2, "ill") { 5 } else { 4 };
            }
            2
        }
        Some('e') if w.get(i + 2) == Some(&'u') => {
            eu(w, i + 3, end, out);
            3
        }
        Some('n' | 'm') if nasal(w, i + 1) && i + 1 < end.max(i + 2) => {
            push(out, "ɔ̃");
            2
        }
        _ => {
            let plural = end + 2 == w.len() && w.ends_with(&['e', 's']);
            let said = if i + 1 >= end || next == Some('r') && w.get(i + 2) == Some(&'r') {
                "o"
            } else if i + 2 >= end {
                if plural { "o" } else { "ɔ" }
            } else {
                let c2 = w.get(i + 2).copied();
                let c1 = next.unwrap_or(' ');
                let digraph = matches!((c1, c2), ('c' | 'p' | 't', Some('h')) | ('g', Some('n')))
                    && i + 3 < end;
                let closed = !digraph && c2.is_some_and(|c| !is_vowel(c));
                if closed || Some(c1) == c2 { "ɔ" } else { "o" }
            };
            push(out, said);
            1
        }
    }
}

/// One consonant or consonant spelling at `i`; returns the letters used.
fn consonant(w: &[char], i: usize, end: usize, out: &mut Vec<Seg>) -> usize {
    let Some(&c) = w.get(i) else {
        return 1;
    };
    let next = w.get(i + 1).copied();
    let front = |k: usize| w.get(k).is_some_and(|c| "eiyéèêëîï".contains(*c));
    let prev = i.checked_sub(1).and_then(|p| w.get(p)).copied();
    let double = next == Some(c) && i + 1 < end;
    match c {
        'c' if next == Some('h') => {
            let hard = w.get(i + 2).is_some_and(|&c| matches!(c, 'r' | 'l'));
            push(out, if hard { "k" } else { "ʃ" });
            2
        }
        'c' if next == Some('c') && front(i + 2) => {
            push(out, "k s");
            2
        }
        'c' if front(i + 1) => {
            push(out, "s");
            1
        }
        'c' if next == Some('k') => 1,
        'c' | 'k' | 'q' => {
            push(out, "k");
            if c == 'q' && next == Some('u') {
                return 2;
            }
            if double { 2 } else { 1 }
        }
        'ç' => {
            push(out, "s");
            1
        }
        'g' if next == Some('n') => {
            let after = w.get(i + 2).copied();
            let mute = i + 2 >= end;
            let last_i =
                after == Some('i') && vowel_groups(w.get(i + 3..end).unwrap_or_default()) == 0;
            if mute || after == Some('i') && !last_i {
                push(out, "ɲ");
            } else if last_i {
                push(out, "n");
            } else {
                push(out, "n");
                push_glide(out, "j");
            }
            2
        }
        'g' if next == Some('u') && w.get(i + 2).is_some_and(|&c| is_vowel(c)) => {
            push(out, "ɡ");
            2
        }
        'g' if next == Some('e') && w.get(i + 2).is_some_and(|c| "aoâ".contains(*c)) => {
            push(out, "ʒ");
            2
        }
        'g' if front(i + 1) => {
            push(out, "ʒ");
            1
        }
        'g' if prev == Some('n')
            && next.is_some_and(|c| !is_vowel(c) && !matches!(c, 'r' | 'l')) =>
        {
            1
        }
        'g' => {
            push(out, "ɡ");
            if double { 2 } else { 1 }
        }
        'h' => 1,
        'j' => {
            push(out, "ʒ");
            1
        }
        'p' if next == Some('h') => {
            push(out, "f");
            2
        }
        'p' if next == Some('t') && prev == Some('m') => 1,
        's' if next == Some('c') && front(i + 2) => {
            push(out, "s");
            2
        }
        's' if starts(w, i, "sch") => {
            push(out, "ʃ");
            3
        }
        's' if double => {
            push(out, "s");
            2
        }
        's' => {
            let between = prev.is_some_and(is_vowel) && next.is_some_and(is_vowel);
            push(out, if between { "z" } else { "s" });
            1
        }
        't' if next == Some('h') => {
            push(out, "t");
            2
        }
        't' if ["ion", "iel", "ial", "ienc", "ieu"]
            .iter()
            .any(|s| starts(w, i + 1, s)) =>
        {
            let soft = !matches!(prev, Some('s' | 'x'));
            push(out, if soft { "s" } else { "t" });
            1
        }
        'x' => {
            let ex = prev.is_some_and(|p| p == 'e') && i <= 2;
            let before_vowel = next.is_some_and(|c| is_vowel(c) || c == 'h');
            if ex && before_vowel {
                push(out, "ɡ z");
            } else if next == Some('c') && front(i + 2) {
                push(out, "k s");
                return 2;
            } else {
                push(out, "k s");
            }
            1
        }
        'r' => {
            push(out, "ʁ");
            if double { 2 } else { 1 }
        }
        'w' => {
            push_glide(out, "w");
            1
        }
        _ => {
            let said = match c {
                'b' => "b",
                'd' => "d",
                'f' => "f",
                'l' => "l",
                'm' => "m",
                'n' => "n",
                'p' => "p",
                't' => "t",
                'v' => "v",
                'z' => "z",
                'ñ' => "ɲ",
                _ => return 1,
            };
            push(out, said);
            if double { 2 } else { 1 }
        }
    }
}

/// Words that make `tous` before them a determiner, its `s` silent.
const DETERMINERS: &[&str] = &[
    "les", "ces", "mes", "tes", "ses", "nos", "vos", "leurs", "ceux", "deux", "trois", "quatre",
    "cinq", "six", "sept", "huit", "dix", "cent", "celles",
];

/// `son` after `de` takes stress; `tous` alone says its `s`.
fn stress_after_de(words: &mut [Said]) {
    for k in 0..words.len() {
        let next = words.get(k + 1).map(|w| w.text.clone()).unwrap_or_default();
        if let Some(word) = words.get_mut(k)
            && word.text == "tous"
            && !DETERMINERS.contains(&next.as_str())
            && let Part::Word(sounds) = &mut word.spoken.part
        {
            sounds.push(Sound::consonant("s"));
            word.latent = None;
        }
    }
    for k in 1..words.len() {
        let after_de = words.get(k - 1).is_some_and(|w| w.text == "de");
        let Some(word) = words.get_mut(k) else {
            continue;
        };
        if after_de
            && word.text == "son"
            && let Part::Word(sounds) = &mut word.spoken.part
            && let Some(sound) = sounds.iter_mut().find(|s| s.vowel)
        {
            sound.stress = Stress::Primary;
            word.latent = None;
        }
    }
}

/// Lends each latent consonant to an open word that follows it.
fn link(words: &mut [Said]) {
    for k in 0..words.len().saturating_sub(1) {
        let Some((head, tail)) = words.split_at_mut_checked(k + 1) else {
            continue;
        };
        let (Some(this), Some(next)) = (head.last_mut(), tail.first()) else {
            continue;
        };
        let vingt = this.text == "vingt" && !next.space;
        let lent = if vingt {
            Some("t")
        } else if next.space {
            this.latent
        } else {
            this.joined
        };
        let Some(latent) = lent else {
            continue;
        };
        let open = next.open || vingt;
        if !open || !matches!(next.spoken.part, Part::Word(_)) {
            continue;
        }
        if let Part::Word(sounds) = &mut this.spoken.part {
            let said = sounds.last().is_some_and(|s| s.ipa == latent);
            if !said {
                sounds.push(Sound::consonant(latent));
            }
        }
    }
    for word in words.iter_mut() {
        word.spoken.space = word.space;
    }
}

const ONES: [&str; 20] = [
    "z e ʁ o",
    "œ̃",
    "d ø",
    "t ʁ w a",
    "k a t ʁ",
    "s ɛ̃ k",
    "s i s",
    "s ɛ t",
    "y i t",
    "n œ f",
    "d i s",
    "ɔ̃ z",
    "d u z",
    "t ʁ ɛ z",
    "k a t ɔ ʁ z",
    "k ɛ̃ z",
    "s ɛ z",
    "d i s s ɛ t",
    "d i z y i t",
    "d i z n œ f",
];

const TENS: [&str; 7] = [
    "",
    "",
    "v ɛ̃",
    "t ʁ ɑ̃ t",
    "k a ʁ ɑ̃ t",
    "s ɛ̃ k ɑ̃ t",
    "s w a s ɑ̃ t",
];

fn pick(table: &[&'static str], i: u64) -> &'static str {
    usize::try_from(i)
        .ok()
        .and_then(|i| table.get(i))
        .copied()
        .unwrap_or_default()
}

/// A number as words, only the last stressed.
fn number(text: &str, space: bool, words: &mut Vec<Said>) {
    let written = letters::number(text);
    let mut groups: Vec<Vec<Sound>> = Vec::new();
    match written.whole {
        Some(n) => cardinal(n, &mut groups),
        None => {
            for ch in text.chars().filter(char::is_ascii_digit) {
                groups.push(parse(pick(&ONES, u64::from(ch) - u64::from('0'))));
            }
        }
    }
    if let Some(fraction) = written.fraction {
        groups.push(parse("v i ʁ ɡ y l"));
        match fraction.parse::<u64>() {
            Ok(n) if !fraction.starts_with('0') => cardinal(n, &mut groups),
            _ => {
                for ch in fraction.chars().filter(char::is_ascii_digit) {
                    groups.push(parse(pick(&ONES, u64::from(ch) - u64::from('0'))));
                }
            }
        }
    }
    let count = groups.len();
    for (i, mut sounds) in groups.into_iter().enumerate() {
        if i + 1 == count
            && let Some(sound) = sounds.iter_mut().rev().find(|s| s.vowel)
        {
            sound.stress = Stress::Primary;
        }
        let last = sounds.last().map(|s| s.ipa);
        let latent = match last {
            Some("ø" | "a" | "i") => Some("z"),
            Some("ɛ̃" | "ɑ̃") => Some("t"),
            _ => None,
        };
        words.push(Said {
            spoken: Spoken {
                part: Part::Word(sounds),
                space: true,
            },
            space: if i == 0 { space } else { true },
            latent: if i + 1 == count { latent } else { None },
            open: false,
            joined: None,
            text: String::new(),
        });
    }
}

/// `n` as word groups: thousands, hundreds, and the rest glued.
fn cardinal(n: u64, groups: &mut Vec<Vec<Sound>>) {
    if n >= 1_000_000 {
        let millions = n / 1_000_000;
        if millions > 1 {
            cardinal(millions, groups);
        } else {
            groups.push(parse("œ̃"));
        }
        groups.push(parse("m i l j ɔ̃"));
        if !n.is_multiple_of(1_000_000) {
            cardinal(n % 1_000_000, groups);
        }
        return;
    }
    if n >= 1000 {
        if n / 1000 > 1 {
            cardinal(n / 1000, groups);
        }
        groups.push(parse("m i l"));
        if !n.is_multiple_of(1000) {
            cardinal(n % 1000, groups);
        }
        return;
    }
    if n >= 100 {
        let mut sounds = Vec::new();
        if n / 100 > 1 {
            let short = match n / 100 {
                5 => "s ɛ̃",
                6 => "s i",
                8 => "y i",
                d => pick(&ONES, d),
            };
            sounds.extend(parse(short));
        }
        sounds.extend(parse("s ɑ̃"));
        groups.push(sounds);
        if !n.is_multiple_of(100) {
            cardinal(n % 100, groups);
        }
        return;
    }
    groups.push(tens(n));
}

/// Below a hundred, as one glued word.
fn tens(n: u64) -> Vec<Sound> {
    if n < 20 {
        return parse(pick(&ONES, n));
    }
    let (ten, unit) = match n {
        70..=79 => (6, n - 60),
        80..=99 => (8, n - 80),
        _ => (n / 10, n % 10),
    };
    let mut sounds = if ten == 8 {
        parse("k a t ʁ ə v ɛ̃")
    } else {
        parse(pick(&TENS, ten))
    };
    if unit == 0 {
        return sounds;
    }
    if ten == 2 {
        sounds.push(Sound::consonant("t"));
    }
    if matches!(unit, 1 | 11) && ten != 8 {
        sounds.extend(parse("e"));
    }
    sounds.extend(parse(pick(&ONES, unit)));
    sounds
}
