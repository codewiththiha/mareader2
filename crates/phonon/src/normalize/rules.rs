//! Word rewrites: the first rule that claims a word wins; none chain.

use std::borrow::Cow;

use super::tlds::is_tld;
use super::{AT, COLON, DASH, DOT, PLUS, SLASH, UNDERSCORE};

/// Sentence punctuation a rule carries through at a word's end.
const PUNCTUATION: &str = "!?.:,;…";

/// Each word rule, in the order tried.
const RULES: [fn(&str) -> Option<String>; 7] = [
    numbers,
    currency,
    dashed_digits,
    emails,
    urls,
    abbreviations,
    elongations,
];

/// `word` rewritten by the first rule claiming it.
pub(super) fn rewrite_word(word: &str) -> Option<String> {
    RULES.iter().find_map(|rule| rule(word))
}

/// Every word of `text` rewritten; `unclaimed` takes what no rule does.
pub(super) fn rewrite_text(text: &str, unclaimed: impl Fn(&str) -> String) -> String {
    let words: Vec<&str> = text.split(' ').collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while let Some(rest) = words.get(i..).filter(|r| !r.is_empty()) {
        let prev = out.last().map(String::as_str);
        let span = scaled(rest)
            .or_else(|| digit_run(rest))
            .or_else(|| ending_in(prev, rest))
            .or_else(|| ranks(rest));
        if let Some((rewritten, used)) = span {
            out.push(rewritten);
            i += used;
            continue;
        }
        let word = rest.first().copied().unwrap_or_default();
        out.push(rewrite_word(word).unwrap_or_else(|| unclaimed(word)));
        i += 1;
    }
    out.join(" ")
}

/// `rest` when it is only carried punctuation.
fn suffix(rest: &str) -> Option<&str> {
    rest.chars()
        .all(|c| PUNCTUATION.contains(c))
        .then_some(rest)
}

/// `word` without its trailing punctuation, and that punctuation.
fn split_suffix(word: &str) -> (&str, &str) {
    let body = word.trim_end_matches(|c| PUNCTUATION.contains(c));
    (body, word.get(body.len()..).unwrap_or_default())
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// A two-letter address part spelled out: "fr" is "F-R", "www" "W-W-W".
fn spell_short(word: &str) -> Cow<'_, str> {
    match word.as_bytes() {
        b"www" | b"WWW" => "W-W-W".into(),
        [a, b] if a.is_ascii() && b.is_ascii() => format!(
            "{}-{}",
            a.to_ascii_uppercase() as char,
            b.to_ascii_uppercase() as char
        )
        .into(),
        _ => word.into(),
    }
}

/// Big numbers with scale words; codes spelled, ranges read with "to".
fn numbers(word: &str) -> Option<String> {
    let (body, end) = split_suffix(word);
    if body.len() >= 2 && body.starts_with('0') && all_digits(body) {
        return Some(format!("{}{end}", spell(body)));
    }
    if let Some(range) = range(body) {
        return Some(format!("{range}{end}"));
    }
    if let Some((words, suffix)) = number_words(word) {
        return Some(format!("{words}{suffix}"));
    }
    let (head, tail) = word.split_at(word.find('-')?);
    if !tail
        .get(1..)
        .is_some_and(|t| t.starts_with(char::is_alphabetic))
    {
        return None;
    }
    match number_words(head)? {
        (words, "") => Some(format!("{words}{tail}")),
        _ => None,
    }
}

fn range(body: &str) -> Option<String> {
    let (from, to) = body.split_once('-')?;
    let small = |s: &str| {
        (2..=3).contains(&s.len()) && all_digits(s) && (s.len() == 1 || !s.starts_with('0'))
    };
    let year =
        |s: &str| s.len() == 4 && all_digits(s) && (s.starts_with('1') || s.starts_with("20"));
    let ok = match (from.len(), to.len()) {
        _ if small(from) && small(to) => from.parse::<u32>().ok()? < to.parse::<u32>().ok()?,
        (4, 4) => year(from) && year(to) && from < to,
        (4, 2) => year(from) && all_digits(to) && from.get(2..).is_some_and(|f| f < to),
        _ => false,
    };
    ok.then(|| format!("{from} to {to}"))
}

