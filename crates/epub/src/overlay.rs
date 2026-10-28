//! Narration: a chapter's media overlay read into timed phrases (E10).

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use mareader_reflow::Anchor;

use crate::href;
use crate::xhtml::Refs;

/// Phrases kept from one overlay file, so a huge one can't swamp memory.
pub const MAX_PHRASES: usize = 100_000;
/// `seq` nesting followed; deeper ones count as their outermost.
const MAX_DEPTH: usize = 64;

/// Kinds of secondary content a listener may choose to skip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Skips {
    /// Footnotes, endnotes and other notes.
    pub notes: bool,
    /// Printed page numbers.
    pub page_numbers: bool,
}

impl Skips {
    fn of(kinds: &str) -> Self {
        let has = |names: &[&str]| kinds.split_ascii_whitespace().any(|k| names.contains(&k));
        Self {
            notes: has(&["footnote", "endnote", "rearnote", "note", "noteref"]),
            page_numbers: has(&["pagebreak", "page-break"]),
        }
    }

    fn or(self, other: Self) -> Self {
        Self {
            notes: self.notes || other.notes,
            page_numbers: self.page_numbers || other.page_numbers,
        }
    }

    pub fn any(self) -> bool {
        self.notes || self.page_numbers
    }
}

/// Whether `kinds` names a structure a listener may step out of.
fn escapable(kinds: &str) -> bool {
    let names = [
        "table",
        "table-row",
        "table-cell",
        "list",
        "list-item",
        "figure",
        "aside",
        "sidebar",
    ];
    kinds.split_ascii_whitespace().any(|k| names.contains(&k))
}

/// One `par` as written: the text it shows and the audio that reads it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Par {
    /// The text's file and its fragment, if it names one.
    pub(crate) file: String,
    pub(crate) id: Option<String>,
    pub(crate) audio: Option<Spoken>,
    pub(crate) skips: Skips,
    /// The outermost escapable structure it is in, numbered book-wide.
    pub(crate) escape: Option<u32>,
}

/// A stretch of an audio file, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Spoken {
    /// Its path in the archive.
    pub path: String,
    pub begin: f64,
    /// `None` plays to the file's end.
    pub end: Option<f64>,
}

/// A stretch of the book read aloud, and where its sound is.
#[derive(Debug, Clone, PartialEq)]
pub struct Phrase {
    /// The text it reads, from `start` up to `end`.
    pub start: Anchor,
    pub end: Anchor,
    pub audio: Spoken,
    pub skips: Skips,
    /// Phrases sharing one leave a table or list together.
    pub escape: Option<u32>,
}

/// A book's narration: its phrases in order, and who reads them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Narration {
    pub phrases: Vec<Phrase>,
    pub narrator: Option<String>,
    /// The whole book's length in seconds, as its metadata says.
    pub duration: Option<f64>,
}

/// Clips this close count as one read straight on.
const TOUCHING: f64 = 0.002;

/// Phrases read straight through one stretch of one file.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub path: String,
    pub from: f64,
    pub until: Option<f64>,
    /// The phrases it reads, by index, in order.
    pub phrases: Vec<usize>,
}

impl Narration {
    /// Whether phrase `i` is read when `skip`'s kinds are left out.
    pub fn reads(&self, i: usize, skip: Skips) -> bool {
        self.phrases.get(i).is_some_and(|p| {
            let s = p.skips;
            !(s.notes && skip.notes || s.page_numbers && skip.page_numbers)
        })
    }

    /// What is read from phrase `from` to the end, in runs.
    pub fn runs(&self, from: usize, skip: Skips) -> Vec<Run> {
        let mut runs: Vec<Run> = Vec::new();
        for (i, p) in self.phrases.iter().enumerate().skip(from) {
            if !self.reads(i, skip) {
                continue;
            }
            let a = &p.audio;
            if let Some(run) = runs.last_mut() {
                let on = run.until.is_some_and(|u| (u - a.begin).abs() < TOUCHING);
                if on && run.path == a.path {
                    run.until = a.end;
                    run.phrases.push(i);
                    continue;
                }
            }
            runs.push(Run {
                path: a.path.clone(),
                from: a.begin,
                until: a.end,
                phrases: vec![i],
            });
        }
        runs
    }

