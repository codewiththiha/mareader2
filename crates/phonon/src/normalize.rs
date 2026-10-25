//! English made speakable before tokenizing, as the reference does it.

mod rules;
mod tlds;

/// Spoken forms of the symbols read aloud.
const AT: &str = "at";
const PLUS: &str = "plus";
const EQUALS: &str = "equals";
const COLON: &str = "colon";
const SLASH: &str = "slash";
const DASH: &str = "dash";
const DOT: &str = "dot";
const UNDERSCORE: &str = "underscore";

/// `text` as English the model reads well.
pub fn normalize(input: &str) -> String {
    let mut res = Appender::default();
    let mut chars = input.chars().peekable();
    let mut prev: Option<char> = None;
    while let Some(c) = chars.next() {
        match c {
            c if is_double_quote(c) => res.push('"'),
            c if is_single_quote(c) => res.push('\''),
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2015}' => res.push('-'),
            '★' | '☆' => {
                let mut full = usize::from(c == '★');
                let mut empty = usize::from(c == '☆');
                while let Some(&next) = chars.peek().filter(|&&c| c == '★' || c == '☆') {
                    full += usize::from(next == '★');
                    empty += usize::from(next == '☆');
                    chars.next();
                }
                let half = chars.next_if_eq(&'½').is_some();
                let n = match half {
                    false => full.to_string(),
                    true => format!("{full}.5"),
                };
                let spoken = match empty > 0 {
                    true => format!("{n} out of {} stars", full + empty + usize::from(half)),
                    false => format!("{n} stars"),
                };
                res.push_spoken(&spoken);
            }
            '°' | 'º' if res.ends_with_n() => {
                if res.buffer.last() == Some(&'.') {
                    res.buffer.pop();
                }
                match res.buffer.pop() == Some('N') {
                    true => res.push_spoken("Number"),
                    false => res.push_spoken("number"),
                }
            }
            '°' => {
                res.push_spoken(if res.ends_with_one() {
                    "degree"
                } else {
                    "degrees"
                });
                let scale = match chars.peek() {
                    Some('C') => Some("Celsius"),
                    Some('F') => Some("Fahrenheit"),
                    _ => None,
                };
                let mut after = chars.clone();
                after.next();
                if let Some(scale) =
                    scale.filter(|_| !after.peek().is_some_and(|c| c.is_alphanumeric()))
                {
                    chars.next();
                    res.push_str(scale);
                }
            }
            c if unaccented(c).is_some() => res.push(unaccented(c).unwrap_or(c)),
            c if subscript(c).is_some() => res.push(subscript(c).unwrap_or(c)),
            c if superscript(c).is_some() => {
                let mut n: String = superscript(c).into_iter().collect();
                while let Some(d) = chars.peek().and_then(|&c| superscript(c)) {
                    n.push(d);
                    chars.next();
                }
                match prev.is_some_and(|p| p.is_alphanumeric() || p == ')') {
                    true => res.push_spoken(&power(&n)),
                    false => res.push_str(&n),
                }
            }
            c if greek(c).is_some() => res.push_spoken(greek(c).unwrap_or_default()),
            c if symbol(c).is_some() => res.push_spoken(symbol(c).unwrap_or_default()),
            '±' => res.push_spoken("plus or minus"),
            '❤' | '♥' => {
                chars.next_if_eq(&'\u{FE0F}');
                res.push_spoken("love");
            }
            '~' if chars.peek().is_some_and(char::is_ascii_digit) => res.push_spoken("about"),
            '*' if prev.is_none_or(char::is_whitespace)
                && chars.peek().is_some_and(char::is_ascii_digit) =>
            {
                res.push_spoken("star")
            }
            '–' | '*' | '—' | '[' | ']' | '{' | '}' => res.push_whitespace(),
            '•' | '‣' | '◦' | '·' | '→' | '←' | '↑' | '↓' | '➡' | '➜' => {
                res.push_whitespace()
            }
            // "250 €": a symbol after a number and a space joins it.
            c if rules::is_currency_symbol(c)
                && res.ends_with_spaced_digit()
                && !chars.peek().is_some_and(char::is_ascii_digit) =>
            {
                res.pop_whitespace();
                res.push(c);
            }
            '…' => res.push('.'),
            '=' => res.push_spoken(EQUALS),
            ':' if !prev.is_none_or(char::is_whitespace)
                && !chars.peek().is_none_or(|c| c.is_whitespace()) =>
            {
                res.push(':')
            }
            ';' | ':' | '(' | ')' => {
                res.pop_whitespace();
                res.push(',');
                res.push_whitespace();
            }
            c if is_emoji(c) || c.is_control() || c.is_whitespace() => res.push_whitespace(),
            c => res.push(c),
        }
        prev = Some(c);
    }
    let text = res.into_string();
    let text = rules::rewrite_text(&text, spell_symbols);
    strip_closing_quote(&text).to_owned()
}

