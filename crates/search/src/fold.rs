//! Folding text so a search ignores case, accents and quote styles.

/// Char for char, so ranges map back: lower case, no accents.
pub fn fold(c: char) -> char {
    let c = c.to_lowercase().next().unwrap_or(c);
    match c {
        'à'..='å' => 'a',
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ð' => 'd',
        'ñ' => 'n',
        'ò'..='ö' | 'ø' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        '\u{2018}' | '\u{2019}' | '\u{02bc}' => '\'',
        '\u{201c}' | '\u{201d}' => '"',
        '\u{2010}'..='\u{2015}' => '-',
        '\u{0100}'..='\u{017f}' => extended_a(c),
        _ => c,
    }
}

/// Latin Extended-A by base letter; `_` keeps the char (ligatures).
const EXTENDED_A: &[u8; 128] = b"aaaaaaccccccccddddeeeeeeeeeegggggggghhhhiiiiiiiiii\
__jjkkkllllllllllnnnnnnnnnoooooo__rrrrrrsssssssstttttt\
uuuuuuuuuuuuwwyyyzzzzzzs";

fn extended_a(c: char) -> char {
    let at = (c as usize).wrapping_sub(0x100);
    match EXTENDED_A.get(at) {
        Some(b'_') | None => c,
        Some(&base) => char::from(base),
    }
}

/// A typeset ligature's letters, as a reader would type them.
pub fn spell(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{fb00}' => "ff",
        '\u{fb01}' => "fi",
        '\u{fb02}' => "fl",
        '\u{fb03}' => "ffi",
        '\u{fb04}' => "ffl",
        '\u{fb05}' | '\u{fb06}' => "st",
        _ => return None,
    })
}

/// Characters that only steer layout: soft hyphens, zero widths, BOMs.
pub fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00ad}' | '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_accents_and_quotes_fold_away() {
        let folded: String = "Ÿes, Ł’Été “Ñ”".chars().map(fold).collect();
        assert_eq!(folded, "yes, l'ete \"n\"");
        assert_eq!(fold('Œ'), 'œ', "a ligature is kept, not half-spelled");
        assert_eq!(spell('\u{fb01}'), Some("fi"));
        assert!(invisible('\u{00ad}') && !invisible('-'));
    }
}