    /// The phrase `run` reads `time` seconds into its file.
    pub fn phrase_at(&self, run: &Run, time: f64) -> Option<usize> {
        let begins = |i: &&usize| {
            let p = self.phrases.get(**i);
            p.is_some_and(|p| p.audio.begin <= time + TOUCHING)
        };
        let last = run.phrases.iter().rev().find(begins);
        last.or(run.phrases.first()).copied()
    }

    /// The next phrase read after `i`.
    pub fn next(&self, i: usize, skip: Skips) -> Option<usize> {
        (i + 1..self.phrases.len()).find(|&j| self.reads(j, skip))
    }

    /// The phrase read before `i`.
    pub fn previous(&self, i: usize, skip: Skips) -> Option<usize> {
        (0..i).rev().find(|&j| self.reads(j, skip))
    }

    /// The first phrase read past the table or list `i` is in.
    pub fn escape(&self, i: usize, skip: Skips) -> Option<usize> {
        let from = self.phrases.get(i)?.escape?;
        let out = |j: &usize| self.phrases.get(*j).is_some_and(|p| p.escape != Some(from));
        (i + 1..self.phrases.len()).find(|j| out(j) && self.reads(*j, skip))
    }

    /// The first phrase read that ends after `at`.
    pub fn from_anchor(&self, at: Anchor, skip: Skips) -> Option<usize> {
        let ends = |i: &usize| self.phrases.get(*i).is_some_and(|p| p.end > at);
        (0..self.phrases.len()).find(|i| ends(i) && self.reads(*i, skip))
    }
}

/// `pars` as phrases of the book; those without sound are dropped.
pub(crate) fn phrases(pars: Vec<Par>, refs: &Refs, blocks: usize) -> Vec<Phrase> {
    let mut starts: Vec<usize> = (refs.places.iter())
        .filter(|(key, _)| !key.contains('#'))
        .map(|(_, at)| at.block)
        .collect();
    starts.sort_unstable();
    starts.dedup();
    // A whole chapter runs to where the next one starts.
    let chapter = |start: Anchor| {
        let next = starts.iter().find(|b| **b > start.block);
        let block = next.copied().unwrap_or(blocks.max(start.block + 1));
        (start, Anchor { block, offset: 0 })
    };
    let span = |par: &Par| {
        let file = *refs.places.get(&par.file)?;
        let Some(id) = &par.id else {
            return Some(chapter(file));
        };
        let key = format!("{}#{id}", par.file);
        let Some(start) = refs.places.get(&key).copied() else {
            return Some(chapter(file));
        };
        let next = Anchor {
            block: start.block + 1,
            offset: 0,
        };
        let end = refs.ends.get(&key).copied().filter(|e| *e > start);
        Some((start, end.unwrap_or(next)))
    };
    (pars.into_iter())
        .filter_map(|par| {
            let (start, end) = span(&par)?;
            Some(Phrase {
                start,
                end,
                audio: par.audio?,
                skips: par.skips,
                escape: par.escape,
            })
        })
        .collect()
}

/// A `seq` being read: what it marks and its escape number, if any.
struct Seq {
    skips: Skips,
    escape: Option<u32>,
}