/// Digits read one by one; dashes dropped.
fn spell(s: &str) -> String {
    const DIGITS: [&str; 10] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    ];
    let spoken = s.chars().filter(|&c| c != '-').map(|c| {
        let digit = c.to_digit(10).and_then(|d| DIGITS.get(d as usize));
        digit.map_or_else(|| c.to_string(), |w| (*w).to_owned())
    });
    spoken.collect::<Vec<_>>().join(" ")
}

/// Runs of digit groups, as codes and phone numbers are, spelled.
fn digit_run(words: &[&str]) -> Option<(String, usize)> {
    let group = |s: &str| {
        (1..=8).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
            && s.bytes().any(|b| b.is_ascii_digit())
    };
    let plus = words.first()?.starts_with('+');
    let mut bodies = vec![];
    let mut end = "";
    for word in words {
        let (body, suffix) = split_suffix(word);
        let body = match bodies.is_empty() && plus {
            true => body.get(1..).unwrap_or_default(),
            false => body,
        };
        if !group(body) {
            break;
        }
        bodies.push(body);
        if !suffix.is_empty() {
            end = suffix;
            break;
        }
    }
    let years = (bodies.iter()).all(|b| b.len() == 4 && matches!(b.get(..2), Some("19" | "20")));
    let long = bodies.len() >= 3 && !years;
    let leading_zero = bodies.iter().any(|b| b.starts_with('0'));
    if !long && !(bodies.len() == 2 && (plus || leading_zero)) {
        return None;
    }
    let spelled = bodies
        .iter()
        .map(|b| spell(b))
        .collect::<Vec<_>>()
        .join(", ");
    let spelled = match plus {
        true => format!("{PLUS} {spelled}"),
        false => spelled,
    };
    Some((format!("{spelled}{end}"), bodies.len()))
}

/// "ending in -1234", spelled.
fn ending_in(prev: Option<&str>, words: &[&str]) -> Option<(String, usize)> {
    let prev = prev?.to_lowercase();
    if !matches!(prev.as_str(), "ending" | "ends" | "ended") || *words.first()? != "in" {
        return None;
    }
    let (body, end) = split_suffix(words.get(1)?);
    let code = body.strip_prefix('-')?;
    let ok = |b: u8| b.is_ascii_digit() || b.is_ascii_uppercase() || b == b'-';
    if !code.bytes().any(|b| b.is_ascii_digit()) || !code.bytes().all(ok) {
        return None;
    }
    Some((format!("in {}{end}", spell(code)), 2))
}

/// What [`numbers`] reads `word` as, and the punctuation it carries.
fn number_words(word: &str) -> Option<(String, &str)> {
    let mut lead = word.chars();
    if lead.next() == Some('0') && lead.next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    let (negative, rest) = match word.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, word),
    };
    let end = (rest.find(|c: char| !c.is_ascii_digit() && c != ',')).unwrap_or(rest.len());
    let int = rest.get(..end)?.trim_end_matches(',');
    if int.is_empty() {
        return None;
    }
    let rest = rest.get(int.len()..)?;
    let (decimal, rest) = match rest.strip_prefix('.') {
        Some(after) => match after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len())
        {
            0 => (None, rest),
            end => (Some(after.get(..end)?), after.get(end..)?),
        },
        None => (None, rest),
    };
    let suffix = suffix(rest)?;
    let digits: String = int.chars().filter(char::is_ascii_digit).collect();
    let value: i64 = digits.parse().ok()?;
    if value >= 1_000_000_000_000 {
        return None;
    }
    let mut words = match value < 1000 || (1900..2100).contains(&value) {
        true => value.to_string(),
        false => {
            let scales = [
                (1_000_000_000, "billion"),
                (1_000_000, "million"),
                (1000, "thousand"),
            ];
            let mut parts: Vec<String> = (scales.iter())
                .filter_map(|(div, name)| match (value / div) % 1000 {
                    0 => None,
                    group => Some(format!("{group} {name}")),
                })
                .collect();
            if value % 1000 != 0 {
                parts.push((value % 1000).to_string());
            }
            parts.join(" ")
        }
    };
    if let Some(decimal) = decimal {
        words = format!("{words} point {decimal}");
    }
    if negative {
        words = format!("minus {words}");
    }
    Some((words, suffix))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Currency {
    Dollar,
    Euro,
    Pound,
    Yen,
    Cent,
}