/// Text built a character at a time, tidying space before `.` and `,`.
#[derive(Default)]
struct Appender {
    buffer: Vec<char>,
}

impl Appender {
    fn push(&mut self, c: char) {
        if c == '.' || c == ',' {
            while let Some(&l) = self.buffer.last() {
                if !(l.is_whitespace() || (l.is_ascii_punctuation() && !self.keeps(l))) {
                    break;
                }
                self.buffer.pop();
            }
        }
        self.buffer.push(c);
    }

    fn push_str(&mut self, s: &str) {
        for c in s.chars() {
            self.push(c);
        }
    }

    /// Whether punctuation `last` stays before a `.` or `,`.
    fn keeps(&self, last: char) -> bool {
        match last {
            '"' | '\'' | '@' | '+' | '%' | '#' | '&' => true,
            '$' => self.before_last().is_some_and(|c| c.is_ascii_digit()),
            _ => false,
        }
    }

    fn before_last(&self) -> Option<char> {
        self.buffer
            .len()
            .checked_sub(2)
            .and_then(|i| self.buffer.get(i))
            .copied()
    }

    /// Whether the text ends in a lone `1`, for "1 degree".
    fn ends_with_one(&self) -> bool {
        let end = self
            .buffer
            .iter()
            .rposition(|c| !c.is_whitespace())
            .map_or(0, |i| i + 1);
        let b = self.buffer.get(..end).unwrap_or_default();
        let before = b.len().checked_sub(2).and_then(|i| b.get(i));
        b.last() == Some(&'1')
            && !before.is_some_and(|&c| c.is_ascii_digit() || c == '.' || c == ',')
    }

    /// Whether the text ends in a lone `n` or `N`, as "n°" starts.
    fn ends_with_n(&self) -> bool {
        let b = self.buffer.strip_suffix(&['.']).unwrap_or(&self.buffer);
        let before = b.len().checked_sub(2).and_then(|i| b.get(i));
        matches!(b.last(), Some('n' | 'N')) && !before.is_some_and(|c| c.is_alphanumeric())
    }

    fn ends_with_spaced_digit(&self) -> bool {
        self.last_is_whitespace()
            && (self.buffer.iter().rev())
                .find(|c| !c.is_whitespace())
                .is_some_and(char::is_ascii_digit)
    }

    fn push_spoken(&mut self, word: &str) {
        self.push_whitespace();
        self.push_str(word);
        self.push_whitespace();
    }

    fn into_string(mut self) -> String {
        self.pop_whitespace();
        self.buffer.into_iter().collect()
    }

    fn last_is_whitespace(&self) -> bool {
        self.buffer.last().is_some_and(|c| c.is_whitespace())
    }

    fn push_whitespace(&mut self) {
        if !self.last_is_whitespace() && !self.buffer.is_empty() {
            self.push(' ');
        }
    }

    fn pop_whitespace(&mut self) {
        while self.last_is_whitespace() {
            self.buffer.pop();
        }
    }
}

fn power(n: &str) -> String {
    match n {
        "2" => "squared".into(),
        "3" => "cubed".into(),
        n => format!("to the power of {n}"),
    }
}

fn symbol(c: char) -> Option<&'static str> {
    Some(match c {
        '×' => "times",
        '÷' => "divided by",
        '≤' => "less than or equal to",
        '≥' => "greater than or equal to",
        '≠' => "is not equal to",
        '≈' => "approximately",
        '√' => "square root of",
        '∞' => "infinity",
        '∑' => "sigma",
        '‰' => "per mille",
        _ => return None,
    })
}

