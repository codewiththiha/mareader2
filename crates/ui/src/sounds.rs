//! How words sound on their cards, from a download voice models share.

use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, mpsc};

use egui::text::{LayoutJob, TextFormat};
use egui::{FontId, Ui};
use mareader_settings::{Accent, Notation, PronunciationSettings};
use mareader_speech::{Item, PRONUNCIATION, Shelf, Stage};

use crate::theme::{self, Tokens};

/// Lookups remembered; past this the list starts over.
const KEPT: usize = 256;
/// Seconds a loaded accent stays after its last word.
#[cfg(not(target_arch = "wasm32"))]
const IDLE: f64 = 120.0;
/// Longer selections get no pronunciation.
const MOST_WORDS: usize = 3;
const MOST_CHARS: usize = 40;

/// What the Vocabulary tab asks of the download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Download,
    Pause,
    Cancel,
    Remove,
}

/// An accent the data holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Way {
    American,
    British,
}

impl Way {
    fn tag(self) -> &'static str {
        match self {
            Self::American => "US",
            Self::British => "UK",
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn dialect(self) -> mareader_g2p::Dialect {
        match self {
            Self::American => mareader_g2p::Dialect::American,
            Self::British => mareader_g2p::Dialect::British,
        }
    }
}

/// One syllable as written, and whether it takes the stress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beat {
    pub text: String,
    pub stressed: bool,
}

/// One accent on a card: its tag, then each word's beats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// "US" or "UK"; empty when only one accent shows.
    pub tag: &'static str,
    pub words: Vec<Vec<Beat>>,
    /// IPA sits between slashes.
    pub slashed: bool,
}

/// What a card hears: the word read aloud, and how card words sound.
#[derive(Clone, Copy, Default)]
pub struct Heard<'a> {
    pub saying: Option<&'a str>,
    pub sounds: &'a [(String, Vec<Line>)],
}

impl Heard<'_> {
    pub fn saying(&self, word: &str) -> bool {
        self.saying == Some(word)
    }

    pub fn lines(&self, word: &str) -> &[Line] {
        let found = self.sounds.iter().find(|(w, _)| w == word);
        found.map_or(&[], |(_, lines)| lines.as_slice())
    }
}

type Key = (String, Way);

#[cfg(not(target_arch = "wasm32"))]
type Answer = Result<(String, Arc<mareader_g2p::G2p>), String>;

/// A lookup away from the frame.
#[cfg(not(target_arch = "wasm32"))]
struct Asking {
    key: Key,
    answer: mpsc::Receiver<Answer>,
}

/// The pronunciation download and the words looked up in it.
#[derive(Default)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct Sounds {
    shelf: Shelf,
    /// Each word's phonemes per accent; None when nothing could say it.
    found: HashMap<Key, Option<String>>,
    wanted: Vec<Key>,
    /// Lookups run on the frame, for tests.
    inline: bool,
    #[cfg(not(target_arch = "wasm32"))]
    lexicons: Option<mareader_speech::Lexicons>,
    #[cfg(not(target_arch = "wasm32"))]
    waker: Option<mareader_speech::Waker>,
    #[cfg(not(target_arch = "wasm32"))]
    asking: Option<Asking>,
    /// Loaded accents, kept while words come; dropped when idle.
    #[cfg(not(target_arch = "wasm32"))]
    held: Vec<Arc<mareader_g2p::G2p>>,
    used: f64,
}

