//! Where sentences end: the stops a voice pauses at.

use std::ops::Range;

/// Longest a sentence runs, in characters, before it breaks at a pause.
pub const LONGEST: usize = 360;

/// Words a full stop shortens, lower case, without their stop.
const SHORTENED: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "vs", "etc", "e.g", "i.e", "cf", "no",
    "vol", "fig", "pp", "ch", "mt", "ft", "inc", "ltd", "co", "jan", "feb", "mar", "apr", "jun",
    "jul", "aug", "sep", "sept", "oct", "nov", "dec", "approx", "dept", "gen", "gov", "lt", "col",
    "capt", "rev", "hon", "op", "ed", "eds", "viz",
];

/// Ends a sentence when a space or the end follows.
fn stop(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '…' | '‼' | '⁇' | '؟' | '।' | '॥')
}

/// Ends a sentence where it stands: these scripts put no space after.
fn stop_unspaced(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '။' | '｡')
}

/// Closes what a sentence opened: quotes and brackets after its stop.
fn closer(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '”' | '’' | '»' | '›' | ')' | ']' | '}' | '」' | '』' | '）' | '〕' | '】'
    )
}

/// The sentences of `text`, as byte ranges, trimmed; a newline ends one.
pub fn split(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let end = if c == '\n' {
            Some(i)
        } else if stop(c) || stop_unspaced(c) {
            let mut end = i + c.len_utf8();
            while let Some(&(j, n)) = chars.peek() {
                if !(stop(n) || stop_unspaced(n) || closer(n)) {
                    break;
                }
                end = j + n.len_utf8();
                chars.next();
            }
            let next = chars.peek().map(|&(_, n)| n);
            let spaced = next.is_none_or(char::is_whitespace);
            let short = c == '.' && shortened(text, start, i);
            let ends = stop_unspaced(c) || (spaced && !short && !carries_on(text, end));
            ends.then_some(end)
        } else {
            None
        };
        if let Some(end) = end {
            push(&mut out, text, start..end);
            start = end;
        }
    }
    push(&mut out, text, start..text.len());
    out
}

/// True when the stop at `dot` shortens a word rather than ending.
fn shortened(text: &str, start: usize, dot: usize) -> bool {
    let before = text.get(start..dot).unwrap_or_default();
    let word = before
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or_default();
    let word = word.trim_start_matches(|c: char| !c.is_alphanumeric());
    // A single letter is an initial: J. R. R. Tolkien.
    let initial = word.chars().count() == 1 && word.chars().all(char::is_alphabetic);
    initial || SHORTENED.contains(&word.to_lowercase().as_str())
}

/// True when a lower-case word follows `at`: the same sentence goes on.
fn carries_on(text: &str, at: usize) -> bool {
    let after = text.get(at..).unwrap_or_default();
    let after = after.trim_start_matches([' ', '\t']);
    after.chars().next().is_some_and(char::is_lowercase)
}

/// Adds `range` trimmed, broken at pauses when long, if it says anything.
fn push(out: &mut Vec<Range<usize>>, text: &str, range: Range<usize>) {
    let Some(part) = text.get(range.clone()) else {
        return;
    };
    let lead = part.len() - part.trim_start().len();
    let trail = part.len() - part.trim_end().len();
    let (mut from, to) = (range.start + lead, range.end - trail);
    while from < to {
        let rest = text.get(from..to).unwrap_or_default();
        let cut = match rest.chars().count() > LONGEST {
            true => pause(rest).map_or(to, |at| from + at),
            false => to,
        };
        let piece = text.get(from..cut).unwrap_or_default();
        let trimmed = piece.trim_end();
        if trimmed.chars().any(char::is_alphanumeric) {
            out.push(from..from + trimmed.len());
        }
        let skip = text.get(cut..to).unwrap_or_default();
        from = cut + (skip.len() - skip.trim_start().len());
    }
}

/// A long run's break within [`LONGEST`]: after a pause mark, else a space.
fn pause(text: &str) -> Option<usize> {
    let limit = text
        .char_indices()
        .nth(LONGEST)
        .map_or(text.len(), |(i, _)| i);
    let head = text.get(..limit)?;
    let marks = [';', ':', ',', '—', '–', '、', '，', '；', '၊'];
    let at = head
        .rfind(|c| marks.contains(&c))
        .filter(|&i| i > limit / 3);
    let mark_end = |i: usize| head.get(i..)?.chars().next().map(|c| i + c.len_utf8());
    at.and_then(mark_end)
        .or_else(|| head.rfind(char::is_whitespace).filter(|&i| i > 0))
        .or(Some(limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(text: &str) -> Vec<&str> {
        split(text)
            .into_iter()
            .filter_map(|r| text.get(r))
            .collect()
    }

    #[test]
    fn stops_end_sentences_and_quotes_go_with_them() {
        let text = "It rained. \"Did it?\" she asked. Yes!  It did…";
        assert_eq!(
            said(text),
            ["It rained.", "\"Did it?\" she asked.", "Yes!", "It did…"]
        );
    }

    #[test]
    fn titles_initials_and_numbers_do_not_end_one() {
        let text = "Mr. Smith met J. R. Tolkien at 3.30 p.m. on Monday. Then he left.";
        assert_eq!(
            said(text),
            [
                "Mr. Smith met J. R. Tolkien at 3.30 p.m. on Monday.",
                "Then he left."
            ]
        );
    }

    #[test]
    fn a_newline_ends_a_heading_without_a_stop() {
        assert_eq!(said("Chapter One\nIt began."), ["Chapter One", "It began."]);
    }

    #[test]
    fn unspaced_scripts_end_at_their_stops() {
        assert_eq!(said("雨だ。晴れた！"), ["雨だ。", "晴れた！"]);
        assert_eq!(said("မိုးရွာသည်။နေသာသည်။"), ["မိုးရွာသည်။", "နေသာသည်။"]);
    }

    #[test]
    fn marks_alone_say_nothing() {
        assert_eq!(said("* * *\nThe end."), ["The end."]);
        assert!(said("   ").is_empty());
    }

    #[test]
    fn a_long_run_breaks_at_a_pause_within_the_limit() {
        let clause = "and the river ran on past the mill";
        let text = vec![clause; 20].join(", ") + ".";
        let parts = said(&text);
        assert!(parts.len() > 1);
        assert!(parts.iter().all(|p| p.chars().count() <= LONGEST));
        assert_eq!(parts.concat().replace(' ', ""), text.replace(' ', ""));
    }
}
