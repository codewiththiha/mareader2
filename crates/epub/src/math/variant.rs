//! `mathvariant`: letters and digits as Unicode's math alphabets.

/// A `mathvariant` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variant {
    Normal,
    Bold,
    Italic,
    BoldItalic,
    DoubleStruck,
    BoldFraktur,
    Script,
    BoldScript,
    Fraktur,
    Sans,
    BoldSans,
    SansItalic,
    SansBoldItalic,
    Monospace,
}

impl Variant {
    pub(crate) fn parse(v: &str) -> Option<Self> {
        Some(match v.trim().to_ascii_lowercase().as_str() {
            "normal" => Self::Normal,
            "bold" => Self::Bold,
            "italic" => Self::Italic,
            "bold-italic" => Self::BoldItalic,
            "double-struck" => Self::DoubleStruck,
            "bold-fraktur" => Self::BoldFraktur,
            "script" => Self::Script,
            "bold-script" => Self::BoldScript,
            "fraktur" => Self::Fraktur,
            "sans-serif" => Self::Sans,
            "bold-sans-serif" => Self::BoldSans,
            "sans-serif-italic" => Self::SansItalic,
            "sans-serif-bold-italic" => Self::SansBoldItalic,
            "monospace" => Self::Monospace,
            _ => return None,
        })
    }

    /// Where its Latin capital A sits.
    fn latin(self) -> Option<u32> {
        Some(match self {
            Self::Normal => return None,
            Self::Bold => 0x1D400,
            Self::Italic => 0x1D434,
            Self::BoldItalic => 0x1D468,
            Self::Script => 0x1D49C,
            Self::BoldScript => 0x1D4D0,
            Self::Fraktur => 0x1D504,
            Self::DoubleStruck => 0x1D538,
            Self::BoldFraktur => 0x1D56C,
            Self::Sans => 0x1D5A0,
            Self::BoldSans => 0x1D5D4,
            Self::SansItalic => 0x1D608,
            Self::SansBoldItalic => 0x1D63C,
            Self::Monospace => 0x1D670,
        })
    }

    /// Where its Greek capital Alpha sits, for the five with Greek.
    fn greek(self) -> Option<u32> {
        match self {
            Self::Bold => Some(0x1D6A8),
            Self::Italic => Some(0x1D6E2),
            Self::BoldItalic => Some(0x1D71C),
            Self::BoldSans => Some(0x1D756),
            Self::SansBoldItalic => Some(0x1D790),
            _ => None,
        }
    }

    fn digit(self) -> Option<u32> {
        match self {
            Self::Bold => Some(0x1D7CE),
            Self::DoubleStruck => Some(0x1D7D8),
            Self::Sans => Some(0x1D7E2),
            Self::BoldSans => Some(0x1D7EC),
            Self::Monospace => Some(0x1D7F6),
            _ => None,
        }
    }

    /// `c` in this alphabet, or `c` itself if it has no such letter.
    pub(crate) fn map(self, c: char) -> char {
        if let Some(hole) = hole(self, c) {
            return hole;
        }
        let code = u32::from(c);
        let at = match c {
            'A'..='Z' => self.latin().map(|b| b + code - 0x41),
            'a'..='z' => self.latin().map(|b| b + 26 + code - 0x61),
            '0'..='9' => self.digit().map(|b| b + code - 0x30),
            'ı' if self == Self::Italic => Some(0x1D6A4),
            'ȷ' if self == Self::Italic => Some(0x1D6A5),
            _ => greek_offset(c).and_then(|o| self.greek().map(|b| b + o)),
        };
        at.and_then(char::from_u32).unwrap_or(c)
    }
}

/// A Greek letter's place in a math Greek alphabet.
fn greek_offset(c: char) -> Option<u32> {
    let code = u32::from(c);
    Some(match c {
        // U+03A2 is unassigned; its slot holds the theta symbol.
        'Α'..='Ω' if code != 0x3A2 => code - 0x391,
        'ϴ' => 17,
        '∇' => 25,
        'α'..='ω' => 26 + code - 0x3B1,
        '∂' => 51,
        'ϵ' => 52,
        'ϑ' => 53,
        'ϰ' => 54,
        'ϕ' => 55,
        'ϱ' => 56,
        'ϖ' => 57,
        _ => return None,
    })
}

/// Letters Unicode had already, so the math alphabets skip them.
fn hole(v: Variant, c: char) -> Option<char> {
    Some(match (v, c) {
        (Variant::Italic, 'h') => 'ℎ',
        (Variant::Script, 'B') => 'ℬ',
        (Variant::Script, 'E') => 'ℰ',
        (Variant::Script, 'F') => 'ℱ',
        (Variant::Script, 'H') => 'ℋ',
        (Variant::Script, 'I') => 'ℐ',
        (Variant::Script, 'L') => 'ℒ',
        (Variant::Script, 'M') => 'ℳ',
        (Variant::Script, 'R') => 'ℛ',
        (Variant::Script, 'e') => 'ℯ',
        (Variant::Script, 'g') => 'ℊ',
        (Variant::Script, 'o') => 'ℴ',
        (Variant::Fraktur, 'C') => 'ℭ',
        (Variant::Fraktur, 'H') => 'ℌ',
        (Variant::Fraktur, 'I') => 'ℑ',
        (Variant::Fraktur, 'R') => 'ℜ',
        (Variant::Fraktur, 'Z') => 'ℨ',
        (Variant::DoubleStruck, 'C') => 'ℂ',
        (Variant::DoubleStruck, 'H') => 'ℍ',
        (Variant::DoubleStruck, 'N') => 'ℕ',
        (Variant::DoubleStruck, 'P') => 'ℙ',
        (Variant::DoubleStruck, 'Q') => 'ℚ',
        (Variant::DoubleStruck, 'R') => 'ℝ',
        (Variant::DoubleStruck, 'Z') => 'ℤ',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_take_their_alphabet() {
        assert_eq!(Variant::Italic.map('x'), '𝑥');
        assert_eq!(Variant::Italic.map('h'), 'ℎ');
        assert_eq!(Variant::DoubleStruck.map('R'), 'ℝ');
        assert_eq!(Variant::DoubleStruck.map('A'), '𝔸');
        assert_eq!(Variant::Bold.map('A'), '𝐀');
        assert_eq!(Variant::Script.map('A'), '𝒜');
        assert_eq!(Variant::Italic.map('α'), '𝛼');
        assert_eq!(Variant::Bold.map('7'), '𝟕');
        assert_eq!(Variant::Italic.map('+'), '+');
        assert_eq!(Variant::Normal.map('x'), 'x');
    }
}
