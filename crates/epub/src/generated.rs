//! Generated content: `content`, counters, quotes and list marker styles.
//! See `docs/epub-support.md`.
use std::sync::Arc;

use crate::href;

/// Parts of one `content` value past this are dropped.
const MAX_ITEMS: usize = 32;
/// Counters one declaration names past this are dropped.
const MAX_COUNTERS: usize = 16;

/// One part of a `content` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    Text(String),
    /// The element's attribute of this name.
    Attr(String),
    Counter(String, ListStyle),
    /// Every counter of the name, outermost first, joined by the string.
    Counters(String, String, ListStyle),
    OpenQuote,
    CloseQuote,
    NoOpenQuote,
    NoCloseQuote,
    /// A picture, resolved against its sheet.
    Url(String),
}

/// A `content` value: nothing, or parts shown in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Content {
    None,
    Items(Arc<[Item]>),
}

impl Content {
    /// Resolves its pictures against `path`, where it was read.
    pub(crate) fn resolve(&mut self, path: &str) {
        let Content::Items(items) = self else {
            return;
        };
        if !items.iter().any(|i| matches!(i, Item::Url(_))) {
            return;
        }
        let resolved = items.iter().filter_map(|item| match item {
            Item::Url(raw) => resolve(path, raw).map(Item::Url),
            other => Some(other.clone()),
        });
        *items = resolved.collect();
    }
}

/// A picture's path against `path`; `data:` URLs stay as they are.
pub(crate) fn resolve(path: &str, raw: &str) -> Option<String> {
    match raw.starts_with("data:") {
        true => Some(raw.to_owned()),
        false => href::resolve(path, raw),
    }
}

/// How a counter or list item's number shows, as `list-style-type`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum ListStyle {
    None,
    /// The same mark on every item, as `disc`.
    Symbol(char),
    #[default]
    Decimal,
    DecimalZero,
    /// Ten digits from this code point up, as `myanmar` and `thai`.
    Digits(u32),
    CjkDecimal,
    /// Letters counted as a, b, … z, aa, ab.
    Alphabetic(&'static str),
    /// Symbols adding up to the number, as Roman numerals.
    Additive(&'static [(u32, &'static str)], bool),
    /// Chinese and Japanese numbers in words.
    CjkInformal,
    /// A string shown as written.
    Text(Arc<str>),
}

const ROMAN: &[(u32, &str)] = &[
    (1000, "M"),
    (900, "CM"),
    (500, "D"),
    (400, "CD"),
    (100, "C"),
    (90, "XC"),
    (50, "L"),
    (40, "XL"),
    (10, "X"),
    (9, "IX"),
    (5, "V"),
    (4, "IV"),
    (1, "I"),
];

const ARMENIAN: &[(u32, &str)] = &[
    (9000, "Ք"),
    (8000, "Փ"),
    (7000, "Ւ"),
    (6000, "Ց"),
    (5000, "Ր"),
    (4000, "Տ"),
    (3000, "Վ"),
    (2000, "Ս"),
    (1000, "Ռ"),
    (900, "Ջ"),
    (800, "Պ"),
    (700, "Չ"),
    (600, "Ո"),
    (500, "Շ"),
    (400, "Ն"),
    (300, "Յ"),
    (200, "Մ"),
    (100, "Ճ"),
    (90, "Ղ"),
    (80, "Ձ"),
    (70, "Հ"),
    (60, "Կ"),
    (50, "Ծ"),
    (40, "Խ"),
    (30, "Լ"),
    (20, "Ի"),
    (10, "Ժ"),
    (9, "Թ"),
    (8, "Ը"),
    (7, "Է"),
    (6, "Զ"),
    (5, "Ե"),
    (4, "Դ"),
    (3, "Գ"),
    (2, "Բ"),
    (1, "Ա"),
];

const GEORGIAN: &[(u32, &str)] = &[
    (10000, "ჵ"),
    (9000, "ჰ"),
    (8000, "ჯ"),
    (7000, "ჴ"),
    (6000, "ხ"),
    (5000, "ჭ"),
    (4000, "წ"),
    (3000, "ძ"),
    (2000, "ც"),
    (1000, "ჩ"),
    (900, "შ"),
    (800, "ყ"),
    (700, "ღ"),
    (600, "ქ"),
    (500, "ფ"),
    (400, "ჳ"),
    (300, "ტ"),
    (200, "ს"),
    (100, "რ"),
    (90, "ჟ"),
    (80, "პ"),
    (70, "ო"),
    (60, "ჲ"),
    (50, "ნ"),
    (40, "მ"),
    (30, "ლ"),
    (20, "კ"),
    (10, "ი"),
    (9, "თ"),
    (8, "ჱ"),
    (7, "ზ"),
    (6, "ვ"),
    (5, "ე"),
    (4, "დ"),
    (3, "გ"),
    (2, "ბ"),
    (1, "ა"),
];

