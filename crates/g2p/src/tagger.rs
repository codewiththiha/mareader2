//! An averaged-perceptron tagger trained on spaCy's tags, which misaki reads.

use std::collections::HashMap;

use crate::Error;

const MAGIC: &[u8; 8] = b"LQTAG001";
const START: [&str; 2] = ["-START-", "-START2-"];
const END: [&str; 2] = ["-END-", "-END2-"];

/// Whitespace tokens get spaCy's `_SP` tag without consulting the model.
pub(crate) const SPACE_TAG: &str = "_SP";

pub(crate) struct Tagger {
    classes: Vec<String>,
    /// Words the training data always tagged the same way.
    tagdict: HashMap<String, u8>,
    weights: HashMap<String, Vec<(u8, f32)>>,
}

fn bad(reason: &str) -> Error {
    Error::Data(format!("tagger: {reason}"))
}

impl Tagger {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(MAGIC.len())? != MAGIC {
            return Err(bad("not a tagger model"));
        }
        let classes = (0..r.u8()?)
            .map(|_| r.str8())
            .collect::<Result<Vec<_>, _>>()?;
        let mut tagdict = HashMap::new();
        for _ in 0..r.u32()? {
            let word = r.str8()?;
            let class = r.u8()?;
            if usize::from(class) >= classes.len() {
                return Err(bad("unknown tag"));
            }
            tagdict.insert(word, class);
        }
        let mut weights = HashMap::new();
        for _ in 0..r.u32()? {
            let feature = r.str16()?;
            let n = r.u8()?;
            let row = (0..n)
                .map(|_| Ok((r.u8()?, r.f32()?)))
                .collect::<Result<Vec<_>, Error>>()?;
            weights.insert(feature, row);
        }
        if r.at != bytes.len() {
            return Err(bad("trailing bytes"));
        }
        Ok(Self {
            classes,
            tagdict,
            weights,
        })
    }

    fn class(&self, class: usize) -> String {
        self.classes.get(class).cloned().unwrap_or_default()
    }

    /// One tag per word of a tokenized sentence.
    pub(crate) fn tag(&self, words: &[&str]) -> Vec<String> {
        let context = context(words);
        let mut prev = START[0].to_owned();
        let mut prev2 = START[1].to_owned();
        let mut tags = Vec::with_capacity(words.len());
        for (i, word) in words.iter().enumerate() {
            let tag = if word.chars().all(char::is_whitespace) && !word.is_empty() {
                SPACE_TAG.to_owned()
            } else if let Some(&class) = self.tagdict.get(*word) {
                self.class(usize::from(class))
            } else {
                let mut scores = vec![0f32; self.classes.len()];
                for feature in features(i, word, &context, &prev, &prev2) {
                    for &(class, weight) in self.weights.get(&feature).into_iter().flatten() {
                        if let Some(score) = scores.get_mut(usize::from(class)) {
                            *score += weight;
                        }
                    }
                }
                // Ties go to the later class, as training broke them.
                let best = scores
                    .iter()
                    .enumerate()
                    .max_by(|(a, x), (b, y)| x.total_cmp(y).then(a.cmp(b)))
                    .map_or(0, |(i, _)| i);
                self.class(best)
            };
            prev2 = std::mem::replace(&mut prev, tag.clone());
            tags.push(tag);
        }
        tags
    }
}

/// The normalised sentence between start and end markers.
fn context(words: &[&str]) -> Vec<String> {
    START
        .iter()
        .map(|s| (*s).to_owned())
        .chain(words.iter().map(|w| normalise(w)))
        .chain(END.iter().map(|s| (*s).to_owned()))
        .collect()
}

/// Folds words that behave alike onto one feature value.
fn normalise(word: &str) -> String {
    let first = word.chars().next();
    if word.contains('-') && first != Some('-') {
        "!HYPHEN".to_owned()
    } else if word.len() == 4 && word.bytes().all(|b| b.is_ascii_digit()) {
        "!YEAR".to_owned()
    } else if first.is_some_and(|c| c.is_ascii_digit()) {
        "!DIGITS".to_owned()
    } else {
        word.to_lowercase()
    }
}