const SYMBOLS: [char; 5] = ['$', '€', '£', '¥', '¢'];

impl Currency {
    fn from_symbol(symbol: char) -> Option<Self> {
        match symbol {
            '$' => Some(Self::Dollar),
            '€' => Some(Self::Euro),
            '£' => Some(Self::Pound),
            '¥' => Some(Self::Yen),
            '¢' => Some(Self::Cent),
            _ => None,
        }
    }

    /// Its name, singular and plural.
    fn name(self) -> (&'static str, &'static str) {
        match self {
            Self::Dollar => ("dollar", "dollars"),
            Self::Euro => ("euro", "euros"),
            Self::Pound => ("pound", "pounds"),
            Self::Yen => ("yen", "yen"),
            Self::Cent => ("cent", "cents"),
        }
    }
}

/// Whether `c` is a currency symbol the rules read.
pub(super) fn is_currency_symbol(c: char) -> bool {
    Currency::from_symbol(c).is_some()
}

/// An amount of money: "$4.59" is "4 dollars and 59 cents".
fn currency(word: &str) -> Option<String> {
    let (body, suffix) = split_suffix(word);
    if let Some(body) = body.strip_prefix('-')
        && let Some(amount) = body.strip_prefix(SYMBOLS)
    {
        let symbol = body.get(..body.len() - amount.len())?;
        return currency(&format!("{symbol}-{amount}{suffix}"));
    }
    let (currency, amount) = split_symbol(body)?;
    if let Some((amount, scale)) = split_scale(amount) {
        return with_scale(amount, scale, currency).map(|spoken| format!("{spoken}{suffix}"));
    }
    if currency != Currency::Cent
        && let Some(spoken) = with_cents(amount, currency)
    {
        return Some(format!("{spoken}{suffix}"));
    }
    let (spoken, _) = number_words(amount)?;
    // The plural even for 1, as the reference reads it; but "1 cent".
    let (one, many) = currency.name();
    let name = match currency == Currency::Cent && amount == "1" {
        true => one,
        false => many,
    };
    Some(format!("{spoken} {name}{suffix}"))
}

/// An amount with a scale letter or word joined: "2.5bn", "3k".
fn split_scale(amount: &str) -> Option<(&str, &'static str)> {
    const SUFFIXES: [(&str, &str); 14] = [
        ("thousand", "thousand"),
        ("million", "million"),
        ("billion", "billion"),
        ("trillion", "trillion"),
        ("bn", "billion"),
        ("BN", "billion"),
        ("mn", "million"),
        ("tn", "trillion"),
        ("k", "thousand"),
        ("K", "thousand"),
        ("m", "million"),
        ("M", "million"),
        ("b", "billion"),
        ("B", "billion"),
    ];
    SUFFIXES.iter().find_map(|&(suffix, scale)| {
        let number = amount.strip_suffix(suffix)?;
        number
            .ends_with(|c: char| c.is_ascii_digit())
            .then_some((number, scale))
    })
}

fn scale_word(word: &str) -> bool {
    matches!(word, "thousand" | "million" | "billion" | "trillion")
}

fn with_scale(amount: &str, scale: &str, currency: Currency) -> Option<String> {
    let (amount, rest) = number_words(amount)?;
    if !rest.is_empty() {
        return None;
    }
    let (_, name) = currency.name();
    Some(format!("{amount} {scale} {name}"))
}

/// "$2 million": an amount and a scale word after it.
fn scaled(words: &[&str]) -> Option<(String, usize)> {
    let [word, next, ..] = words else {
        return None;
    };
    let (scale, suffix) = split_suffix(next);
    let (currency, amount) = split_symbol(word)?;
    if !scale_word(scale) {
        return None;
    }
    let spoken = with_scale(amount, scale, currency)?;
    Some((format!("{spoken}{suffix}"), 2))
}