fn greek(c: char) -> Option<&'static str> {
    const NAMES: [&str; 24] = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
        "lambda", "mu", "nu", "xi", "omicron", "pi", "rho", "sigma", "tau", "upsilon", "phi",
        "chi", "psi", "omega",
    ];
    let (n, a, big) = (c as usize, 'α' as usize, 'Α' as usize);
    let i = match c {
        'α'..='ρ' => n - a,
        'ς' => 17,
        'σ'..='ω' => n - a - 1,
        'Α'..='Ρ' => n - big,
        'Σ'..='Ω' => n - big - 1,
        _ => return None,
    };
    NAMES.get(i).copied()
}

/// Letter pairs: each accented letter the tokenizer lacks, then its plain one.
const FOLDS: &str = "ÌIÒOÝYìiòoýyÿyĀAāaĂAăaĄAąaĆCćcĈCĉcĊCċcČCčcĎDďdĒEēeĔEĕeĖEėeĘEęeĚEěeĜGĝgĞGğgĠGġgĢGģgĤHĥhĨIĩiĪIīiĬIĭiĮIįiİIĴJĵjĶKķkĹLĺlĻLļlĽLľlŃNńnŅNņnŇNňnŌOōoŎOŏoŐOőoŔRŕrŖRŗrŘRřrŚSśsŜSŝsŞSşsŠSšsŢTţtŤTťtŨUũuŪUūuŬUŭuŮUůuŰUűuŲUųuŴWŵwŶYŷyŸYŹZźzŻZżzŽZžzƠOơoƯUưuǍAǎaǏIǐiǑOǒoǓUǔuǕUǖuǗUǘuǙUǚuǛUǜuǞAǟaǠAǡaǦGǧgǨKǩkǪOǫoǬOǭoǰjǴGǵgǸNǹnǺAǻaȀAȁaȂAȃaȄEȅeȆEȇeȈIȉiȊIȋiȌOȍoȎOȏoȐRȑrȒRȓrȔUȕuȖUȗuȘSșsȚTțtȞHȟhȦAȧaȨEȩeȪOȫoȬOȭoȮOȯoȰOȱoȲYȳyḀAḁaḂBḃbḄBḅbḆBḇbḈCḉcḊDḋdḌDḍdḎDḏdḐDḑdḒDḓdḔEḕeḖEḗeḘEḙeḚEḛeḜEḝeḞFḟfḠGḡgḢHḣhḤHḥhḦHḧhḨHḩhḪHḫhḬIḭiḮIḯiḰKḱkḲKḳkḴKḵkḶLḷlḸLḹlḺLḻlḼLḽlḾMḿmṀMṁmṂMṃmṄNṅnṆNṇnṈNṉnṊNṋnṌOṍoṎOṏoṐOṑoṒOṓoṔPṕpṖPṗpṘRṙrṚRṛrṜRṝrṞRṟrṠSṡsṢSṣsṤSṥsṦSṧsṨSṩsṪTṫtṬTṭtṮTṯtṰTṱtṲUṳuṴUṵuṶUṷuṸUṹuṺUṻuṼVṽvṾVṿvẀWẁwẂWẃwẄWẅwẆWẇwẈWẉwẊXẋxẌXẍxẎYẏyẐZẑzẒZẓzẔZẕzẖhẗtẘwẙyẠAạaẢAảaẤAấaẦAầaẨAẩaẪAẫaẬAậaẮAắaẰAằaẲAẳaẴAẵaẶAặaẸEẹeẺEẻeẼEẽeẾEếeỀEềeỂEểeỄEễeỆEệeỈIỉiỊIịiỌOọoỎOỏoỐOốoỒOồoỔOổoỖOỗoỘOộoỚOớoỜOờoỞOởoỠOỡoỢOợoỤUụuỦUủuỨUứuỪUừuỬUửuỮUữuỰUựuỲYỳyỴYỵyỶYỷyỸYỹy";