const HEBREW: &[(u32, &str)] = &[
    (400, "ת"),
    (300, "ש"),
    (200, "ר"),
    (100, "ק"),
    (90, "צ"),
    (80, "פ"),
    (70, "ע"),
    (60, "ס"),
    (50, "נ"),
    (40, "מ"),
    (30, "ל"),
    (20, "כ"),
    (19, "יט"),
    (18, "יח"),
    (17, "יז"),
    (16, "טז"),
    (15, "טו"),
    (10, "י"),
    (9, "ט"),
    (8, "ח"),
    (7, "ז"),
    (6, "ו"),
    (5, "ה"),
    (4, "ד"),
    (3, "ג"),
    (2, "ב"),
    (1, "א"),
];

const LATIN: &str = "abcdefghijklmnopqrstuvwxyz";
const LATIN_UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const GREEK: &str = "αβγδεζηθικλμνξοπρστυφχψω";
const HIRAGANA: &str = "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわゐゑをん";
const KATAKANA: &str = "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヰヱヲン";
const HIRAGANA_IROHA: &str = "いろはにほへとちりぬるをわかよたれそつねならむうゐのおくやまけふこえてあさきゆめみしゑひもせす";
const KATAKANA_IROHA: &str = "イロハニホヘトチリヌルヲワカヨタレソツネナラムウヰノオクヤマケフコエテアサキユメミシヱヒモセス";
const CJK_DIGITS: [char; 10] = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];

/// A `list-style-type` word or string; `None` for other words.
pub(crate) fn list_style(word: &str) -> Option<ListStyle> {
    if let Some((text, rest)) = string(word)
        && rest.trim().is_empty()
    {
        return Some(ListStyle::Text(text.into()));
    }
    let digits = |zero: u32| Some(ListStyle::Digits(zero));
    match word.to_ascii_lowercase().as_str() {
        "none" => Some(ListStyle::None),
        "disc" => Some(ListStyle::Symbol('•')),
        "circle" => Some(ListStyle::Symbol('◦')),
        "square" => Some(ListStyle::Symbol('▪')),
        "disclosure-open" => Some(ListStyle::Symbol('▾')),
        "disclosure-closed" => Some(ListStyle::Symbol('▸')),
        "decimal" => Some(ListStyle::Decimal),
        "decimal-leading-zero" => Some(ListStyle::DecimalZero),
        "arabic-indic" => digits(0x660),
        "persian" => digits(0x6f0),
        "devanagari" => digits(0x966),
        "bengali" => digits(0x9e6),
        "gurmukhi" => digits(0xa66),
        "gujarati" => digits(0xae6),
        "oriya" => digits(0xb66),
        "tamil" => digits(0xbe6),
        "telugu" => digits(0xc66),
        "kannada" => digits(0xce6),
        "malayalam" => digits(0xd66),
        "thai" => digits(0xe50),
        "lao" => digits(0xed0),
        "tibetan" => digits(0xf20),
        "myanmar" => digits(0x1040),
        "khmer" | "cambodian" => digits(0x17e0),
        "mongolian" => digits(0x1810),
        "cjk-decimal" => Some(ListStyle::CjkDecimal),
        "lower-alpha" | "lower-latin" => Some(ListStyle::Alphabetic(LATIN)),
        "upper-alpha" | "upper-latin" => Some(ListStyle::Alphabetic(LATIN_UPPER)),
        "lower-greek" => Some(ListStyle::Alphabetic(GREEK)),
        "hiragana" => Some(ListStyle::Alphabetic(HIRAGANA)),
        "katakana" => Some(ListStyle::Alphabetic(KATAKANA)),
        "hiragana-iroha" => Some(ListStyle::Alphabetic(HIRAGANA_IROHA)),
        "katakana-iroha" => Some(ListStyle::Alphabetic(KATAKANA_IROHA)),
        "lower-roman" => Some(ListStyle::Additive(ROMAN, true)),
        "upper-roman" => Some(ListStyle::Additive(ROMAN, false)),
        "lower-armenian" => Some(ListStyle::Additive(ARMENIAN, true)),
        "upper-armenian" | "armenian" => Some(ListStyle::Additive(ARMENIAN, false)),
        "georgian" => Some(ListStyle::Additive(GEORGIAN, false)),
        "hebrew" => Some(ListStyle::Additive(HEBREW, false)),
        "cjk-ideographic"
        | "simp-chinese-informal"
        | "trad-chinese-informal"
        | "japanese-informal"
        | "korean-hanja-informal" => Some(ListStyle::CjkInformal),
        _ => None,
    }
}

