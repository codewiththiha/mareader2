//! Words to pinyin, as pypinyin's `lazy_pinyin` reads them by default.

use std::collections::HashMap;

use crate::Error;
use crate::json::{self, Json};
use crate::syllable;
use crate::table::Table;

/// pypinyin's readings: per character, and per phrase where they differ.
pub struct Pinyin {
    /// Each character's first reading.
    chars: HashMap<char, Box<str>>,
    /// Each phrase's readings, a span of `readings`.
    phrases: Table<(u32, u32)>,
    /// Every phrase's readings, space-separated, end to end.
    readings: String,
}

impl Pinyin {
    /// From pypinyin's `pinyin_dict.json` and `phrases_dict.json`.
    pub fn parse(chars: &str, phrases: &str) -> Result<Self, Error> {
        let mut map = HashMap::new();
        json::each(chars, |code, all| {
            let Json::Text(all) = all else {
                return None;
            };
            let c = char::from_u32(code.parse().ok()?)?;
            map.insert(c, all.split(',').next()?.into());
            Some(())
        })
        .ok_or(Error::Bad("pinyin_dict.json"))?;
        let mut readings = String::new();
        let mut entries = Vec::new();
        json::each(phrases, |phrase, each| {
            let Json::List(each) = each else {
                return None;
            };
            let start = u32::try_from(readings.len()).ok()?;
            for (i, reading) in each.iter().enumerate() {
                let Json::List(reading) = reading else {
                    return None;
                };
                let Some(Json::Text(first)) = reading.first() else {
                    return None;
                };
                if i > 0 {
                    readings.push(' ');
                }
                readings.push_str(first);
            }
            entries.push((phrase, (start, u32::try_from(readings.len()).ok()?)));
            Some(())
        })
        .ok_or(Error::Bad("phrases_dict.json"))?;
        if map.is_empty() || entries.is_empty() {
            return Err(Error::Bad("pinyin_dict.json"));
        }
        readings.shrink_to_fit();
        map.shrink_to_fit();
        Ok(Self {
            chars: map,
            phrases: Table::new(entries).ok_or(Error::Bad("phrases_dict.json"))?,
            readings,
        })
    }

    pub fn len(&self) -> (usize, usize) {
        (self.chars.len(), self.phrases.len())
    }

    /// A word's syllables as Kokoro's phonemes, run together.
    pub fn push(&self, word: &str, out: &mut String) {
        let mut rest = word;
        while !rest.is_empty() {
            let (piece, tail) = self.next_phrase(rest);
            rest = tail;
            match piece {
                Piece::Phrase(phrase) => self.say(phrase, out),
                Piece::Chars(chars) => {
                    for (at, c) in chars.char_indices() {
                        self.say(chars.get(at..at + c.len_utf8()).unwrap_or(""), out);
                    }
                }
            }
        }
    }

    /// One piece's readings: the phrase's own, else each character's first.
    fn say(&self, piece: &str, out: &mut String) {
        let span = self.phrases.get(piece);
        let phrase = span.and_then(|(a, b)| self.readings.get(a as usize..b as usize));
        match phrase {
            Some(readings) => {
                for reading in readings.split(' ') {
                    syllable::push(reading, out);
                }
            }
            None => {
                for c in piece.chars() {
                    if let Some(reading) = self.chars.get(&c) {
                        syllable::push(reading, out);
                    }
                }
            }
        }
    }

    /// pypinyin's strict longest match: the next phrase, else one character.
    fn next_phrase<'a>(&self, rest: &'a str) -> (Piece<'a>, &'a str) {
        let mut last = None;
        let ends = rest
            .char_indices()
            .skip(1)
            .map(|(at, _)| at)
            .chain([rest.len()]);
        for end in ends {
            let word = rest.get(..end).unwrap_or(rest);
            if !self.phrases.has_prefix(word) {
                return match last {
                    Some(end) => split(rest, end, Piece::Phrase),
                    None => split(rest, first_len(rest), Piece::Chars),
                };
            }
            if self.phrases.get(word).is_some() {
                last = Some(end);
            }
        }
        match last {
            Some(end) => split(rest, end, Piece::Phrase),
            None => (Piece::Chars(rest), ""),
        }
    }
}

enum Piece<'a> {
    Phrase(&'a str),
    /// Characters read one by one.
    Chars(&'a str),
}

fn split<'a>(text: &'a str, at: usize, piece: fn(&'a str) -> Piece<'a>) -> (Piece<'a>, &'a str) {
    let (head, tail) = text.split_at_checked(at).unwrap_or((text, ""));
    (piece(head), tail)
}

fn first_len(text: &str) -> usize {
    text.chars().next().map_or(text.len(), char::len_utf8)
}
