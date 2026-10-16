//! Which selections can be explained, and the words around them.

use std::ops::Range;

/// The longest word explained, in characters.
pub const MAX_GLOSS_CHARS: usize = 60;
/// Characters of context taken on each side of the word.
pub const CONTEXT_CHARS: usize = 60;

/// Marks that hug a word without being part of it.
fn hugging(c: char) -> bool {
    c.is_whitespace()
        || c.is_ascii_punctuation()
        || matches!(
            c,
            '“' | '”'
                | '‘'
                | '’'
                | '«'
                | '»'
                | '‹'
                | '›'
                | '„'
                | '‚'
                | '—'
                | '–'
                | '…'
                | '¡'
                | '¿'
                | '。'
                | '、'
                | '，'
                | '「'
                | '」'
                | '『'
                | '』'
                | '（'
                | '）'
                | '！'
                | '？'
                | '：'
                | '；'
                | '·'
                | '•'
                | '\u{AD}'
        )
}

/// The one word in `text`, as a character range; none past the limit.
pub fn word_in(text: &str) -> Option<Range<usize>> {
    let chars: Vec<char> = text.chars().collect();
    let start = chars.iter().position(|&c| !hugging(c))?;
    let end = chars.iter().rposition(|&c| !hugging(c))? + 1;
    let word = chars.get(start..end)?;
    let one = !word.iter().any(|c| c.is_whitespace());
    (one && word.len() <= MAX_GLOSS_CHARS).then_some(start..end)
}

/// Up to [`CONTEXT_CHARS`] characters either side of `word` in `text`,
/// trimmed; `word` counts characters.
pub fn context(text: &str, word: Range<usize>) -> String {
    let from = word.start.saturating_sub(CONTEXT_CHARS);
    let to = word.end.saturating_add(CONTEXT_CHARS);
    let window: String = text.chars().skip(from).take(to - from).collect();
    let flat: Vec<&str> = window.split_whitespace().collect();
    flat.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_word_passes_and_its_hugging_marks_fall_away() {
        assert_eq!(word_in("fox"), Some(0..3));
        assert_eq!(word_in(" “fox,” "), Some(2..5));
        assert_eq!(word_in("don't"), Some(0..5));
        assert_eq!(word_in("မြန်မာ"), Some(0..6), "marks inside a word stay");
        assert_eq!(word_in(&"🙂".repeat(MAX_GLOSS_CHARS)), Some(0..60));
    }

    #[test]
    fn phrases_blanks_and_long_runs_are_refused() {
        assert_eq!(word_in(""), None);
        assert_eq!(word_in(" ,. "), None);
        assert_eq!(word_in("quick brown"), None);
        assert_eq!(word_in("line\nbreak"), None);
        assert_eq!(word_in(&"a".repeat(MAX_GLOSS_CHARS + 1)), None);
    }

    #[test]
    fn context_takes_the_characters_around_and_flattens_lines() {
        let text = "The quick brown\nfox jumps over the lazy dog.";
        let fox = 16..19;
        assert_eq!(
            context(text, fox),
            "The quick brown fox jumps over the lazy dog."
        );
        let long = format!("{}word{}", "a ".repeat(50), " b".repeat(50));
        let got = context(&long, 100..104);
        assert!(got.contains("word"));
        assert!(got.chars().count() <= 4 + 2 * CONTEXT_CHARS, "{got}");
    }
}
