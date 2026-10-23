//! Number words, string for string as num2words 0.5.14 writes English.

const ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];
const SCALES: [&str; 7] = [
    "",
    "thousand",
    "million",
    "billion",
    "trillion",
    "quadrillion",
    "quintillion",
];

fn ones(n: u64) -> &'static str {
    usize::try_from(n)
        .ok()
        .and_then(|i| ONES.get(i))
        .copied()
        .unwrap_or_default()
}

fn tens(n: u64) -> &'static str {
    usize::try_from(n)
        .ok()
        .and_then(|i| TENS.get(i))
        .copied()
        .unwrap_or_default()
}

/// 1234 is "one thousand, two hundred and thirty-four".
pub(crate) fn cardinal(n: u64) -> String {
    if n == 0 {
        return ones(0).to_owned();
    }
    let mut groups = Vec::new();
    let mut rest = n;
    let mut scale = 0;
    while rest > 0 {
        let group = rest % 1000;
        if group != 0 {
            groups.push((group, scale));
        }
        rest /= 1000;
        scale += 1;
    }
    groups.reverse();
    let last = groups.len().saturating_sub(1);
    let mut out = String::new();
    for (i, &(group, scale)) in groups.iter().enumerate() {
        if i > 0 {
            let and = i == last && scale == 0 && group < 100;
            out.push_str(if and { " and " } else { ", " });
        }
        out.push_str(&below_thousand(group));
        if scale > 0 {
            out.push(' ');
            out.push_str(SCALES.get(scale).copied().unwrap_or_default());
        }
    }
    out
}

fn below_thousand(n: u64) -> String {
    match (n / 100, n % 100) {
        (0, rest) => below_hundred(rest),
        (hundreds, 0) => format!("{} hundred", ones(hundreds)),
        (hundreds, rest) => format!("{} hundred and {}", ones(hundreds), below_hundred(rest)),
    }
}

fn below_hundred(n: u64) -> String {
    match n {
        0..=19 => ones(n).to_owned(),
        _ if n.is_multiple_of(10) => tens(n / 10).to_owned(),
        _ => format!("{}-{}", tens(n / 10), ones(n % 10)),
    }
}

/// 21 is "twenty-first", 100 "one hundredth".
pub(crate) fn ordinal(n: u64) -> String {
    let words = cardinal(n);
    let cut = words.rfind([' ', '-']).map_or(0, |i| i + 1);
    let (head, last) = words.split_at_checked(cut).unwrap_or(("", &words));
    let last = match last {
        "one" => "first".to_owned(),
        "two" => "second".to_owned(),
        "three" => "third".to_owned(),
        "five" => "fifth".to_owned(),
        "eight" => "eighth".to_owned(),
        "nine" => "ninth".to_owned(),
        "twelve" => "twelfth".to_owned(),
        word => match word.strip_suffix('y') {
            Some(stem) => format!("{stem}ieth"),
            None => format!("{word}th"),
        },
    };
    format!("{head}{last}")
}

/// 1905 is "nineteen oh-five" and 2010 "twenty ten", but 2005 is a cardinal.
pub(crate) fn year(n: u64) -> String {
    let (high, low) = (n / 100, n % 100);
    if high == 0 || (high.is_multiple_of(10) && low < 10) || high >= 100 {
        return cardinal(n);
    }
    let low = match low {
        0 => "hundred".to_owned(),
        1..=9 => format!("oh-{}", cardinal(low)),
        _ => cardinal(low),
    };
    format!("{} {low}", cardinal(high))
}

/// `"4.70"` is "four point seven", trailing zeros gone as a float prints.
pub(crate) fn decimal(literal: &str) -> Option<String> {
    let (whole, fraction) = literal.split_once('.').unwrap_or((literal, ""));
    let whole = if whole.is_empty() { "0" } else { whole };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || !digits(fraction) {
        return None;
    }
    let mut out = cardinal(whole.parse().ok()?);
    let fraction = fraction.trim_end_matches('0');
    if !fraction.is_empty() {
        out.push_str(" point");
        for digit in fraction.bytes() {
            out.push(' ');
            out.push_str(ones(u64::from(digit - b'0')));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cardinals_join_groups_with_commas_and_a_final_and() {
        assert_eq!(cardinal(0), "zero");
        assert_eq!(cardinal(40), "forty");
        assert_eq!(cardinal(115), "one hundred and fifteen");
        assert_eq!(cardinal(1234), "one thousand, two hundred and thirty-four");
        assert_eq!(cardinal(1005), "one thousand and five");
        assert_eq!(cardinal(2_000_100), "two million, one hundred");
        assert_eq!(
            cardinal(u64::MAX),
            "eighteen quintillion, four hundred and forty-six quadrillion, \
             seven hundred and forty-four trillion, seventy-three billion, \
             seven hundred and nine million, five hundred and fifty-one \
             thousand, six hundred and fifteen"
        );
    }

    #[test]
    fn ordinals_change_the_last_word() {
        assert_eq!(ordinal(1), "first");
        assert_eq!(ordinal(21), "twenty-first");
        assert_eq!(ordinal(40), "fortieth");
        assert_eq!(ordinal(112), "one hundred and twelfth");
        assert_eq!(ordinal(100), "one hundredth");
    }

    #[test]
    fn years_read_in_pairs_except_round_thousands() {
        assert_eq!(year(1905), "nineteen oh-five");
        assert_eq!(year(1900), "nineteen hundred");
        assert_eq!(year(2010), "twenty ten");
        assert_eq!(year(2005), "two thousand and five");
        assert_eq!(year(999), "nine ninety-nine");
        assert_eq!(year(50), "fifty");
    }

    #[test]
    fn decimals_read_their_fraction_digit_by_digit() {
        assert_eq!(decimal("4.70").as_deref(), Some("four point seven"));
        assert_eq!(decimal("5.0").as_deref(), Some("five"));
        assert_eq!(decimal(".5").as_deref(), Some("zero point five"));
        assert_eq!(decimal("1.2.3"), None);
        assert_eq!(decimal("12a"), None);
    }
}