/// Overlay `xml`'s pars in order; `escapes` numbers tables book-wide.
pub(crate) fn read(xml: &[u8], base: &str, escapes: &mut u32) -> Vec<Par> {
    let mut reader = Reader::from_reader(xml);
    let mut seqs: Vec<Seq> = Vec::new();
    let mut out = Vec::new();
    // The `par` open now: its own kinds, text and audio.
    let mut par: Option<(Skips, Option<String>, Option<Spoken>)> = None;
    while out.len() < MAX_PHRASES {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(e) => e,
        };
        let (start, empty) = match &event {
            Event::Start(e) => (Some(e), false),
            Event::Empty(e) => (Some(e), true),
            _ => (None, false),
        };
        if let Some(e) = start {
            match e.local_name().as_ref() {
                "seq" if !empty => {
                    let kinds = value(e, "type").unwrap_or_default();
                    let outer = seqs.last();
                    let mut escape = outer.and_then(|s| s.escape);
                    if escape.is_none() && escapable(&kinds) {
                        *escapes = escapes.saturating_add(1);
                        escape = Some(*escapes);
                    }
                    let inherited = outer.map_or(Skips::default(), |s| s.skips);
                    if seqs.len() < MAX_DEPTH {
                        seqs.push(Seq {
                            skips: Skips::of(&kinds).or(inherited),
                            escape,
                        });
                    }
                }
                "par" => {
                    let kinds = value(e, "type").unwrap_or_default();
                    par = (!empty).then(|| (Skips::of(&kinds), None, None));
                }
                "text" => {
                    if let Some((_, text, _)) = par.as_mut() {
                        *text = value(e, "src");
                    }
                }
                "audio" => {
                    if let Some((_, _, audio)) = par.as_mut() {
                        *audio = clip(e, base);
                    }
                }
                _ => {}
            }
        }
        let Event::End(e) = &event else {
            continue;
        };
        match e.local_name().as_ref() {
            "seq" => {
                seqs.pop();
            }
            "par" => {
                let Some((own, text, audio)) = par.take() else {
                    continue;
                };
                let Some((file, id)) = text.as_deref().and_then(|t| target(t, base)) else {
                    continue;
                };
                let outer = seqs.last();
                out.push(Par {
                    file,
                    id,
                    audio,
                    skips: own.or(outer.map_or(Skips::default(), |s| s.skips)),
                    escape: outer.and_then(|s| s.escape),
                });
            }
            _ => {}
        }
    }
    out
}

/// An `audio` element's file and span; `None` if it names no file.
fn clip(e: &BytesStart, base: &str) -> Option<Spoken> {
    let path = href::resolve(base, &value(e, "src")?)?;
    let begin = value(e, "clipBegin").and_then(|v| clock(&v)).unwrap_or(0.0);
    let end = value(e, "clipEnd").and_then(|v| clock(&v));
    Some(Spoken {
        path,
        begin,
        end: end.filter(|end| *end > begin),
    })
}

/// A `text` element's file and the id its fragment names.
fn target(src: &str, base: &str) -> Option<(String, Option<String>)> {
    let file = href::resolve(base, src)?;
    let fragment = src.split_once('#').map(|(_, f)| href::percent_decode(f));
    let id = fragment.and_then(|f| fragment_id(&f));
    Some((file, id))
}

/// The id a fragment names: plain, or as `xpointer(id('…'))`.
fn fragment_id(fragment: &str) -> Option<String> {
    let f = fragment.trim();
    if let Some(inner) = f
        .strip_prefix("xpointer(id(")
        .and_then(|r| r.strip_suffix("))"))
    {
        let id = inner.trim_matches(|c| c == '\'' || c == '"');
        return (!id.is_empty()).then(|| id.to_owned());
    }
    // Other schemes, such as `epubcfi(…)`, aren't followed.
    (!f.is_empty() && !f.contains('(')).then(|| f.to_owned())
}