/// The plain letter for `c`, when the tokenizer has no piece for it.
fn unaccented(c: char) -> Option<char> {
    if (c as u32) < 0xCC {
        return None;
    }
    let mut it = FOLDS.chars();
    while let (Some(a), Some(b)) = (it.next(), it.next()) {
        if a == c {
            return Some(b);
        }
    }
    None
}

fn subscript(c: char) -> Option<char> {
    Some(match c {
        '₀'..='₉' => char::from_digit(c as u32 - '₀' as u32, 10)?,
        '₊' => '+',
        '₋' => '-',
        'ₐ' => 'a',
        'ₑ' => 'e',
        'ₒ' => 'o',
        'ₓ' => 'x',
        'ₕ' => 'h',
        'ₖ' => 'k',
        'ₗ' => 'l',
        'ₘ' => 'm',
        'ₙ' => 'n',
        'ₚ' => 'p',
        'ₛ' => 's',
        'ₜ' => 't',
        'ᵢ' => 'i',
        'ⱼ' => 'j',
        _ => return None,
    })
}

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '⁰' => '0',
        '¹' => '1',
        '²' => '2',
        '³' => '3',
        '⁴'..='⁹' => char::from_digit(c as u32 - '⁴' as u32 + 4, 10)?,
        '⁻' => '-',
        '⁺' => '+',
        'ⁿ' => 'n',
        _ => return None,
    })
}

fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE0F | 0x200D)
}

fn is_double_quote(c: char) -> bool {
    matches!(
        c,
        '»' | '«'
            | '‹'
            | '›'
            | '“'
            | '”'
            | '„'
            | '‟'
            | '″'
            | '‶'
            | '⹂'
            | '❝'
            | '❞'
            | '❠'
            | '〝'
            | '〞'
            | '〟'
            | '＂'
    )
}

fn is_single_quote(c: char) -> bool {
    matches!(
        c,
        '‘' | '’'
            | '‚'
            | '‛'
            | '′'
            | '‵'
            | '❛'
            | '❜'
            | '❟'
            | '＇'
            | 'ʼ'
            | 'ʻ'
            | 'ʹ'
            | '´'
            | '`'
    )
}

/// A closing quote after the final punctuation is dropped.
fn strip_closing_quote(text: &str) -> &str {
    let trimmed = text.trim_end_matches('"');
    match trimmed.strip_suffix('\'') {
        Some(rest) if rest.ends_with(['.', '!', '?', ',', ';']) => rest,
        _ => trimmed,
    }
}

/// A word no rule claimed: `@` and `+` spoken, `and/or` read as two words.
fn spell_symbols(word: &str) -> String {
    let rewrite = |w: &str| rules::rewrite_word(w).unwrap_or_else(|| w.to_owned());
    if let Some(text) = split_slash(word) {
        return text.split(' ').map(rewrite).collect::<Vec<_>>().join(" ");
    }
    if !word.contains(['@', '+']) {
        return word.to_owned();
    }
    let mut res = Appender::default();
    for c in word.chars() {
        match c {
            '@' => res.push_spoken(AT),
            '+' => res.push_spoken(PLUS),
            c => res.push(c),
        }
    }
    let text = res.into_string();
    text.split(' ').map(rewrite).collect::<Vec<_>>().join(" ")
}

/// Units written with a slash between letters, which keep it.
const SLASH_UNITS: &[&str] = &[
    "km", "kg", "mg", "ml", "mL", "dL", "mol", "kJ", "kW", "kWh", "Hz", "min", "hr", "hrs", "sec",
    "ft", "lb", "lbs", "oz", "mi", "mph", "kph", "yd", "cm", "mm", "yr", "wk", "mo", "kcal", "rpm",
];

/// `word` with slashes as spaces, when it joins words that aren't units.
fn split_slash(word: &str) -> Option<String> {
    let body = word.trim_end_matches(|c: char| c.is_ascii_punctuation() && c != '/');
    let parts: Vec<&str> = body.split('/').collect();
    let wordlike = |p: &&str| {
        p.chars().count() >= 2 && p.chars().all(char::is_alphabetic) && !SLASH_UNITS.contains(p)
    };
    let rest = word.get(body.len()..).unwrap_or_default();
    (parts.len() > 1 && parts.iter().all(wordlike)).then(|| format!("{}{rest}", parts.join(" ")))
}