impl Sounds {
    /// Keeps the download beside the voice models in `dir`, sharing files.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach(
        &mut self,
        dir: PathBuf,
        waker: mareader_speech::Waker,
        voices: &mut Shelf,
    ) -> Result<(), String> {
        voices.set_neighbours(mareader_speech::AIDS);
        self.shelf
            .attach(dir, mareader_speech::AIDS, waker.clone())?;
        self.shelf.set_neighbours(voices.catalogue());
        self.lexicons = Some(voices.lexicons());
        self.waker = Some(waker);
        self.shelf.adopt();
        Ok(())
    }

    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
    }

    pub fn attached(&self) -> bool {
        self.shelf.attached()
    }

    pub fn stage(&self) -> &Stage {
        self.shelf.stage(Item::Model(PRONUNCIATION.id))
    }

    pub fn ready(&self) -> bool {
        *self.stage() == Stage::Ready
    }

    /// The download's size.
    pub fn bytes(&self) -> u64 {
        PRONUNCIATION.bytes()
    }

    /// Voice models holding the same files; removing the pack frees nothing.
    pub fn shared_with(&self) -> Vec<&'static str> {
        let holders = self.shelf.holders(&PRONUNCIATION);
        holders.iter().map(|m| m.name).collect()
    }

    pub fn act(&mut self, action: Action) -> Result<(), String> {
        let item = Item::Model(PRONUNCIATION.id);
        match action {
            Action::Download => self.shelf.download(item),
            Action::Pause => self.shelf.pause(item),
            Action::Cancel => self.shelf.cancel(item),
            Action::Remove => {
                self.forget();
                return self.shelf.remove(item);
            }
        }
        Ok(())
    }

    /// A voice model came or went: the pack takes in what it holds.
    pub fn voices_changed(&mut self) {
        self.shelf.adopt();
    }

    /// Takes in downloads and answers; true when something changed.
    pub fn poll(&mut self, now: f64) -> bool {
        let mut changed = self.shelf.poll();
        #[cfg(not(target_arch = "wasm32"))]
        {
            changed |= self.receive();
            self.pump();
            if self.asking.is_none() && now - self.used > IDLE {
                self.held.clear();
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = now;
        changed
    }

    fn forget(&mut self) {
        self.found.clear();
        self.wanted.clear();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.asking = None;
            self.held.clear();
        }
    }

    /// How `word` sounds as `settings` asks, once it has been looked up.
    pub fn lines(
        &mut self,
        word: &str,
        settings: &PronunciationSettings,
        now: f64,
    ) -> Option<Vec<Line>> {
        let word = word.trim();
        if !settings.shown || !self.ready() || !speakable(word) {
            return None;
        }
        let ways: &[Way] = match settings.accent {
            Accent::Both => &[Way::American, Way::British],
            Accent::American => &[Way::American],
            Accent::British => &[Way::British],
        };
        let mut lines = Vec::new();
        for &way in ways {
            let key = (word.to_owned(), way);
            match self.found.get(&key) {
                Some(Some(phonemes)) => {
                    let mut line = format(phonemes, way, settings.notation);
                    line.tag = if ways.len() > 1 { way.tag() } else { "" };
                    lines.push(line);
                }
                Some(None) => {}
                None => self.want(key, now),
            }
        }
        (!lines.is_empty()).then_some(lines)
    }

    fn want(&mut self, key: Key, now: f64) {
        self.used = now;
        #[cfg(not(target_arch = "wasm32"))]
        let busy = self.asking.as_ref().is_some_and(|a| a.key == key);
        #[cfg(target_arch = "wasm32")]
        let busy = false;
        if !busy && !self.wanted.contains(&key) {
            self.wanted.push(key);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.pump();
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn keep(&mut self, key: Key, phonemes: Option<String>) {
        if self.found.len() >= KEPT {
            self.found.clear();
        }
        self.found.insert(key, phonemes);
    }

    /// Starts the next lookup when none runs.
    #[cfg(not(target_arch = "wasm32"))]
    fn pump(&mut self) {
        while self.asking.is_none() && !self.wanted.is_empty() {
            let key = self.wanted.remove(0);
            let (Some(lexicons), Some(dir)) =
                (self.lexicons.clone(), self.shelf.folder(&PRONUNCIATION))
            else {
                self.keep(key, None);
                continue;
            };
            let (tx, rx) = mpsc::channel();
            let (word, dialect) = (key.0.clone(), key.1.dialect());
            let job = move || {
                let g2p = lexicons.load(&dir, dialect)?;
                Ok((g2p.phonemize(&word), g2p))
            };
            self.asking = Some(Asking { key, answer: rx });
            if self.inline {
                let _ = tx.send(job());
                self.receive();
                continue;
            }
            let waker = self.waker.clone();
            std::thread::spawn(move || {
                let _ = tx.send(job());
                if let Some(wake) = waker {
                    wake();
                }
            });
        }
    }

    /// Takes in a finished lookup.
    #[cfg(not(target_arch = "wasm32"))]
    fn receive(&mut self) -> bool {
        let Some(asking) = &self.asking else {
            return false;
        };
        let answer = match asking.answer.try_recv() {
            Ok(answer) => answer,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err("stopped".into()),
        };
        let key = asking.key.clone();
        self.asking = None;
        let phonemes = match answer {
            Ok((said, g2p)) => {
                if !self.held.iter().any(|h| Arc::ptr_eq(h, &g2p)) {
                    self.held.push(g2p);
                }
                understood(&said)
            }
            // Files that won't load sound out nothing.
            Err(_) => None,
        };
        self.keep(key, phonemes);
        true
    }
}

/// Whether `word` is English short enough to sound out.
fn speakable(word: &str) -> bool {
    let fits = |c: char| c.is_ascii_alphabetic() || matches!(c, '\'' | '-' | ' ');
    !word.is_empty()
        && word.len() <= MOST_CHARS
        && word.split_whitespace().count() <= MOST_WORDS
        && word.chars().any(|c| c.is_ascii_alphabetic())
        && word.chars().all(fits)
}

/// The phonemes, or None where a word wasn't known.
#[cfg(not(target_arch = "wasm32"))]
fn understood(phonemes: &str) -> Option<String> {
    let said = phonemes.trim();
    let known = !said.is_empty() && !said.contains(mareader_g2p::UNKNOWN);
    known.then(|| said.to_owned())
}

/// `phonemes` written as `notation` asks.
#[cfg(not(target_arch = "wasm32"))]
pub fn format(phonemes: &str, way: Way, notation: Notation) -> Line {
    use mareader_g2p::notation;
    let dialect = way.dialect();
    let letters = |w: &str| w.chars().filter(|c| c.is_alphabetic()).collect();
    let spoken = phonemes.split_whitespace().map(letters);
    let words = spoken.filter(|w: &String| !w.is_empty()).map(|w| {
        let one = |text| {
            vec![Beat {
                text,
                stressed: false,
            }]
        };
        match notation {
            Notation::Ipa => one(notation::ipa(&w, dialect)),
            Notation::Raw => one(notation::raw(&w)),
            Notation::Easy => notation::easy(&w, dialect)
                .into_iter()
                .map(|b| Beat {
                    text: b.text,
                    stressed: b.stressed,
                })
                .collect(),
        }
    });
    Line {
        tag: "",
        words: words.collect(),
        slashed: notation != Notation::Easy,
    }
}

#[cfg(target_arch = "wasm32")]
pub fn format(_: &str, _: Way, notation: Notation) -> Line {
    Line {
        tag: "",
        words: Vec::new(),
        slashed: notation != Notation::Easy,
    }
}

/// A card's sound lines, small and muted under its word.
pub fn show(ui: &mut Ui, lines: &[Line], t: &Tokens) {
    if lines.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for line in lines {
            ui.label(layout(line, t));
        }
    });
}

