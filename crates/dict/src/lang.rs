//! Languages by code, and a word's language read off its script.

/// The writing a character belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Script {
    Myanmar,
    Kana,
    Han,
    Latin,
}

/// Every language a pack joins: code, name and scripts.
const LANGUAGES: [(&str, &str, &[Script]); 4] = [
    ("en", "English", &[Script::Latin]),
    ("fr", "French", &[Script::Latin]),
    ("my", "Burmese", &[Script::Myanmar]),
    ("jp", "Japanese", &[Script::Kana, Script::Han]),
];

/// A language's English name; an unknown code as written, uppercased.
pub fn name(code: &str) -> String {
    LANGUAGES
        .iter()
        .find(|(c, ..)| *c == code)
        .map_or_else(|| code.to_uppercase(), |(_, n, _)| (*n).to_owned())
}

fn script_of(ch: char) -> Option<Script> {
    match u32::from(ch) {
        0x1000..=0x109F | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F => Some(Script::Myanmar),
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => Some(Script::Kana),
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF => Some(Script::Han),
        0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x24F => Some(Script::Latin),
        _ => None,
    }
}

/// The language `text`'s first script names; Latin names none.
pub fn detect(text: &str, langs: &[&str]) -> Option<&'static str> {
    let script = text.chars().find_map(script_of)?;
    if script == Script::Latin {
        return None;
    }
    let mut speakers = LANGUAGES
        .iter()
        .filter(|(code, _, scripts)| scripts.contains(&script) && langs.contains(code))
        .map(|(code, ..)| *code);
    let one = speakers.next()?;
    speakers.next().is_none().then_some(one)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_with_one_speaker_names_its_language() {
        let all = ["en", "my", "jp", "fr"];
        assert_eq!(detect("မီး", &all), Some("my"));
        assert_eq!(detect("ひかり", &all), Some("jp"));
        assert_eq!(detect("（光）", &all), Some("jp"));
        assert_eq!(detect("light", &all), None);
        assert_eq!(detect("2026", &all), None);
        assert_eq!(detect("မီး", &["en", "jp"]), None);
    }

    #[test]
    fn languages_are_named_in_english() {
        assert_eq!(name("my"), "Burmese");
        assert_eq!(name("de"), "DE");
    }
}
