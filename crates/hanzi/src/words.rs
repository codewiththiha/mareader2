//! Chinese text into words, as jieba's default cut finds them.

use crate::Error;
use crate::table::Table;

/// jieba's stand-in for a probability of nothing.
const NEVER: f64 = -3.14e100;

/// Hidden states for a character: begins, ends, middle of, or single.
const B: usize = 0;
const E: usize = 1;
const M: usize = 2;
const S: usize = 3;

/// jieba's start and move probabilities, as logs, in B, E, M, S order.
const START: [f64; 4] = [-0.26268660809250016, NEVER, NEVER, -1.4652633398537678];
const MOVES: [[f64; 4]; 4] = [
    [NEVER, -0.510825623765990, -0.916290731874155, NEVER],
    [-0.5897149736854513, NEVER, NEVER, -0.8085250474669937],
    [NEVER, -0.33344856811948514, -1.2603623820268226, NEVER],
    [-0.7211965654669841, NEVER, NEVER, -0.6658631448798212],
];

/// The states each state may follow, as jieba lists them.
const BEFORE: [[usize; 2]; 4] = [[E, S], [B, M], [M, B], [S, E]];

/// jieba's dictionary and its model for words it doesn't know.
pub struct Words {
    counts: Table<u32>,
    log_total: f64,
    /// How likely each state is to show each character, as logs.
    shows: [Vec<(char, f64)>; 4],
}

impl Words {
    /// From jieba's `dict.txt` and `finalseg/prob_emit.py`.
    pub fn parse(dict: &str, emit: &str) -> Result<Self, Error> {
        let mut entries = Vec::new();
        let mut total: u64 = 0;
        for line in dict.lines() {
            let line = line.trim_matches(|c: char| c.is_ascii_whitespace());
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split(' ');
            let word = parts.next().unwrap_or_default();
            let count: u32 = parts
                .next()
                .and_then(|n| n.parse().ok())
                .ok_or(Error::Bad("dict.txt"))?;
            total = total.saturating_add(u64::from(count));
            entries.push((word, count));
        }
        if entries.is_empty() {
            return Err(Error::Bad("dict.txt"));
        }
        Ok(Self {
            counts: Table::new(entries).ok_or(Error::Bad("dict.txt"))?,
            log_total: (total as f64).ln(),
            shows: emissions(emit)?,
        })
    }

    pub fn len(&self) -> usize {
        self.counts.len()
    }

    /// The words of a run of Han characters, as `jieba.lcut` gives them.
    pub fn cut<'a>(&self, run: &'a str, out: &mut Vec<&'a str>) {
        // jieba's own Han range ends at U+9FD5; later ones stand alone.
        let mut start = 0;
        for (at, c) in run.char_indices() {
            if c > '\u{9FD5}' {
                self.cut_block(run.get(start..at).unwrap_or(""), out);
                out.push(run.get(at..at + c.len_utf8()).unwrap_or(""));
                start = at + c.len_utf8();
            }
        }
        self.cut_block(run.get(start..).unwrap_or(""), out);
    }

    fn count(&self, word: &str) -> u32 {
        self.counts.get(word).unwrap_or(0)
    }

    /// The likeliest path through the word graph, then unknown runs by HMM.
    fn cut_block<'a>(&self, text: &'a str, out: &mut Vec<&'a str>) {
        if text.is_empty() {
            return;
        }
        let bounds: Vec<usize> = text
            .char_indices()
            .map(|(at, _)| at)
            .chain([text.len()])
            .collect();
        let n = bounds.len() - 1;
        let piece = |from: usize, to: usize| -> &'a str {
            match (bounds.get(from), bounds.get(to)) {
                (Some(&a), Some(&b)) => text.get(a..b).unwrap_or(""),
                _ => "",
            }
        };
        // route[i]: the best score from i on, and where its first word ends.
        let mut route = vec![(0.0_f64, 0_usize); n + 1];
        for from in (0..n).rev() {
            let mut best: Option<(f64, usize)> = None;
            let mut to = from;
            let mut any = false;
            while to < n && self.counts.has_prefix(piece(from, to + 1)) {
                if self.count(piece(from, to + 1)) > 0 {
                    any = true;
                    best = better(best, self.score(piece(from, to + 1), &route, to));
                }
                to += 1;
            }
            if !any {
                best = better(best, self.score(piece(from, from + 1), &route, from));
            }
            if let (Some(best), Some(slot)) = (best, route.get_mut(from)) {
                *slot = best;
            }
        }
        let mut at = 0;
        let mut single = at;
        while at < n {
            let end = route.get(at).map_or(at, |r| r.1) + 1;
            if end - at > 1 {
                self.flush(piece(single, at), out);
                out.push(piece(at, end));
                single = end;
            }
            at = end;
        }
        self.flush(piece(single, n), out);
    }

    /// `log(count or 1) - log(total)` plus the best score after the word.
    fn score(&self, word: &str, route: &[(f64, usize)], last: usize) -> (f64, usize) {
        let count = self.count(word).max(1);
        let after = route.get(last + 1).map_or(0.0, |r| r.0);
        (f64::from(count).ln() - self.log_total + after, last)
    }

    /// A run of single characters: one word if known, else the HMM's cut.
    fn flush<'a>(&self, run: &'a str, out: &mut Vec<&'a str>) {
        let mut chars = run.char_indices();
        match (chars.next(), chars.next()) {
            (None, _) => {}
            (Some(_), None) => out.push(run),
            _ if self.count(run) > 0 => {
                out.extend(
                    run.char_indices()
                        .map(|(at, c)| run.get(at..at + c.len_utf8()).unwrap_or("")),
                );
            }
            _ => self.guess(run, out),
        }
    }

    /// jieba's Viterbi over begin, middle, end and single.
    fn guess<'a>(&self, run: &'a str, out: &mut Vec<&'a str>) {
        let chars: Vec<(usize, char)> = run.char_indices().collect();
        let shows = |state: usize, c: char| -> f64 {
            let Some(table) = self.shows.get(state) else {
                return NEVER;
            };
            match table.binary_search_by(|(k, _)| k.cmp(&c)) {
                Ok(at) => table.get(at).map_or(NEVER, |e| e.1),
                Err(_) => NEVER,
            }
        };
        let Some(&(_, first)) = chars.first() else {
            return;
        };
        let mut scores = [0.0; 4];
        for (state, score) in scores.iter_mut().enumerate() {
            *score = START.get(state).copied().unwrap_or(NEVER) + shows(state, first);
        }
        let mut back: Vec<[usize; 4]> = Vec::with_capacity(chars.len());
        for &(_, c) in chars.iter().skip(1) {
            let mut next = [0.0; 4];
            let mut from = [0; 4];
            for state in [B, M, E, S] {
                let shown = shows(state, c);
                let mut best: Option<(f64, usize)> = None;
                for &prev in BEFORE.get(state).into_iter().flatten() {
                    let moved = MOVES.get(prev).and_then(|m| m.get(state)).copied();
                    let score =
                        scores.get(prev).copied().unwrap_or(NEVER) + moved.unwrap_or(NEVER) + shown;
                    best = better(best, (score, prev));
                }
                if let (Some((score, prev)), Some(n), Some(f)) =
                    (best, next.get_mut(state), from.get_mut(state))
                {
                    *n = score;
                    *f = prev;
                }
            }
            scores = next;
            back.push(from);
        }
        let ends = [E, S].map(|s| (scores.get(s).copied().unwrap_or(NEVER), s));
        let mut state = better(Some(ends[0]), ends[1]).map_or(S, |e| e.1);
        let mut states = vec![state; chars.len()];
        for (i, from) in back.iter().enumerate().rev() {
            state = from.get(state).copied().unwrap_or(S);
            if let Some(slot) = states.get_mut(i) {
                *slot = state;
            }
        }
        let mut begin = 0;
        let mut next = 0;
        for (i, (&(at, c), &state)) in chars.iter().zip(&states).enumerate() {
            let end = at + c.len_utf8();
            match state {
                B => begin = at,
                E => {
                    out.push(run.get(begin..end).unwrap_or(""));
                    next = i + 1;
                }
                S => {
                    out.push(run.get(at..end).unwrap_or(""));
                    next = i + 1;
                }
                _ => {}
            }
        }
        if let Some(&(at, _)) = chars.get(next) {
            out.push(run.get(at..).unwrap_or(""));
        }
    }
}

