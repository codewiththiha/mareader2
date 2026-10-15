//! Manual-order keys: base-62 fractions; another always fits between two.

/// The digits, in the order their bytes compare.
const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE: u16 = 62;

/// A key after `last`, short: appending bumps its final digit.
pub fn after(last: Option<&str>) -> String {
    let Some(mut digits) = last.and_then(parse) else {
        return between(None, None);
    };
    match digits.last_mut() {
        Some(d) if u16::from(*d) + 1 < BASE => *d += 1,
        _ => digits.push(1),
    }
    render(&digits)
}

/// A key strictly between `a` and `b`; none is either end.
pub fn between(a: Option<&str>, b: Option<&str>) -> String {
    let a = a.and_then(parse).unwrap_or_default();
    match b.and_then(parse) {
        Some(b) if a < b => render(&midpoint(&a, Some(&b))),
        // A key out of order, as a merge can leave: go past `a`.
        Some(_) => after(Some(&render(&a))),
        None => render(&midpoint(&a, None)),
    }
}

/// `n` keys in order strictly between `a` and `b`, short for any `n`.
pub fn spread(a: Option<&str>, b: Option<&str>, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    // The middle one first, then each half: lengths grow with log n.
    let mid = between(a, b);
    let half = n / 2;
    let mut out = spread(a, Some(&mid), half);
    let right = spread(Some(&mid), b, n - half - 1);
    out.push(mid);
    out.extend(right);
    out
}

/// Its digits, if it is a key this module could have made.
fn parse(key: &str) -> Option<Vec<u8>> {
    let digits: Option<Vec<u8>> = key.bytes().map(digit).collect();
    digits.filter(|d| d.last().is_some_and(|&last| last != 0))
}

fn digit(byte: u8) -> Option<u8> {
    let at = DIGITS.iter().position(|&d| d == byte)?;
    u8::try_from(at).ok()
}

fn render(digits: &[u8]) -> String {
    let byte = |&d: &u8| DIGITS.get(usize::from(d)).copied().map(char::from);
    digits.iter().filter_map(byte).collect()
}

/// The fraction between `a` (missing digits are 0) and `b` (none is 1).
fn midpoint(a: &[u8], b: Option<&[u8]>) -> Vec<u8> {
    if let Some(b) = b {
        let shared = b
            .iter()
            .enumerate()
            .take_while(|&(i, &d)| a.get(i).copied().unwrap_or(0) == d)
            .count();
        if shared > 0 {
            let mut out: Vec<u8> = b.iter().take(shared).copied().collect();
            let rest = |s: &[u8]| s.get(shared..).unwrap_or_default().to_vec();
            out.extend(midpoint(&rest(a), Some(&rest(b))));
            return out;
        }
    }
    let low = u16::from(a.first().copied().unwrap_or(0));
    let high = b.map_or(BASE, |b| u16::from(b.first().copied().unwrap_or(0)));
    if high.saturating_sub(low) > 1 {
        let mid = (low + high).div_ceil(2);
        return vec![u8::try_from(mid).unwrap_or(1)];
    }
    // Neighbouring digits: `b`'s first alone fits if `b` goes on.
    if let Some(&first) = b.filter(|b| b.len() > 1).and_then(|b| b.first()) {
        return vec![first];
    }
    let mut out = vec![u8::try_from(low).unwrap_or(0)];
    out.extend(midpoint(a.get(1..).unwrap_or_default(), None));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_fit_between_any_two_and_stay_short_when_appended() {
        assert_eq!(between(None, None), "V");
        assert_eq!(after(Some("V")), "W");
        assert_eq!(after(Some("z")), "z1");
        assert_eq!(between(Some("V"), Some("W")), "VV");
        let mut keys = vec![after(None)];
        for _ in 0..1_000 {
            let next = after(keys.last().map(String::as_str));
            keys.push(next);
        }
        assert!(keys.windows(2).all(|w| w.first() < w.last()));
        assert!(keys.iter().all(|k| k.len() <= 20), "{:?}", keys.last());
    }

    #[test]
    fn inserting_anywhere_keeps_every_key_in_order_and_valid() {
        let mut keys: Vec<String> = Vec::new();
        let mut seed = 0x9e37_79b9_u64;
        for _ in 0..2_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let at = usize::try_from(seed % (keys.len() as u64 + 1)).unwrap_or(0);
            let before = at.checked_sub(1).and_then(|i| keys.get(i));
            let key = between(before.map(String::as_str), keys.get(at).map(String::as_str));
            keys.insert(at, key);
        }
        assert!(keys.windows(2).all(|w| w.first() < w.last()));
        assert!(keys.iter().all(|k| parse(k).is_some()));
    }

    #[test]
    fn spread_keys_are_ordered_short_and_inside_their_ends() {
        let keys = spread(Some("A"), Some("B"), 5_000);
        assert_eq!(keys.len(), 5_000);
        assert!(keys.windows(2).all(|w| w.first() < w.last()));
        assert!(keys.iter().all(|k| k.as_str() > "A" && k.as_str() < "B"));
        assert!(keys.iter().all(|k| k.len() <= 16), "{:?}", keys.last());
        assert!(spread(None, None, 0).is_empty());
    }

    #[test]
    fn a_key_out_of_order_or_malformed_still_gives_a_usable_one() {
        assert!(between(Some("W"), Some("V")).as_str() > "W");
        assert!(between(Some("W"), Some("W")).as_str() > "W");
        assert_eq!(after(Some("bad key!")), "V");
        assert_eq!(after(Some("V0")), "V");
    }
}
