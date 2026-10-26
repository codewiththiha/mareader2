//! When each word is heard, from the frames a model gives each phoneme.

/// A word heard: its first sample in a piece, and its byte in the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mark {
    pub at: usize,
    pub byte: usize,
}

/// Phonemes joined from a G2P's tokens, and each phoneme word's text byte.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Spelt {
    pub phonemes: String,
    pub words: Vec<usize>,
}

/// Joins `(text, phonemes, space)` tokens, finding each in `text`.
pub(crate) fn spell<'a>(
    text: &str,
    tokens: impl Iterator<Item = (&'a str, &'a str, &'a str)>,
) -> Spelt {
    let mut spelt = Spelt::default();
    let (mut cursor, mut started) = (0, false);
    for (said, phonemes, space) in tokens {
        let found = text.get(cursor..).and_then(|rest| rest.find(said));
        let byte = found.map_or(cursor, |at| cursor + at);
        if found.is_some() {
            cursor = byte + said.len();
        }
        for c in phonemes.chars().chain(space.chars()) {
            if c.is_whitespace() {
                started = false;
            } else if !started && sounds(c) {
                started = true;
                spelt.words.push(byte);
            }
            spelt.phonemes.push(c);
        }
    }
    spelt
}

/// The character each phoneme word starts at, found as [`spell`] does.
pub(crate) fn word_starts(phonemes: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut started = false;
    for (i, c) in phonemes.chars().enumerate() {
        if c.is_whitespace() {
            started = false;
        } else if !started && sounds(c) {
            started = true;
            starts.push(i);
        }
    }
    starts
}

/// A letter or phoneme, not a mark: words start at one.
fn sounds(c: char) -> bool {
    c.is_alphanumeric()
}

/// Where words are in phonemes said: each start's character and text byte.
pub(crate) struct Words(Vec<(usize, usize)>);

impl Words {
    /// The words of `phonemes`, if they match `spelt`'s one to one.
    pub fn new(phonemes: &str, spelt: &Spelt) -> Self {
        let starts = word_starts(phonemes);
        match starts.len() == spelt.words.len() {
            true => Self(
                starts
                    .into_iter()
                    .zip(spelt.words.iter().copied())
                    .collect(),
            ),
            false => Self(Vec::new()),
        }
    }

    /// Those in the characters `from..until`, counted from `from`.
    pub fn within(&self, from: usize, until: usize) -> Vec<(usize, usize)> {
        let inside = self.0.iter().filter(|(c, _)| (from..until).contains(c));
        inside.map(|&(c, byte)| (c - from, byte)).collect()
    }
}

/// Marks for a piece's `words`, from each character's id, frames and samples.
pub(crate) fn place(
    words: &[(usize, usize)],
    id_at: &[usize],
    frames: &[usize],
    samples: usize,
) -> Vec<Mark> {
    let total: usize = frames.iter().sum();
    if total == 0 {
        return Vec::new();
    }
    let per = samples as f64 / total as f64;
    let mut before = Vec::with_capacity(frames.len() + 1);
    before.push(0);
    for f in frames {
        before.push(before.last().copied().unwrap_or(0) + f);
    }
    let at = |id: usize| before.get(id).or(before.last()).copied().unwrap_or(0);
    let marks = words.iter().filter_map(|&(c, byte)| {
        let id = id_at.get(c).copied()?;
        let sample = (at(id) as f64 * per).round() as usize;
        Some(Mark { at: sample, byte })
    });
    marks.collect()
}

/// Marks moved for `cut` samples gone from the start, kept in `len`.
pub(crate) fn shift(marks: &mut [Mark], cut: usize, len: usize) {
    for m in marks {
        m.at = m.at.saturating_sub(cut).min(len);
    }
}

/// Characters before each of `pieces`, cut in order from `whole`.
pub(crate) fn offsets(whole: &str, pieces: &[String]) -> Vec<usize> {
    let mut cursor = 0;
    let mut out = Vec::with_capacity(pieces.len());
    for piece in pieces {
        let at = whole
            .get(cursor..)
            .and_then(|rest| rest.find(piece.as_str()));
        let byte = at.map_or(cursor, |at| cursor + at);
        out.push(whole.get(..byte).map_or(0, |b| b.chars().count()));
        cursor = byte + piece.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens<'a>(
        list: &'a [(&'a str, &'a str, &'a str)],
    ) -> impl Iterator<Item = (&'a str, &'a str, &'a str)> {
        list.iter().copied()
    }

    #[test]
    fn words_are_found_in_the_text_and_in_the_phonemes() {
        let text = "Well-known, it's 1999!";
        let list = [
            ("Well", "wˈɛl", ""),
            ("-", "-", ""),
            ("known", "nˈOn", ""),
            (",", ",", " "),
            ("it's", "ɪts", " "),
            ("1999", "nˈIntin nˈIndi", ""),
            ("!", "!", ""),
        ];
        let spelt = spell(text, tokens(&list));
        assert_eq!(spelt.phonemes, "wˈɛl-nˈOn, ɪts nˈIntin nˈIndi!");
        // A hyphen joins its halves into one word; a number says two.
        assert_eq!(spelt.words, [0, 12, 17, 17]);
        assert_eq!(word_starts(&spelt.phonemes), [0, 11, 15, 23]);
        let words = Words::new(&spelt.phonemes, &spelt);
        assert_eq!(words.within(11, 30), [(0, 12), (4, 17), (12, 17)]);
        // Spelt anew with a word lost, nothing is matched.
        assert!(Words::new("wˈɛl ɪts", &spelt).within(0, 99).is_empty());
    }

    #[test]
    fn marks_fall_where_their_frames_begin() {
        // Ids: pad, a, pad, b; a word at each of a and b.
        let marks = place(&[(0, 0), (1, 4)], &[1, 3], &[2, 4, 2, 2], 1000);
        assert_eq!(
            marks,
            [Mark { at: 200, byte: 0 }, Mark { at: 800, byte: 4 }]
        );
        assert!(place(&[(0, 0)], &[1], &[0, 0], 10).is_empty());
        let mut moved = marks;
        shift(&mut moved, 300, 400);
        let at: Vec<usize> = moved.iter().map(|m| m.at).collect();
        assert_eq!(at, [0, 400]);
    }

    #[test]
    fn pieces_are_found_in_order() {
        let whole = "ab, cd ab, ef";
        let pieces = ["ab,".to_owned(), "cd ab,".to_owned(), "ef".to_owned()];
        assert_eq!(offsets(whole, &pieces), [0, 4, 11]);
    }
}