fn split_symbol(word: &str) -> Option<(Currency, &str)> {
    match word.strip_prefix(SYMBOLS) {
        Some(amount) => Some((Currency::from_symbol(word.chars().next()?)?, amount)),
        None => {
            let amount = word.strip_suffix(SYMBOLS)?;
            Some((Currency::from_symbol(word.chars().next_back()?)?, amount))
        }
    }
}

/// An amount with exactly two decimals, read in cents.
fn with_cents(amount: &str, currency: Currency) -> Option<String> {
    let grouped = |whole: &str| {
        (whole.split(',').enumerate()).all(|(i, group)| {
            all_digits(group)
                && match i {
                    0 => group.len() <= 3,
                    _ => group.len() == 3,
                }
        })
    };
    let (whole, cents) =
        (amount.rsplit_once('.')).filter(|(whole, _)| all_digits(whole) || grouped(whole))?;
    if cents.len() != 2 || !all_digits(cents) {
        return None;
    }
    let (whole, _) = number_words(whole)?;
    let (one, many) = currency.name();
    let units = format!("{whole} {}", if whole == "1" { one } else { many });
    if cents == "00" {
        return Some(units);
    }
    let cents = cents.strip_prefix('0').unwrap_or(cents);
    if whole == "0" {
        return Some(format!("{cents} cents"));
    }
    Some(format!("{units} and {cents} cents"))
}

/// Digit groups split by two or more `-` or `_`, read group by group.
fn dashed_digits(word: &str) -> Option<String> {
    let (body, suffix) = split_suffix(word);
    if !body.chars().all(|c| c.is_ascii_digit() || is_separator(c))
        || body.matches(is_separator).count() < 2
    {
        return None;
    }
    let groups = groups(body)?;
    if !body.contains('_') && looks_like_date(&groups) {
        return None;
    }
    let mut words = vec![];
    for (group, separator) in groups {
        match group.len() >= 4 {
            true => words.extend((0..group.len()).filter_map(|i| group.get(i..=i))),
            false => words.push(group),
        }
        if separator == Some('_') {
            words.push(UNDERSCORE);
        }
    }
    Some(format!("{}{suffix}", words.join(" ")))
}

fn is_separator(c: char) -> bool {
    c == '-' || c == '_'
}

/// `body` cut at its separators; None when one leads, trails or doubles.
fn groups(body: &str) -> Option<Vec<(&str, Option<char>)>> {
    let mut groups = vec![];
    let mut start = 0;
    for (i, c) in body.char_indices().filter(|&(_, c)| is_separator(c)) {
        if i == start {
            return None;
        }
        groups.push((body.get(start..i)?, Some(c)));
        start = i + 1;
    }
    if start == body.len() {
        return None;
    }
    groups.push((body.get(start..)?, None));
    Some(groups)
}

/// Whether three groups read as a date, either way round.
fn looks_like_date(groups: &[(&str, Option<char>)]) -> bool {
    fn day_month(x: &str, y: &str) -> bool {
        if x.len() > 2 || y.len() > 2 {
            return false;
        }
        match (x.parse::<u32>(), y.parse::<u32>()) {
            (Ok(x), Ok(y)) => {
                ((1..=31).contains(&x) && (1..=12).contains(&y))
                    || ((1..=12).contains(&x) && (1..=31).contains(&y))
            }
            _ => false,
        }
    }
    match groups {
        [(a, _), (b, _), (c, _)] => {
            (a.len() == 4 && day_month(b, c)) || ((c.len() == 4 || c.len() == 2) && day_month(a, b))
        }
        _ => false,
    }
}

