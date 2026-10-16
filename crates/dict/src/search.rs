//! How tightly a word fits what was asked, and near misses.

/// Where a row matched, tightest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WordMatch {
    Exact,
    Prefix,
    Substring,
    /// Within two edits: a typo or an inflection away.
    Fuzzy,
}

/// The compare form: lowercase, one apostrophe, one inner space.
pub fn fold(word: &str) -> String {
    let lower: String = word
        .trim()
        .chars()
        .map(|c| match c {
            '\u{2019}' | '\u{2018}' => '\'',
            c => c,
        })
        .flat_map(char::to_lowercase)
        .collect();
    lower.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Levenshtein distance; past `cap` it answers `cap + 1`.
pub fn edit_distance(a: &str, b: &str, cap: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > cap {
        return cap + 1;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        let mut left = i + 1;
        cur.clear();
        cur.push(left);
        for (j, cb) in b.iter().enumerate() {
            let diag = prev.get(j).copied().unwrap_or(usize::MAX);
            let up = prev.get(j + 1).copied().unwrap_or(usize::MAX);
            left = (diag + usize::from(ca != cb)).min(up + 1).min(left + 1);
            cur.push(left);
        }
        std::mem::swap(&mut prev, &mut cur);
        if prev.iter().min().is_some_and(|m| *m > cap) {
            return cap + 1;
        }
    }
    prev.last().copied().unwrap_or(cap + 1).min(cap + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apostrophes_and_spaces_compare_as_one() {
        assert_eq!(fold("It\u{2019}s"), "it's");
        assert_eq!(fold("  ice \n cream "), "ice cream");
    }

    #[test]
    fn distance_gives_up_past_the_cap() {
        assert_eq!(edit_distance("run", "run", 2), 0);
        assert_eq!(edit_distance("run", "ran", 2), 1);
        assert_eq!(edit_distance("kitten", "sitting", 2), 3);
        assert_eq!(edit_distance("a", "abcdef", 2), 3);
        assert_eq!(edit_distance("", "ab", 2), 2);
    }
}
