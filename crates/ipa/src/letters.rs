//! A word as lowercase letters, accents joined to the letter they mark.

/// `word` lowercased, each accent typed apart joined to its letter.
pub(crate) fn letters(word: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::with_capacity(word.len());
    for ch in word.chars().flat_map(char::to_lowercase) {
        if ('\u{300}'..='\u{36f}').contains(&ch) {
            let joined = out.last().and_then(|&base| join(base, ch));
            if let (Some(joined), Some(last)) = (joined, out.last_mut()) {
                *last = joined;
            }
            continue;
        }
        out.push(if ch == '’' { '\'' } else { ch });
    }
    out
}

/// The letter `base` with the accent `mark`, where one is written.
fn join(base: char, mark: char) -> Option<char> {
    let row = match mark {
        '\u{301}' => "áéíóú",
        '\u{300}' => "àèìòù",
        '\u{308}' => "äëïöü",
        '\u{303}' if base == 'n' => return Some('ñ'),
        '\u{303}' => "ãẽĩõũ",
        '\u{302}' => "âêîôû",
        _ => return None,
    };
    row.chars().nth("aeiou".find(base)?)
}

/// A number as written: whole part, and the digits after the comma.
pub(crate) struct Written<'a> {
    /// None when too big or zero-led: then read digit by digit.
    pub whole: Option<u64>,
    pub fraction: Option<&'a str>,
}

/// `text` read with `.` grouping thousands and `,` before a fraction.
pub(crate) fn number(text: &str) -> Written<'_> {
    let (whole, fraction) = match text.split_once(',') {
        Some((whole, fraction)) if !fraction.contains([',', '.']) => (whole, Some(fraction)),
        Some(_) => (text, None),
        None => match text.split_once('.') {
            Some((head, rest)) if !grouped(head, rest) => (head, Some(rest)),
            _ => (text, None),
        },
    };
    let digits: String = whole.chars().filter(char::is_ascii_digit).collect();
    let leading = digits.len() > 1 && digits.starts_with('0');
    let ok = grouped_or_plain(whole) && !leading;
    let value = digits.parse::<u64>().ok().filter(|_| ok);
    Written {
        whole: value.filter(|v| *v < 1_000_000_000_000),
        fraction,
    }
}

fn grouped_or_plain(whole: &str) -> bool {
    match whole.split_once('.') {
        Some((head, rest)) => grouped(head, rest),
        None => !whole.contains(','),
    }
}

/// Whether `head.rest` groups thousands, as `1.000.000`.
fn grouped(head: &str, rest: &str) -> bool {
    head.len() <= 3 && rest.split('.').all(|group| group.len() == 3)
}