/// `n` in `style`, as `counter()` shows it.
pub(crate) fn format(n: i32, style: &ListStyle) -> String {
    let decimal = || n.to_string();
    let positive = u32::try_from(n).ok().filter(|&n| n > 0);
    match style {
        ListStyle::None => String::new(),
        ListStyle::Symbol(c) => c.to_string(),
        ListStyle::Text(s) => s.to_string(),
        ListStyle::Decimal => decimal(),
        ListStyle::DecimalZero if (0..10).contains(&n) => format!("0{n}"),
        ListStyle::DecimalZero => decimal(),
        ListStyle::Digits(zero) => digits(n, |d| char::from_u32(zero + d)),
        ListStyle::CjkDecimal => digits(n, |d| CJK_DIGITS.get(d as usize).copied()),
        ListStyle::Alphabetic(letters) => positive
            .map(|n| alphabetic(n, letters))
            .unwrap_or_else(decimal),
        ListStyle::Additive(table, lower) => {
            let most = table.first().map_or(0, |t| t.0.saturating_mul(4) - 1);
            match positive.filter(|&n| n <= most) {
                Some(n) => additive(n, table, *lower),
                None => decimal(),
            }
        }
        ListStyle::CjkInformal => match positive.filter(|&n| n < 10_000) {
            Some(n) => cjk_informal(n),
            None => decimal(),
        },
    }
}

/// A list item's marker, as `1.` or `•`; none for `list-style: none`.
pub(crate) fn marker(n: i32, style: &ListStyle) -> Option<String> {
    let shown = format(n, style);
    match style {
        ListStyle::None => None,
        ListStyle::Symbol(_) | ListStyle::Text(_) => Some(shown),
        ListStyle::CjkInformal | ListStyle::CjkDecimal => Some(shown + "、"),
        _ => Some(shown + "."),
    }
}

fn digits(n: i32, digit: impl Fn(u32) -> Option<char>) -> String {
    let shown: Option<String> = n
        .unsigned_abs()
        .to_string()
        .chars()
        .map(|c| digit(c.to_digit(10)?))
        .collect();
    match shown {
        Some(s) if n < 0 => format!("-{s}"),
        Some(s) => s,
        None => n.to_string(),
    }
}

fn alphabetic(mut n: u32, letters: &str) -> String {
    let letters: Vec<char> = letters.chars().collect();
    let base = u32::try_from(letters.len()).unwrap_or(1).max(1);
    let mut out: Vec<char> = Vec::new();
    while n > 0 {
        n -= 1;
        out.extend(letters.get((n % base) as usize));
        n /= base;
    }
    out.iter().rev().collect()
}

fn additive(mut n: u32, table: &[(u32, &str)], lower: bool) -> String {
    let mut out = String::new();
    for &(value, symbol) in table {
        while n >= value && value > 0 {
            out.push_str(symbol);
            n -= value;
        }
    }
    if lower { out.to_lowercase() } else { out }
}

/// 105 as 一百零五, 15 as 十五.
fn cjk_informal(n: u32) -> String {
    const UNITS: [&str; 4] = ["千", "百", "十", ""];
    let places = [n / 1000 % 10, n / 100 % 10, n / 10 % 10, n % 10];
    let (mut out, mut zero) = (String::new(), false);
    for (i, d) in places.into_iter().enumerate() {
        if d == 0 {
            zero |= !out.is_empty();
            continue;
        }
        if std::mem::take(&mut zero) {
            out.push('零');
        }
        // Ten to nineteen drop the leading one.
        if !(d == 1 && i == 2 && out.is_empty()) {
            out.extend(CJK_DIGITS.get(d as usize));
        }
        out.push_str(UNITS.get(i).copied().unwrap_or_default());
    }
    out
}