/// A SMIL clock value in seconds: `h:mm:ss.f`, `mm:ss.f` or `2.5s`.
pub(crate) fn clock(value: &str) -> Option<f64> {
    let v = value.trim();
    let parts: Vec<&str> = v.split(':').collect();
    let number = |s: &str| -> Option<f64> {
        let ok = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'.');
        s.parse::<f64>().ok().filter(|n| ok && n.is_finite())
    };
    let seconds = match parts.as_slice() {
        [h, m, s] => number(h)? * 3600.0 + number(m)? * 60.0 + number(s)?,
        [m, s] => number(m)? * 60.0 + number(s)?,
        [count] => {
            let units = [("ms", 0.001), ("min", 60.0), ("h", 3600.0), ("s", 1.0)];
            let (digits, scale) = units
                .iter()
                .find_map(|(unit, scale)| count.strip_suffix(unit).map(|d| (d, *scale)))
                .unwrap_or((count, 1.0));
            number(digits.trim())? * scale
        }
        _ => return None,
    };
    (seconds >= 0.0 && seconds.is_finite()).then_some(seconds)
}

/// Attribute `name`'s value by its local name, unescaped.
fn value(e: &BytesStart, name: &str) -> Option<String> {
    let mut attributes = e.attributes();
    attributes.with_checks(false);
    let raw = attributes
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)?
        .value;
    let unescaped = quick_xml::escape::unescape(&raw).map(|v| v.into_owned());
    Some(unescaped.unwrap_or_else(|_| raw.into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrase(block: usize, path: &str, (begin, end): (f64, f64), skips: Skips) -> Phrase {
        Phrase {
            start: Anchor { block, offset: 0 },
            end: Anchor {
                block: block + 1,
                offset: 0,
            },
            audio: Spoken {
                path: path.into(),
                begin,
                end: Some(end),
            },
            skips,
            escape: None,
        }
    }

    fn told() -> Narration {
        let note = Skips {
            notes: true,
            ..Skips::default()
        };
        let plain = Skips::default();
        let mut phrases = vec![
            phrase(0, "a", (0.0, 1.0), plain),
            phrase(1, "a", (1.0, 2.0), plain),
            phrase(2, "a", (2.0, 2.5), note),
            phrase(3, "a", (2.5, 3.0), plain),
            phrase(4, "a", (3.0, 4.0), plain),
            phrase(5, "b", (4.0, 5.0), plain),
        ];
        phrases[3].escape = Some(1);
        phrases[4].escape = Some(1);
        Narration {
            phrases,
            ..Narration::default()
        }
    }

    #[test]
    fn touching_clips_read_as_one_run_and_skips_split_them() {
        let n = told();
        let all = n.runs(0, Skips::default());
        let spans: Vec<(&str, f64, Option<f64>)> = (all.iter())
            .map(|r| (r.path.as_str(), r.from, r.until))
            .collect();
        assert_eq!(spans, [("a", 0.0, Some(4.0)), ("b", 4.0, Some(5.0))]);
        assert_eq!(all[0].phrases, [0, 1, 2, 3, 4]);
        let skip = Skips {
            notes: true,
            ..Skips::default()
        };
        let runs = n.runs(1, skip);
        let phrases: Vec<&[usize]> = runs.iter().map(|r| r.phrases.as_slice()).collect();
        assert_eq!(phrases, [&[1][..], &[3, 4], &[5]]);
        assert_eq!(n.phrase_at(&all[0], 2.7), Some(3));
        assert_eq!(n.phrase_at(&all[0], 0.0), Some(0));
        assert_eq!(n.phrase_at(&all[0], 1.0), Some(1));
    }

    #[test]
    fn stepping_skips_what_is_left_out_and_escapes_leave_the_table() {
        let n = told();
        let skip = Skips {
            notes: true,
            ..Skips::default()
        };
        assert_eq!(n.next(1, skip), Some(3));
        assert_eq!(n.next(1, Skips::default()), Some(2));
        assert_eq!(n.previous(3, skip), Some(1));
        assert_eq!(n.next(5, skip), None);
        assert_eq!(n.escape(3, skip), Some(5));
        assert_eq!(n.escape(1, skip), None);
        let at = |block, offset| Anchor { block, offset };
        assert_eq!(n.from_anchor(at(2, 3), skip), Some(3));
        assert_eq!(n.from_anchor(at(0, 0), skip), Some(0));
        assert_eq!(n.from_anchor(at(9, 0), skip), None);
    }

    #[test]
    fn clock_values_read_in_every_form() {
        assert_eq!(clock("0:00:24.500"), Some(24.5));
        assert_eq!(clock("1:02:03"), Some(3723.0));
        assert_eq!(clock("02:30.25"), Some(150.25));
        assert_eq!(clock("3.2s"), Some(3.2));
        assert_eq!(clock("1.5min"), Some(90.0));
        assert_eq!(clock("250ms"), Some(0.25));
        assert_eq!(clock("2h"), Some(7200.0));
        assert_eq!(clock(" 12 "), Some(12.0));
        for bad in ["", "x", "-1s", "1:2:3:4", "1e3", "NaNs", "inf"] {
            assert_eq!(clock(bad), None, "{bad}");
        }
    }

    #[test]
    fn pars_name_their_text_and_audio_from_the_overlay_file() {
        let smil = r#"<smil xmlns="http://www.w3.org/ns/SMIL" xmlns:epub="http://www.idpf.org/2007/ops" version="3.0">
            <body><seq epub:textref="c1.xhtml" epub:type="chapter">
              <par id="p1"><text src="../text/c1.xhtml#s1"/>
                <audio src="../audio/a.mp3" clipBegin="0:00:01.000" clipEnd="0:00:02.500"/></par>
              <par><audio src="../audio/a.mp3" clipBegin="2.5s"/><text src="../text/c1.xhtml#xpointer(id('s2'))"/></par>
              <par><text src="../text/c1.xhtml"/></par>
              <par><audio src="../audio/a.mp3"/></par>
            </seq></body></smil>"#;
        let mut escapes = 0;
        let pars = read(smil.as_bytes(), "/OPS/smil/c1.smil", &mut escapes);
        assert_eq!(pars.len(), 3, "{pars:?}");
        assert_eq!(pars[0].file, "/OPS/text/c1.xhtml");
        assert_eq!(pars[0].id.as_deref(), Some("s1"));
        let a = pars[0].audio.as_ref().unwrap();
        assert_eq!(
            (a.path.as_str(), a.begin, a.end),
            ("/OPS/audio/a.mp3", 1.0, Some(2.5))
        );
        assert_eq!(pars[1].id.as_deref(), Some("s2"));
        assert_eq!(pars[1].audio.as_ref().unwrap().end, None);
        // A par with no audio is kept; one with no text isn't.
        assert_eq!(
            (pars[2].id.as_deref(), pars[2].audio.is_none()),
            (None, true)
        );
    }

    #[test]
    fn notes_and_page_numbers_can_be_skipped_and_tables_left() {
        let smil = r#"<smil><body>
            <par epub:type="pagebreak"><text src="c.xhtml#pg1"/><audio src="a.mp3"/></par>
            <seq epub:type="footnote"><par><text src="c.xhtml#n1"/><audio src="a.mp3"/></par></seq>
            <seq epub:type="table"><seq epub:type="table-row">
              <par><text src="c.xhtml#r1"/><audio src="a.mp3"/></par></seq>
              <par><text src="c.xhtml#r2"/><audio src="a.mp3"/></par></seq>
            <seq epub:type="list"><par><text src="c.xhtml#l1"/><audio src="a.mp3"/></par></seq>
            <par><text src="c.xhtml#after"/><audio src="a.mp3"/></par>
        </body></smil>"#;
        let mut escapes = 0;
        let pars = read(smil.as_bytes(), "/c.smil", &mut escapes);
        let skips: Vec<Skips> = pars.iter().map(|p| p.skips).collect();
        assert!(skips[0].page_numbers && !skips[0].notes);
        assert!(skips[1].notes);
        assert!(!skips[2].any() && !skips[5].any());
        let escape: Vec<Option<u32>> = pars.iter().map(|p| p.escape).collect();
        // The row is inside the table: both leave as one.
        assert_eq!(escape, [None, None, Some(1), Some(1), Some(2), None]);
        assert_eq!(escapes, 2);
    }
}
