//! Text cut into words, numbers and the punctuation kept between them.

/// What a token is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Piece<'a> {
    Word(&'a str),
    Number(&'a str),
    Mark(char),
    /// Dashes typed as hyphens: a pause with nothing said.
    Break,
}

/// A piece, and whether space came before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token<'a> {
    pub piece: Piece<'a>,
    pub space: bool,
    /// Joined to the word before by one hyphen, as `dá-lhe`.
    pub hyphen: bool,
}

/// Punctuation kept as it stands; it marks a pause.
const MARKS: &str = ";:,.!?¡¿—…\"«»“”(){}[]";

/// `text` in tokens; symbols that are not marks count as space.
pub(crate) fn split(text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut space = false;
    let mut hyphen = false;
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        let (piece, len) = if ch.is_alphabetic() {
            let len = word_len(rest);
            (Some(Piece::Word(head(rest, len))), len)
        } else if ch.is_ascii_digit() {
            let len = number_len(rest);
            (Some(Piece::Number(head(rest, len))), len)
        } else if MARKS.contains(ch) {
            (Some(Piece::Mark(ch)), ch.len_utf8())
        } else if let Some(len) = dash_len(rest) {
            (Some(Piece::Break), len)
        } else if ch == '-' && joins(&tokens, space, rest) {
            hyphen = true;
            (None, 1)
        } else {
            (None, ch.len_utf8())
        };
        match piece {
            Some(piece) => {
                let hyphen = std::mem::take(&mut hyphen);
                tokens.push(Token {
                    piece,
                    space,
                    hyphen,
                });
                space = false;
            }
            None => space = true,
        }
        rest = rest.get(len..).unwrap_or_default();
    }
    tokens
}

/// Whether the hyphen at `rest` runs straight from a word into another.
fn joins(tokens: &[Token<'_>], space: bool, rest: &str) -> bool {
    let after_word = matches!(tokens.last().map(|t| t.piece), Some(Piece::Word(_)));
    let next = rest.get(1..).and_then(|r| r.chars().next());
    after_word && !space && next.is_some_and(char::is_alphabetic)
}

/// Hyphens standing for a dash: two or more, or one with space round it.
fn dash_len(text: &str) -> Option<usize> {
    let len = text.len() - text.trim_start_matches(['-', '–', '―']).len();
    let lone = text.starts_with('-') && len == 1;
    let spaced = text
        .get(1..)
        .is_some_and(|r| r.starts_with(char::is_whitespace));
    (len > 0 && (!lone || spaced)).then_some(len)
}

fn head(text: &str, len: usize) -> &str {
    text.get(..len).unwrap_or(text)
}

/// Letters, with apostrophes only between them, as in `l'uomo`.
fn word_len(text: &str) -> usize {
    let mut len = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        let letter = ch.is_alphabetic() || is_mark(ch);
        let joins = matches!(ch, '\'' | '’')
            && len > 0
            && chars.peek().is_some_and(|(_, c)| c.is_alphabetic());
        if !letter && !joins {
            break;
        }
        len = at + ch.len_utf8();
    }
    len
}

/// Accents typed apart from their letter; Devanagari virama and nukta.
fn is_mark(ch: char) -> bool {
    ('\u{300}'..='\u{36f}').contains(&ch) || matches!(ch, '\u{93c}' | '\u{94d}')
}

/// Digits, with `.` or `,` only between them.
fn number_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut len = 0;
    while let Some(&b) = bytes.get(len) {
        let between =
            matches!(b, b'.' | b',') && bytes.get(len + 1).is_some_and(u8::is_ascii_digit);
        if !b.is_ascii_digit() && !between {
            break;
        }
        len += 1;
    }
    len
}