/// The features the model was trained on; their text is its key.
fn features(i: usize, word: &str, context: &[String], prev: &str, prev2: &str) -> Vec<String> {
    let c = i + START.len();
    let at = |k: usize| context.get(k).map(String::as_str).unwrap_or_default();
    vec![
        "bias".to_owned(),
        format!("i suffix {}", suffix(word, 3)),
        format!("i suffix2 {}", suffix(word, 2)),
        format!("i suffix1 {}", suffix(word, 1)),
        format!(
            "i pref1 {}",
            word.chars().next().map(String::from).unwrap_or_default()
        ),
        format!(
            "i pref3 {}",
            word.chars().take(3).collect::<String>().to_lowercase()
        ),
        format!("i shape {}", shape(word)),
        format!("i-1 tag {prev}"),
        format!("i-2 tag {prev2}"),
        format!("i tag+i-2 tag {prev} {prev2}"),
        format!("i word {}", at(c)),
        format!("i-1 tag+i word {prev} {}", at(c)),
        format!("i-1 word {}", at(c - 1)),
        format!("i-1 suffix {}", suffix(at(c - 1), 3)),
        format!("i-2 word {}", at(c - 2)),
        format!("i+1 word {}", at(c + 1)),
        format!("i+1 suffix {}", suffix(at(c + 1), 3)),
        format!("i+2 word {}", at(c + 2)),
    ]
}

fn suffix(word: &str, n: usize) -> String {
    let skip = word.chars().count().saturating_sub(n);
    word.chars().skip(skip).collect()
}

/// spaCy's word shape: "Apple" is "Xxxxx", "1999" "dddd", runs cut at four.
fn shape(word: &str) -> String {
    let mut out = String::new();
    let mut last = None;
    let mut run = 0;
    for c in word.chars() {
        let s = if c.is_alphabetic() {
            if c.is_uppercase() { 'X' } else { 'x' }
        } else if c.is_ascii_digit() {
            'd'
        } else {
            c
        };
        run = if Some(s) == last { run + 1 } else { 1 };
        last = Some(s);
        if run <= 4 {
            out.push(s);
        }
    }
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let end = self.at.checked_add(n).ok_or_else(|| bad("truncated"))?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| bad("truncated"))?;
        self.at = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        self.take(N)?.try_into().map_err(|_| bad("truncated"))
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(u8::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    fn str8(&mut self) -> Result<String, Error> {
        let n = self.u8()?;
        self.string(usize::from(n))
    }

    fn str16(&mut self) -> Result<String, Error> {
        let n = u16::from_le_bytes(self.array()?);
        self.string(usize::from(n))
    }

    fn string(&mut self, n: usize) -> Result<String, Error> {
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| bad("text is not UTF-8"))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn push_str(out: &mut Vec<u8>, s: &str, wide: bool) {
        let n = s.len() as u16;
        match wide {
            true => out.extend(n.to_le_bytes()),
            false => out.push(n as u8),
        }
        out.extend(s.as_bytes());
    }

    /// A model in the binary format: tags, fixed words, feature weights.
    pub(crate) fn encode(
        classes: &[&str],
        tagdict: &[(&str, u8)],
        weights: &[(&str, &[(u8, f32)])],
    ) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.push(classes.len() as u8);
        for class in classes {
            push_str(&mut out, class, false);
        }
        out.extend((tagdict.len() as u32).to_le_bytes());
        for (word, class) in tagdict {
            push_str(&mut out, word, false);
            out.push(*class);
        }
        out.extend((weights.len() as u32).to_le_bytes());
        for (feature, row) in weights {
            push_str(&mut out, feature, true);
            out.push(row.len() as u8);
            for (class, weight) in row.iter() {
                out.push(*class);
                out.extend(weight.to_le_bytes());
            }
        }
        out
    }

    #[test]
    fn shape_matches_spacy() {
        assert_eq!(shape("Apple"), "Xxxxx");
        assert_eq!(shape("1999"), "dddd");
        assert_eq!(shape("9:30"), "d:dd");
        assert_eq!(shape("internationalization"), "xxxx");
    }

    #[test]
    fn tags_come_from_the_dictionary_then_the_weights() {
        let model = encode(
            &["NN", "VB"],
            &[("the", 0)],
            &[("bias", &[(1, 0.5)]), ("i word cat", &[(0, 2.0)])],
        );
        let tagger = Tagger::from_bytes(&model).ok();
        let tags = tagger.map(|t| t.tag(&["the", "run", "cat", " "]));
        assert_eq!(
            tags.as_deref(),
            Some(&["NN", "VB", "NN", "_SP"].map(String::from)[..])
        );
    }

    #[test]
    fn a_damaged_model_is_an_error() {
        let model = encode(&["NN"], &[("the", 0)], &[]);
        for cut in [0, 5, model.len() - 1] {
            let damaged = model.get(..cut).unwrap_or_default();
            assert!(Tagger::from_bytes(damaged).is_err());
        }
        let mut longer = model.clone();
        longer.push(0);
        assert!(Tagger::from_bytes(&longer).is_err());
        assert!(Tagger::from_bytes(&encode(&["NN"], &[("the", 3)], &[])).is_err());
    }
}