fn layout(line: &Line, t: &Tokens) -> LayoutJob {
    let size = t.text_small + 1.0;
    let plain = TextFormat::simple(FontId::proportional(size), t.ink);
    let strong = TextFormat::simple(theme::medium(size), t.ink);
    let quiet = TextFormat::simple(FontId::proportional(size), t.muted);
    let tag = TextFormat::simple(theme::medium(t.text_small - 1.0), t.muted);
    let mut job = LayoutJob::default();
    if !line.tag.is_empty() {
        job.append(line.tag, 0.0, tag);
        job.append(" ", 0.0, quiet.clone());
    }
    let slash = if line.slashed { "/" } else { "" };
    job.append(slash, 0.0, quiet.clone());
    for (i, word) in line.words.iter().enumerate() {
        if i > 0 {
            job.append(" ", 0.0, plain.clone());
        }
        for (j, beat) in word.iter().enumerate() {
            if j > 0 {
                job.append("·", 0.0, quiet.clone());
            }
            let look = if beat.stressed { &strong } else { &plain };
            job.append(&beat.text, 0.0, look.clone());
        }
    }
    job.append(slash, 0.0, quiet);
    job
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn text(line: &Line) -> String {
        let word = |w: &Vec<Beat>| {
            let beat = |b: &Beat| match b.stressed {
                true => b.text.to_uppercase(),
                false => b.text.clone(),
            };
            w.iter().map(beat).collect::<Vec<_>>().join("·")
        };
        line.words.iter().map(word).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn each_notation_writes_the_phonemes_its_way() {
        let us = |n| text(&format("jˈɑt", Way::American, n));
        assert_eq!(us(Notation::Ipa), "jɑːt");
        assert_eq!(us(Notation::Easy), "yaat");
        assert_eq!(us(Notation::Raw), "jˈɑt");
        let two = format("ɡˈʊd mˈɔɹnɪŋ,", Way::American, Notation::Easy);
        assert_eq!(text(&two), "gud MOR·ning");
        assert!(!two.slashed);
        assert!(format("jˈɒt", Way::British, Notation::Ipa).slashed);
    }

    #[test]
    fn only_short_english_is_sounded_out_and_unknowns_show_nothing() {
        assert!(speakable("yacht"));
        assert!(speakable("give up"));
        assert!(speakable("don't"));
        assert!(!speakable("မင်္ဂလာ"));
        assert!(!speakable("1984"));
        assert!(!speakable("one two three four"));
        assert!(!speakable(""));
        assert_eq!(understood(" jˈɑt "), Some("jˈɑt".to_owned()));
        assert_eq!(understood("ʤ❓"), None);
        assert_eq!(understood(""), None);
    }

    #[test]
    fn cards_find_their_words_lines() {
        let line = format("jˈɑt", Way::American, Notation::Easy);
        let sounds = [("yacht".to_owned(), vec![line.clone()])];
        let heard = Heard {
            saying: Some("yacht"),
            sounds: &sounds,
        };
        assert!(heard.saying("yacht"));
        assert_eq!(heard.lines("yacht"), [line]);
        assert!(heard.lines("boat").is_empty());
    }

    #[test]
    fn nothing_shows_before_the_pack_is_in() {
        let mut sounds = Sounds::default();
        let shown = PronunciationSettings::default();
        assert_eq!(sounds.lines("yacht", &shown, 0.0), None);
        assert!(sounds.wanted.is_empty());
    }
}