/// A quoted CSS string at the start of `s`, unescaped, and what follows.
pub(crate) fn string(s: &str) -> Option<(String, &str)> {
    let quote = s.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let mut out = String::new();
    let mut chars = s.char_indices().skip(1).peekable();
    while let Some((i, c)) = chars.next() {
        if c == quote {
            return Some((out, s.get(i + c.len_utf8()..).unwrap_or_default()));
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        let mut hex = String::new();
        while let Some(&(_, h)) = chars.peek() {
            if hex.len() >= 6 || !h.is_ascii_hexdigit() {
                break;
            }
            hex.push(h);
            chars.next();
        }
        if hex.is_empty() {
            // A backslash before a newline continues the line.
            match chars.next() {
                Some((_, '\n')) | None => {}
                Some((_, other)) => out.push(other),
            }
            continue;
        }
        if chars
            .peek()
            .is_some_and(|&(_, w)| w == ' ' || w == '\n' || w == '\t')
        {
            chars.next();
        }
        let code = u32::from_str_radix(&hex, 16).unwrap_or(0xfffd);
        out.push(
            char::from_u32(code)
                .filter(|&c| c != '\0')
                .unwrap_or('\u{fffd}'),
        );
    }
    Some((out, ""))
}

/// The end of a function's arguments: its `)`, outside quotes.
fn close_paren(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, ')') => return Some(i),
            _ => {}
        }
    }
    None
}

/// A function's arguments split at top-level commas, trimmed.
fn arguments(args: &str) -> Vec<&str> {
    args.split(',').map(str::trim).collect()
}

/// A `content` value, its case kept; unknown parts are skipped.
pub(crate) fn content(value: &str) -> Option<Content> {
    let value = value.trim();
    match value.to_ascii_lowercase().as_str() {
        "" => return None,
        "none" | "normal" => return Some(Content::None),
        _ => {}
    }
    let mut items = Vec::new();
    let mut rest = value;
    while let Some(c) = rest.chars().next() {
        if items.len() >= MAX_ITEMS || c == '/' {
            break;
        }
        if c == '"' || c == '\'' {
            let (text, after) = string(rest)?;
            items.push(Item::Text(text));
            rest = after.trim_start();
            continue;
        }
        let end = rest
            .find(|w: char| w.is_whitespace() || matches!(w, '"' | '\'' | '(' | '/'))
            .unwrap_or(rest.len());
        let (word, after) = rest.split_at(end);
        let word = word.to_ascii_lowercase();
        match after.strip_prefix('(') {
            Some(inner) => {
                let close = close_paren(inner)?;
                items.extend(function(&word, inner.get(..close)?));
                rest = inner.get(close + 1..)?.trim_start();
            }
            None => {
                items.extend(match word.as_str() {
                    "open-quote" => Some(Item::OpenQuote),
                    "close-quote" => Some(Item::CloseQuote),
                    "no-open-quote" => Some(Item::NoOpenQuote),
                    "no-close-quote" => Some(Item::NoCloseQuote),
                    _ => None,
                });
                rest = after.trim_start();
            }
        }
    }
    Some(Content::Items(items.into()))
}

fn function(name: &str, args: &str) -> Option<Item> {
    let style = |s: Option<&&str>| s.and_then(|s| list_style(s)).unwrap_or_default();
    match name {
        "attr" => {
            let name = args.split_ascii_whitespace().next()?;
            Some(Item::Attr(name.to_ascii_lowercase()))
        }
        "counter" => {
            let args = arguments(args);
            let name = args.first().filter(|n| !n.is_empty())?;
            Some(Item::Counter((*name).to_owned(), style(args.get(1))))
        }
        "counters" => {
            let args = arguments(args);
            let name = args.first().filter(|n| !n.is_empty())?;
            let joint = args.get(1).and_then(|s| string(s)).map(|s| s.0)?;
            Some(Item::Counters(
                (*name).to_owned(),
                joint,
                style(args.get(2)),
            ))
        }
        "url" => {
            let raw = match string(args.trim()) {
                Some((s, _)) => s,
                None => args.trim().to_owned(),
            };
            (!raw.is_empty()).then_some(Item::Url(raw))
        }
        _ => None,
    }
}

/// A `counter-reset`, `-increment` or `-set` list; bare names take `default`.
pub(crate) fn counters(value: &str, default: i32) -> Option<Arc<[(String, i32)]>> {
    let words: Vec<&str> = value.split_ascii_whitespace().collect();
    if let [only] = words.as_slice()
        && only.eq_ignore_ascii_case("none")
    {
        return Some(Arc::from([]));
    }
    let mut out: Vec<(String, i32)> = Vec::new();
    let mut words = words.into_iter().peekable();
    while let Some(name) = words.next() {
        if name.parse::<i32>().is_ok() || out.len() >= MAX_COUNTERS {
            return None;
        }
        let n = match words.peek().and_then(|w| w.parse::<i32>().ok()) {
            Some(n) => {
                words.next();
                n
            }
            None => default,
        };
        out.push((name.to_owned(), n));
    }
    (!out.is_empty()).then(|| out.into())
}

