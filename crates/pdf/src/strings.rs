//! PDF text strings, as titles, authors and chapters are written.

/// PDFDocEncoding where it parts from Latin-1 (PDF 2.0, Annex D).
const PDF_DOC: [(u8, char); 40] = [
    (0x18, '\u{02D8}'),
    (0x19, '\u{02C7}'),
    (0x1A, '\u{02C6}'),
    (0x1B, '\u{02D9}'),
    (0x1C, '\u{02DD}'),
    (0x1D, '\u{02DB}'),
    (0x1E, '\u{02DA}'),
    (0x1F, '\u{02DC}'),
    (0x80, '\u{2022}'),
    (0x81, '\u{2020}'),
    (0x82, '\u{2021}'),
    (0x83, '\u{2026}'),
    (0x84, '\u{2014}'),
    (0x85, '\u{2013}'),
    (0x86, '\u{0192}'),
    (0x87, '\u{2044}'),
    (0x88, '\u{2039}'),
    (0x89, '\u{203A}'),
    (0x8A, '\u{2212}'),
    (0x8B, '\u{2030}'),
    (0x8C, '\u{201E}'),
    (0x8D, '\u{201C}'),
    (0x8E, '\u{201D}'),
    (0x8F, '\u{2018}'),
    (0x90, '\u{2019}'),
    (0x91, '\u{201A}'),
    (0x92, '\u{2122}'),
    (0x93, '\u{FB01}'),
    (0x94, '\u{FB02}'),
    (0x95, '\u{0141}'),
    (0x96, '\u{0152}'),
    (0x97, '\u{0160}'),
    (0x98, '\u{0178}'),
    (0x99, '\u{017D}'),
    (0x9A, '\u{0131}'),
    (0x9B, '\u{0142}'),
    (0x9C, '\u{0153}'),
    (0x9D, '\u{0161}'),
    (0x9E, '\u{017E}'),
    (0xA0, '\u{20AC}'),
];

/// A PDF text string as Unicode: a BOM, bare UTF-8, else PDFDocEncoding.
pub(crate) fn decode_text(bytes: &[u8]) -> String {
    let text = match bytes {
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        // Many writers skip the BOM; Latin text rarely forms valid UTF-8.
        _ => match std::str::from_utf8(bytes) {
            Ok(s) if !bytes.iter().any(|b| (0x18..=0x1F).contains(b)) => s.to_owned(),
            _ => bytes.iter().map(|&b| pdf_doc(b)).collect(),
        },
    };
    clean(&text)
}

fn pdf_doc(b: u8) -> char {
    let mapped = PDF_DOC.iter().find(|(k, _)| *k == b).map(|(_, c)| *c);
    mapped.unwrap_or(char::from(b))
}

fn utf16(bytes: &[u8], word: fn([u8; 2]) -> u16) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|c| word(*c));
    char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// Breaks become spaces; other control characters and stray BOMs go.
fn clean(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\t' | '\n' | '\r' => Some(' '),
            '\u{FEFF}' => None,
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_strings_decode_from_each_encoding() {
        assert_eq!(decode_text(b"Plain"), "Plain");
        assert_eq!(decode_text(&[0xFE, 0xFF, 0, b'H', 0, b'i']), "Hi");
        assert_eq!(decode_text(&[0xFF, 0xFE, b'H', 0, b'i', 0]), "Hi");
        assert_eq!(decode_text(&[0xFE, 0xFF, 0xD8, 0x00]), "\u{FFFD}");
        assert_eq!(decode_text(&[0xE9]), "é");
    }

    #[test]
    fn utf8_without_a_bom_reads_as_utf8() {
        assert_eq!(
            decode_text("日本語のタイトル".as_bytes()),
            "日本語のタイトル"
        );
        assert_eq!(decode_text("Café Müller".as_bytes()), "Café Müller");
    }

    #[test]
    fn pdf_doc_encoding_is_not_latin1() {
        // "The ﬁrst — “quoted” • €5"
        let raw = b"The \x93rst \x84 \x8Dquoted\x8E \x80 \xA05";
        assert_eq!(decode_text(raw), "The \u{FB01}rst — “quoted” • €5");
        assert_eq!(decode_text(b"Ole\xF8"), "Oleø");
    }

    #[test]
    fn breaks_and_control_characters_are_tidied() {
        assert_eq!(decode_text(b"Part\r\nOne\x00\x07"), "Part  One");
        assert_eq!(decode_text(&[0xFE, 0xFF, 0xFE, 0xFF, 0, b'A']), "A");
        // A PDFDocEncoding accent is a character, not a control.
        assert_eq!(decode_text(b"a\x18"), "a\u{02D8}");
    }
}