/// An email address with its separators spoken.
fn emails(word: &str) -> Option<String> {
    let (user, rest) = word.split_once('@')?;
    let user_char = |c: char| c.is_ascii_alphanumeric() || "-_.+".contains(c);
    if user.is_empty() || !user.chars().all(user_char) {
        return None;
    }
    let (first, mut rest) = label(rest)?;
    let mut labels = vec![first];
    while let Some((next, after)) = rest.strip_prefix('.').and_then(label) {
        labels.push(next);
        rest = after;
    }
    let suffix = suffix(rest)?;
    let (tld, domain) = labels
        .split_last()
        .filter(|(_, domain)| !domain.is_empty())?;
    let mut words = vec![];
    speak_email(user, &mut words, |c| match c {
        '-' => Some(DASH),
        '.' => Some(DOT),
        '+' => Some(PLUS),
        '_' => Some(UNDERSCORE),
        _ => None,
    });
    words.push(AT);
    for (i, part) in domain.iter().enumerate() {
        if i > 0 {
            words.push(DOT);
        }
        speak_email(part, &mut words, |c| (c == '-').then_some(DASH));
    }
    words.push(DOT);
    Some(format!("{} {}{suffix}", words.join(" "), spell_short(tld)))
}

/// The domain label `s` starts with, and the rest.
fn label(s: &str) -> Option<(&str, &str)> {
    let n = (s.bytes())
        .take_while(|&b| b.is_ascii_alphanumeric() || b == b'-')
        .count();
    (n > 0).then(|| s.split_at(n))
}

/// `part`'s words, each separator `spoken` knows replaced by its word.
fn speak_email<'a>(
    part: &'a str,
    words: &mut Vec<&'a str>,
    spoken: impl Fn(char) -> Option<&'a str>,
) {
    let mut start = 0;
    for (i, c) in part.char_indices() {
        if let Some(word) = spoken(c) {
            if start < i {
                words.extend(part.get(start..i));
            }
            words.push(word);
            start = i + c.len_utf8();
        }
    }
    if start < part.len() {
        words.extend(part.get(start..));
    }
}

/// A web address ending in a known domain, separators spoken.
fn urls(word: &str) -> Option<String> {
    let mut words: Vec<Cow<str>> = vec![];
    let rest = if let Some(rest) = word.strip_prefix("https://") {
        words.push("H-T-T-P-S".into());
        rest
    } else if let Some(rest) = word.strip_prefix("http://") {
        words.push("H-T-T-P".into());
        rest
    } else {
        word
    };
    if !words.is_empty() {
        words.extend([COLON, SLASH, SLASH].map(Cow::from));
    }
    let part = |c: char| c.is_alphanumeric() || c == '-';
    let (first, mut rest) = run(rest, part)?;
    let mut labels = vec![first];
    while let Some((next, after)) = rest.strip_prefix('.').and_then(|r| run(r, part)) {
        labels.push(next);
        rest = after;
    }
    let (path, rest) = match run(rest, |c| part(c) || c == '/') {
        Some((path, rest)) => (Some(path), rest),
        None => (None, rest),
    };
    let suffix = suffix(rest)?;
    let tld = *labels.last()?;
    if labels.len() < 2 || !is_tld(tld) {
        return None;
    }
    let capitalized = tld.chars().any(char::is_uppercase) && tld.chars().any(char::is_lowercase);
    let www = labels
        .first()
        .is_some_and(|l| l.eq_ignore_ascii_case("www"));
    if capitalized && words.is_empty() && !www {
        return None;
    }
    for (i, l) in labels.into_iter().enumerate() {
        if i > 0 {
            words.push(DOT.into());
        }
        speak_url(l, &mut words);
    }
    if let Some(path) = path {
        speak_url(path, &mut words);
    }
    Some(format!("{}{suffix}", words.join(" ")))
}

/// The run `s` starts with that `keep` accepts, and the rest.
fn run(s: &str, keep: impl Fn(char) -> bool) -> Option<(&str, &str)> {
    let n = s.find(|c| !keep(c)).unwrap_or(s.len());
    (n > 0).then(|| s.split_at(n))
}

/// A label's or path's words: separators spoken, digits one by one.
fn speak_url<'a>(part: &'a str, words: &mut Vec<Cow<'a, str>>) {
    let mut start = 0;
    for (i, c) in part.char_indices() {
        let word = match c {
            '-' => DASH,
            '/' => SLASH,
            '0'..='9' => part.get(i..=i).unwrap_or_default(),
            _ => continue,
        };
        if start < i {
            words.extend(part.get(start..i).map(spell_short));
        }
        words.push(word.into());
        start = i + 1;
    }
    if start < part.len() {
        words.extend(part.get(start..).map(spell_short));
    }
}

