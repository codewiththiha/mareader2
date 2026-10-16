//! Reading JSON from model text: whole answers and answers mid-stream.

use serde_json::Value;

/// How many cut points a partial answer tries before giving up.
const MAX_CUTS: usize = 8;

/// The answer in `text`: bare, fenced, or amid prose.
pub fn whole(text: &str) -> Option<Value> {
    let text = strip_fences(after_thinking(text)?);
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        return Some(v);
    }
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(text.get(start..=end)?).ok()
}

/// What a prefix of an object says so far; open strings and brackets close.
pub fn partial(text: &str) -> Option<Value> {
    let text = after_thinking(text)?;
    let body = text.get(text.find('{')?..)?;
    let scan = Scan::of(body);
    if let Some(v) = close(body, scan.end.as_ref()) {
        return Some(v);
    }
    // Drop the last member, which may be a bare key or a cut number.
    scan.cuts
        .iter()
        .rev()
        .take(MAX_CUTS)
        .find_map(|(at, open)| close(body.get(..*at)?, Some(open)))
}

/// Text after a reasoning model's `<think>` block; none while it thinks.
fn after_thinking(text: &str) -> Option<&str> {
    match text.find("</think>") {
        Some(end) => text.get(end + "</think>".len()..),
        None if text.trim_start().starts_with("<think>") => None,
        None => Some(text),
    }
}

fn strip_fences(text: &str) -> &str {
    let t = text.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return t;
    };
    let rest = rest.split_once('\n').map_or(rest, |(_, body)| body);
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}

/// Where a prefix stands: open brackets and whether a string is open.
#[derive(Clone, Debug, Default)]
struct Open {
    brackets: Vec<char>,
    in_string: bool,
    /// The prefix ends inside an escape, so its last character goes.
    escape: bool,
}

struct Scan {
    /// The state at the end, unless a closer didn't match.
    end: Option<Open>,
    /// Commas outside strings, with the state just before each.
    cuts: Vec<(usize, Open)>,
}

impl Scan {
    fn of(body: &str) -> Self {
        let mut open = Open::default();
        let mut cuts = Vec::new();
        for (i, c) in body.char_indices() {
            if open.in_string {
                match (open.escape, c) {
                    (true, _) => open.escape = false,
                    (false, '\\') => open.escape = true,
                    (false, '"') => open.in_string = false,
                    _ => {}
                }
                continue;
            }
            match c {
                '"' => open.in_string = true,
                '{' | '[' => open.brackets.push(c),
                '}' | ']' => {
                    let want = if c == '}' { '{' } else { '[' };
                    if open.brackets.pop() != Some(want) {
                        return Self { end: None, cuts };
                    }
                }
                ',' => cuts.push((i, open.clone())),
                _ => {}
            }
        }
        Self {
            end: Some(open),
            cuts,
        }
    }
}

/// `prefix` closed as `open` says, if that parses.
fn close(prefix: &str, open: Option<&Open>) -> Option<Value> {
    let open = open?;
    let mut text = prefix.to_owned();
    if open.escape {
        text.pop();
    }
    if open.in_string {
        text.push('"');
    }
    for b in open.brackets.iter().rev() {
        text.push(if *b == '{' { '}' } else { ']' });
    }
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn whole_answers_come_bare_fenced_or_wrapped() {
        let want = json!({"a": 1});
        assert_eq!(whole(r#"{"a":1}"#), Some(want.clone()));
        assert_eq!(whole("```json\n{\"a\":1}\n```"), Some(want.clone()));
        assert_eq!(whole("Sure! {\"a\":1} Hope it helps."), Some(want.clone()));
        assert_eq!(whole("<think>{maybe}</think>\n{\"a\":1}"), Some(want));
        assert_eq!(whole("no json here"), None);
    }

    #[test]
    fn a_prefix_shows_what_it_has() {
        let p = |s: &str| partial(s).map(|v| v.to_string());
        assert_eq!(
            p(r#"{"pos":"adj","meaning":"Moving fa"#).as_deref(),
            Some(r#"{"pos":"adj","meaning":"Moving fa"}"#)
        );
        assert_eq!(
            p(r#"{"pos":"adj","mea"#).as_deref(),
            Some(r#"{"pos":"adj"}"#)
        );
        assert_eq!(
            p(r#"{"pos":"adj","meaning":"#).as_deref(),
            Some(r#"{"pos":"adj"}"#)
        );
        assert_eq!(p(r#"{"pos":"adj","#).as_deref(), Some(r#"{"pos":"adj"}"#));
        assert_eq!(p(r#"{"s":["a","b"#).as_deref(), Some(r#"{"s":["a","b"]}"#));
        assert_eq!(p(r#"{"m":"say \"#).as_deref(), Some(r#"{"m":"say "}"#));
        assert_eq!(p(r#"{"n":1"#).as_deref(), Some(r#"{"n":1}"#));
    }

    #[test]
    fn nothing_shows_before_an_object_or_while_thinking() {
        assert_eq!(partial(""), None);
        assert_eq!(partial("Sure"), None);
        assert_eq!(partial("<think>{\"a\":"), None);
        assert_eq!(partial("{\"a\":1}]"), None, "a stray closer");
        assert_eq!(partial("{").map(|v| v.to_string()).as_deref(), Some("{}"));
    }
}