/// Quote marks by depth when `quotes` says nothing.
pub(crate) fn default_quotes() -> Arc<[(String, String)]> {
    Arc::from([
        ("\u{201c}".to_owned(), "\u{201d}".to_owned()),
        ("\u{2018}".to_owned(), "\u{2019}".to_owned()),
    ])
}

/// A `quotes` value: pairs of strings, none, or the default's.
pub(crate) fn quotes(value: &str) -> Option<Arc<[(String, String)]>> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => return Some(Arc::from([])),
        "auto" => return Some(default_quotes()),
        _ => {}
    }
    let mut marks: Vec<String> = Vec::new();
    let mut rest = value.trim();
    while !rest.is_empty() {
        let (s, after) = string(rest)?;
        marks.push(s);
        rest = after.trim_start();
    }
    let (pairs, _) = marks.as_chunks::<2>();
    let pairs: Vec<(String, String)> = (pairs.iter())
        .map(|[open, close]| (open.clone(), close.clone()))
        .collect();
    (!pairs.is_empty()).then(|| pairs.into())
}

/// The first `url(…)` in `value`, its case kept; `data:` URLs too.
pub(crate) fn url(value: &str) -> Option<String> {
    let start = value.to_ascii_lowercase().find("url(")? + 4;
    let inner = value.get(start..)?;
    let inner = inner.get(..close_paren(inner)?)?.trim();
    let raw = string(inner).map_or_else(|| inner.to_owned(), |s| s.0);
    (!raw.is_empty()).then_some(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(value: &str) -> Vec<Item> {
        match content(value) {
            Some(Content::Items(items)) => items.to_vec(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn content_reads_strings_escapes_and_functions() {
        let got = items(r#""\201C Chapter " counter(chapter, upper-roman) attr(title) open-quote"#);
        let roman = ListStyle::Additive(ROMAN, false);
        assert_eq!(
            got,
            [
                Item::Text("\u{201c}Chapter ".into()),
                Item::Counter("chapter".into(), roman),
                Item::Attr("title".into()),
                Item::OpenQuote,
            ]
        );
        let got = items(r#"counters(item, ".") url("../img/orn.png") / "alt""#);
        assert_eq!(
            got,
            [
                Item::Counters("item".into(), ".".into(), ListStyle::Decimal),
                Item::Url("../img/orn.png".into()),
            ]
        );
        assert_eq!(content("none"), Some(Content::None));
        assert_eq!(items("'\\2766'"), [Item::Text("❦".into())]);
    }

    #[test]
    fn counters_quotes_and_urls_parse() {
        let reset = counters("chapter section 3", 0).unwrap_or_else(|| Arc::from([]));
        assert_eq!(*reset, [("chapter".into(), 0), ("section".into(), 3)]);
        assert_eq!(counters("none", 1).map(|c| c.len()), Some(0));
        let q = quotes(r#""«" "»" "‹" "›""#).unwrap_or_else(default_quotes);
        assert_eq!(q.first().map(|p| p.0.as_str()), Some("«"));
        assert_eq!(q.len(), 2);
        assert_eq!(url("url( 'a b.png' )"), Some("a b.png".into()));
    }

    #[test]
    fn every_list_style_counts_its_own_way() {
        let shown = |word: &str, n: i32| format(n, &list_style(word).unwrap_or(ListStyle::None));
        assert_eq!(shown("lower-roman", 1994), "mcmxciv");
        assert_eq!(shown("upper-alpha", 28), "AB");
        assert_eq!(shown("lower-greek", 3), "γ");
        assert_eq!(shown("decimal-leading-zero", 7), "07");
        assert_eq!(shown("myanmar", 42), "၄၂");
        assert_eq!(shown("cjk-ideographic", 105), "一百零五");
        assert_eq!(shown("cjk-ideographic", 15), "十五");
        assert_eq!(shown("hebrew", 15), "טו");
        assert_eq!(shown("upper-armenian", 2), "Բ");
        assert_eq!(shown("lower-alpha", 0), "0");
        assert_eq!(shown("\"→\"", 9), "→");
        let decimal = ListStyle::Decimal;
        assert_eq!(marker(3, &decimal), Some("3.".into()));
        assert_eq!(marker(3, &ListStyle::Symbol('◦')), Some("◦".into()));
        assert_eq!(marker(3, &ListStyle::None), None);
    }
}