/// Python's `max` over `(score, index)` pairs: ties go to the larger index.
fn better(best: Option<(f64, usize)>, candidate: (f64, usize)) -> Option<(f64, usize)> {
    match best {
        Some(b) if b.0 > candidate.0 || (b.0 == candidate.0 && b.1 > candidate.1) => Some(b),
        _ => Some(candidate),
    }
}

/// `prob_emit.py`: a Python dict of each state's characters and log odds.
fn emissions(text: &str) -> Result<[Vec<(char, f64)>; 4], Error> {
    let bad = Error::Bad("prob_emit.py");
    let mut shows: [Vec<(char, f64)>; 4] = Default::default();
    let mut state: Option<usize> = None;
    let mut rest = text.split_once('{').ok_or(Error::Bad("prob_emit.py"))?.1;
    while let Some(open) = rest.find('\'') {
        let after = rest.get(open + 1..).unwrap_or("");
        let close = after.find('\'').ok_or(Error::Bad("prob_emit.py"))?;
        let key = unescape(after.get(..close).unwrap_or("")).ok_or(Error::Bad("prob_emit.py"))?;
        let tail = after.get(close + 1..).unwrap_or("");
        let tail = tail
            .trim_start()
            .strip_prefix(':')
            .ok_or(Error::Bad("prob_emit.py"))?;
        let tail = tail.trim_start();
        if let Some(inner) = tail.strip_prefix('{') {
            state = match key.as_str() {
                "B" => Some(B),
                "E" => Some(E),
                "M" => Some(M),
                "S" => Some(S),
                _ => return Err(bad),
            };
            rest = inner;
            continue;
        }
        let len = tail
            .find(|c: char| c == ',' || c == '}' || c.is_whitespace())
            .unwrap_or(tail.len());
        let odds: f64 = tail
            .get(..len)
            .and_then(|n| n.parse().ok())
            .ok_or(Error::Bad("prob_emit.py"))?;
        let mut chars = key.chars();
        let (Some(c), None, Some(table)) = (
            chars.next(),
            chars.next(),
            state.and_then(|s| shows.get_mut(s)),
        ) else {
            return Err(bad);
        };
        table.push((c, odds));
        rest = tail.get(len..).unwrap_or("");
    }
    for table in &mut shows {
        // A repeated key keeps its last value, as in a Python dict.
        table.reverse();
        table.sort_by_key(|e| e.0);
        table.dedup_by_key(|e| e.0);
    }
    match shows.iter().all(|t| !t.is_empty()) {
        true => Ok(shows),
        false => Err(bad),
    }
}

/// A Python string literal's body, with its `\uXXXX` escapes.
fn unescape(body: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'u' => {
                let hex: String = chars.by_ref().take(4).collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
            }
            'U' => {
                let hex: String = chars.by_ref().take(8).collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
            }
            '\\' => out.push('\\'),
            '\'' => out.push('\''),
            _ => return None,
        }
    }
    Some(out)
}
