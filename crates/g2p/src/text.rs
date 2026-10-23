//! Python's string semantics, which misaki's rules lean on, and its stress.

pub(crate) const PRIMARY: char = 'ˈ';
pub(crate) const SECONDARY: char = 'ˌ';
pub(crate) const VOWELS: &str = "AIOQWYaiuæɑɒɔəɛɜɪʊʌᵻ";
const DIPHTHONGS: &str = "AIOQWYʤʧ";

/// `&s[range]`, empty where the range is not on character boundaries.
pub(crate) fn slice<R: std::slice::SliceIndex<str, Output = str>>(s: &str, range: R) -> &str {
    s.get(range).unwrap_or_default()
}

/// Length in characters, as Python counts.
pub(crate) fn len(s: &str) -> usize {
    s.chars().count()
}

/// `s[:-n]`, by characters.
pub(crate) fn drop_last(s: &str, n: usize) -> &str {
    let keep = len(s).saturating_sub(n);
    match s.char_indices().nth(keep) {
        Some((i, _)) => s.get(..i).unwrap_or(s),
        None => s,
    }
}

/// `s[1:]`, by characters.
pub(crate) fn tail(s: &str) -> &str {
    match s.char_indices().nth(1) {
        Some((i, _)) => s.get(i..).unwrap_or_default(),
        None => "",
    }
}

pub(crate) fn last_char(s: &str) -> Option<char> {
    s.chars().next_back()
}

pub(crate) fn is_lower(s: &str) -> bool {
    s == s.to_lowercase()
}

pub(crate) fn is_upper(s: &str) -> bool {
    s == s.to_uppercase()
}

/// `str.isalpha`: not empty, letters only.
pub(crate) fn isalpha(s: &str) -> bool {
    !s.is_empty() && s.chars().all(char::is_alphabetic)
}

/// `str.capitalize`: the first character upper, the rest lower.
pub(crate) fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

/// misaki's `is_digit`: ASCII digits, at least one.
pub(crate) fn is_digit(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

pub(crate) fn stress_weight(ps: &str) -> usize {
    ps.chars()
        .map(|c| if DIPHTHONGS.contains(c) { 2 } else { 1 })
        .sum()
}

fn has_stress(ps: &str) -> bool {
    ps.contains(PRIMARY) || ps.contains(SECONDARY)
}

fn has_vowel(ps: &str) -> bool {
    ps.chars().any(|c| VOWELS.contains(c))
}

/// `restress(mark + ps)`: the mark lands just before the first vowel.
fn stress_first_vowel(mark: char, ps: &str) -> String {
    let at = ps
        .char_indices()
        .find(|&(_, c)| VOWELS.contains(c))
        .map_or(0, |(i, _)| i);
    let (head, rest) = ps.split_at_checked(at).unwrap_or(("", ps));
    format!("{head}{mark}{rest}")
}

pub(crate) fn apply_stress(ps: &str, stress: Option<f64>) -> String {
    let Some(stress) = stress else {
        return ps.to_owned();
    };
    let demote = || ps.replace(SECONDARY, "").replace(PRIMARY, "ˌ");
    if stress < -1.0 {
        ps.replace([PRIMARY, SECONDARY], "")
    } else if stress == -1.0 || ((stress == 0.0 || stress == -0.5) && ps.contains(PRIMARY)) {
        demote()
    } else if (stress == 0.0 || stress == 0.5 || stress == 1.0) && !has_stress(ps) {
        match has_vowel(ps) {
            true => stress_first_vowel(SECONDARY, ps),
            false => ps.to_owned(),
        }
    } else if stress >= 1.0 && !ps.contains(PRIMARY) && ps.contains(SECONDARY) {
        ps.replace(SECONDARY, "ˈ")
    } else if stress > 1.0 && !has_stress(ps) {
        match has_vowel(ps) {
            true => stress_first_vowel(PRIMARY, ps),
            false => ps.to_owned(),
        }
    } else {
        ps.to_owned()
    }
}

pub(crate) fn apply_stress_opt(ps: Option<String>, stress: Option<f64>) -> Option<String> {
    ps.map(|p| apply_stress(&p, stress))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stress_follows_misaki() {
        assert_eq!(apply_stress("həlˈO", Some(-2.0)), "həlO");
        assert_eq!(apply_stress("həlˈO", Some(-1.0)), "həlˌO");
        assert_eq!(apply_stress("wʌn", Some(0.0)), "wˌʌn");
        assert_eq!(apply_stress("wʌn", Some(2.0)), "wˈʌn");
        assert_eq!(apply_stress("ˌaɪ", Some(1.0)), "ˈaɪ");
        assert_eq!(apply_stress("bd", Some(2.0)), "bd");
        assert_eq!(apply_stress("bd", None), "bd");
    }

    #[test]
    fn strings_behave_as_in_python() {
        assert_eq!(drop_last("café", 1), "caf");
        assert_eq!(drop_last("ab", 5), "");
        assert_eq!(tail("Word"), "ord");
        assert_eq!(tail(""), "");
        assert_eq!(capitalize("hELLO"), "Hello");
        assert!(isalpha("Straße") && !isalpha("don't") && !isalpha(""));
        assert!(is_upper("NASA") && is_upper("9") && !is_upper("Nasa"));
    }
}