const ABBREVIATIONS: &[(&str, &str)] = &[
    ("Lt", "Lieutenant"),
    ("Lt.", "Lieutenant"),
    ("Col.", "Colonel"),
    ("Sgt.", "Sergeant"),
    ("Sgt", "Sergeant"),
    ("Capt.", "Captain"),
    ("Cpl.", "Corporal"),
    ("Gen.", "General"),
    ("Adm.", "Admiral"),
    ("Sen.", "Senator"),
    ("Sens.", "Senators"),
    ("Rep.", "Representative"),
    ("Reps.", "Representatives"),
    ("Gov.", "Governor"),
    ("Pres.", "President"),
    ("Det.", "Detective"),
    ("Insp.", "Inspector"),
    ("Supt.", "Superintendent"),
    ("Ft.", "Fort"),
    ("Mt.", "Mount"),
    ("Jan.", "January"),
    ("Feb.", "February"),
    ("Mar.", "March"),
    ("Apr.", "April"),
    ("Jun.", "June"),
    ("Jul.", "July"),
    ("Aug.", "August"),
    ("Sep.", "September"),
    ("Sept.", "September"),
    ("Oct.", "October"),
    ("Nov.", "November"),
    ("Dec.", "December"),
    ("Mon.", "Monday"),
    ("Tue.", "Tuesday"),
    ("Tues.", "Tuesday"),
    ("Wed.", "Wednesday"),
    ("Thu.", "Thursday"),
    ("Thur.", "Thursday"),
    ("Thurs.", "Thursday"),
    ("Fri.", "Friday"),
    ("Sat.", "Saturday"),
    ("Sun.", "Sunday"),
    ("vol.", "volume"),
    ("Vol.", "Volume"),
    ("ed.", "edition"),
    ("approx.", "approximately"),
    ("Approx.", "Approximately"),
    ("dept.", "department"),
    ("Dept.", "Department"),
    ("vs.", "versus"),
    ("w/", "with"),
    ("W/", "With"),
    ("w/o", "without"),
    ("W/o", "Without"),
];

fn abbreviations(word: &str) -> Option<String> {
    let (body, suffix) = split_suffix(word);
    let (short, suffix) = match suffix.strip_prefix('.') {
        Some(rest) => (word.get(..body.len() + 1)?, rest),
        None => (body, suffix),
    };
    let (_, long) = ABBREVIATIONS.iter().find(|(s, _)| *s == short)?;
    Some(format!("{long}{suffix}"))
}

/// "Lt Col", two words for one rank.
fn ranks(words: &[&str]) -> Option<(String, usize)> {
    let [first, second, ..] = words else {
        return None;
    };
    let (rank, suffix) = split_suffix(second);
    (*first == "Lt" && rank == "Col").then(|| (format!("Lieutenant Colonel{suffix}"), 2))
}

/// Stretched words cut back: "sooooo" is "soo".
fn elongations(word: &str) -> Option<String> {
    let (body, suffix) = split_suffix(word);
    let chars: Vec<char> = body.chars().collect();
    let stretched = |c: char| "aeiouyhw".contains(c.to_ascii_lowercase());
    let tripled = |w: &[char]| matches!(w, [a, b, c] if a == b && b == c && stretched(*a));
    if !chars.iter().all(char::is_ascii_alphabetic)
        || chars.iter().all(char::is_ascii_uppercase)
        || !chars.windows(3).any(tripled)
    {
        return None;
    }
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let run = i >= 1 && chars.get(i - 1) == Some(&c) && stretched(c);
        let near = chars.get(i.saturating_sub(2)..(i + 3).min(chars.len()));
        let long = (near.unwrap_or_default().windows(3)).any(|w| w.iter().all(|&x| x == c));
        if !(run && long) {
            out.push(c);
        }
    }
    Some(format!("{out}{suffix}"))
}
