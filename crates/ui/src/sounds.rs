//! How words sound on their cards, from a download voice models share.

use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, mpsc};

use egui::text::{LayoutJob, TextFormat};
use egui::{FontId, Ui};
use mareader_g2p::Dialect;
pub use mareader_g2p::notation::Beat;
use mareader_settings::{Accent, Notation, PronunciationSettings};
use mareader_speech::{Item, PRONUNCIATION, Shelf, Stage};

use crate::command::Download;
use crate::theme::{self, Tokens};

/// Lookups remembered; past this the list starts over.
const KEPT: usize = 256;
/// Seconds a loaded accent stays after its last word.
#[cfg(not(target_arch = "wasm32"))]
const IDLE: f64 = 120.0;
/// Longer selections get no pronunciation.
const MOST_WORDS: usize = 3;
const MOST_CHARS: usize = 40;

/// The tag a line wears when both accents show.
fn tag(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::American => "US",
        Dialect::British => "UK",
    }
}

/// The tag a stand-in accent wears, as `≈ UK`.
fn near_tag(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::American => "≈ US",
        Dialect::British => "≈ UK",
    }
}

/// The accent's name, as people say it.
pub fn named(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::American => "American",
        Dialect::British => "British",
    }
}

/// One accent on a card: its tag, then each word's beats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// "US", "UK" or "≈ UK"; empty when the voice's own accent shows.
    pub tag: &'static str,
    pub words: Vec<Vec<Beat>>,
    /// IPA sits between slashes.
    pub slashed: bool,
    /// Why a stand-in accent shows, told on hover.
    pub why: Option<String>,
}

/// The voice that says English words on cards, as `Emma`, `en-GB`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnglishVoice {
    pub name: String,
    pub tag: String,
}

/// Which English cards write, from the voice's language tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spoken {
    /// American or British: cards write just that.
    Exactly(Dialect),
    /// Another English, as `Australian`: the nearer of the two.
    Nearest {
        dialect: Dialect,
        english: &'static str,
    },
    /// No English voice, or one not saying which: both show.
    Unsure,
}

/// Englishes without a lexicon of their own, and their nearer accent.
const ENGLISHES: [(&str, &str, Dialect); 15] = [
    ("AU", "Australian", Dialect::British),
    ("NZ", "New Zealand", Dialect::British),
    ("IE", "Irish", Dialect::British),
    ("ZA", "South African", Dialect::British),
    ("IN", "Indian", Dialect::British),
    ("SG", "Singaporean", Dialect::British),
    ("HK", "Hong Kong", Dialect::British),
    ("MY", "Malaysian", Dialect::British),
    ("NG", "Nigerian", Dialect::British),
    ("KE", "Kenyan", Dialect::British),
    ("TZ", "Tanzanian", Dialect::British),
    ("PK", "Pakistani", Dialect::British),
    ("JM", "Jamaican", Dialect::British),
    ("CA", "Canadian", Dialect::American),
    ("PH", "Philippine", Dialect::American),
];

/// What a voice tagged `tag`, as `en-AU`, means for the cards.
pub fn spoken(tag: Option<&str>) -> Spoken {
    let Some(tag) = tag else {
        return Spoken::Unsure;
    };
    let mut parts = tag.split(['-', '_']);
    if !parts.next().is_some_and(|p| p.eq_ignore_ascii_case("en")) {
        return Spoken::Unsure;
    }
    // Past a script, as `Latn`, to the region: two letters.
    let region = parts.find(|p| p.len() != 4).filter(|p| p.len() == 2);
    let Some(region) = region.map(str::to_ascii_uppercase) else {
        return Spoken::Unsure;
    };
    match region.as_str() {
        "US" => Spoken::Exactly(Dialect::American),
        "GB" | "UK" => Spoken::Exactly(Dialect::British),
        _ => match ENGLISHES.iter().find(|(r, ..)| *r == region) {
            Some(&(_, english, dialect)) => Spoken::Nearest { dialect, english },
            None => Spoken::Unsure,
        },
    }
}

/// The accents a card writes, as `settings` and the voice ask.
pub fn shown(settings: &PronunciationSettings, spoken: Spoken) -> &'static [Dialect] {
    const BOTH: &[Dialect] = &[Dialect::American, Dialect::British];
    let one = |d: Dialect| -> &'static [Dialect] {
        match d {
            Dialect::American => &[Dialect::American],
            Dialect::British => &[Dialect::British],
        }
    };
    if !settings.follow_voice {
        return match settings.accent {
            Accent::Both => BOTH,
            Accent::American => one(Dialect::American),
            Accent::British => one(Dialect::British),
        };
    }
    match spoken {
        Spoken::Exactly(d) | Spoken::Nearest { dialect: d, .. } => one(d),
        Spoken::Unsure => BOTH,
    }
}

/// The accent picker's value while the voice decides.
pub fn accent(settings: &PronunciationSettings, spoken: Spoken) -> Accent {
    match shown(settings, spoken) {
        [Dialect::American] => Accent::American,
        [Dialect::British] => Accent::British,
        _ => Accent::Both,
    }
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

type Key = (String, Dialect);

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
        self.shelf
            .attach(dir, mareader_speech::AIDS, waker.clone())?;
        self.shelf.share_with(voices);
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

    pub fn act(&mut self, action: Download) -> Result<(), String> {
        let item = Item::Model(PRONUNCIATION.id);
        #[cfg(not(target_arch = "wasm32"))]
        match action {
            Download::Start => self.shelf.download(item),
            Download::Pause => self.shelf.pause(item),
            Download::Cancel => self.shelf.cancel(item),
            Download::Remove => {
                self.forget();
                return self.shelf.remove(item);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (item, action);
        Ok(())
    }

    /// A voice model came or went: the pack takes in what it holds.
    pub fn voices_changed(&mut self) {
        self.shelf.adopt();
    }

    /// Takes in downloads and answers; true when something changed.
    pub fn poll(&mut self, now: f64) -> bool {
        let changed = self.shelf.poll();
        #[cfg(not(target_arch = "wasm32"))]
        let changed = {
            let answered = self.receive();
            self.pump();
            if self.asking.is_none() && now - self.used > IDLE {
                self.held.clear();
            }
            changed | answered
        };
        #[cfg(target_arch = "wasm32")]
        let _ = now;
        changed
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
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
        (settings, voice): (&PronunciationSettings, Spoken),
        now: f64,
    ) -> Option<Vec<Line>> {
        let word = word.trim();
        if !settings.shown || !self.ready() || !speakable(word) {
            return None;
        }
        let dialects = shown(settings, voice);
        let near = match voice {
            Spoken::Nearest { english, .. } if settings.follow_voice => Some(english),
            _ => None,
        };
        let mut lines = Vec::new();
        for &dialect in dialects {
            let key = (word.to_owned(), dialect);
            match self.found.get(&key) {
                Some(Some(phonemes)) => {
                    let mut line = format(phonemes, dialect, settings.notation);
                    line.tag = match near {
                        _ if dialects.len() > 1 => tag(dialect),
                        Some(_) => near_tag(dialect),
                        None => "",
                    };
                    line.why = near.map(|english| stand_in(english, dialect));
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
            let (word, dialect) = (key.0.clone(), key.1);
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

/// Why `dialect` stands in for the voice's `english`.
pub fn stand_in(english: &str, dialect: Dialect) -> String {
    let dialect = named(dialect);
    format!(
        "Your voice speaks {english} English. Cards know only American \
         and British, so this is {dialect}, the nearer."
    )
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
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn understood(phonemes: &str) -> Option<String> {
    let said = phonemes.trim();
    let known = !said.is_empty() && !said.contains(mareader_g2p::UNKNOWN);
    known.then(|| said.to_owned())
}

/// `phonemes` written as `notation` asks.
pub fn format(phonemes: &str, dialect: Dialect, notation: Notation) -> Line {
    use mareader_g2p::notation;
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
            Notation::Easy => notation::easy(&w, dialect),
        }
    });
    Line {
        tag: "",
        words: words.collect(),
        slashed: notation != Notation::Easy,
        why: None,
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
            let label = ui.label(layout(line, t));
            if let Some(why) = &line.why {
                crate::widgets::tip(&label, why, None);
            }
        }
    });
}

fn layout(line: &Line, t: &Tokens) -> LayoutJob {
    let size = t.text_small + 1.0;
    let plain = TextFormat::simple(FontId::proportional(size), t.ink);
    let strong = TextFormat::simple(theme::medium(size), t.ink);
    let quiet = TextFormat::simple(FontId::proportional(size), t.muted);
    // A stand-in accent's tag is amber, as its settings row warns.
    let tone = if line.why.is_some() {
        theme::caution(t)
    } else {
        t.muted
    };
    let tag = TextFormat::simple(theme::medium(t.text_small - 1.0), tone);
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
        let us = |n| text(&format("jˈɑt", Dialect::American, n));
        assert_eq!(us(Notation::Ipa), "jɑːt");
        assert_eq!(us(Notation::Easy), "yaat");
        assert_eq!(us(Notation::Raw), "jˈɑt");
        let two = format("ɡˈʊd mˈɔɹnɪŋ,", Dialect::American, Notation::Easy);
        assert_eq!(text(&two), "gud MOR·ning");
        assert!(!two.slashed);
        assert!(format("jˈɒt", Dialect::British, Notation::Ipa).slashed);
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
        let line = format("jˈɑt", Dialect::American, Notation::Easy);
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
    fn a_voice_s_tag_picks_its_english_or_the_nearest() {
        use Dialect::{American, British};
        let tag = |t| spoken(Some(t));
        assert_eq!(tag("en-US"), Spoken::Exactly(American));
        assert_eq!(tag("en_GB"), Spoken::Exactly(British));
        assert_eq!(tag("en-Latn-GB"), Spoken::Exactly(British));
        assert_eq!(tag("en-GB-scotland"), Spoken::Exactly(British));
        let au = Spoken::Nearest {
            dialect: British,
            english: "Australian",
        };
        assert_eq!(tag("en-au"), au);
        let ca = Spoken::Nearest {
            dialect: American,
            english: "Canadian",
        };
        assert_eq!(tag("en-CA"), ca);
        for unsure in ["en", "en-001", "en-XX", "fr-FR", "es"] {
            assert_eq!(tag(unsure), Spoken::Unsure, "{unsure}");
        }
        assert_eq!(spoken(None), Spoken::Unsure);
    }

    #[test]
    fn the_voice_decides_unless_told_not_to() {
        use Dialect::{American, British};
        let mut s = PronunciationSettings::default();
        assert_eq!(shown(&s, Spoken::Exactly(British)), [British]);
        let au = spoken(Some("en-AU"));
        assert_eq!(shown(&s, au), [British]);
        assert_eq!(shown(&s, Spoken::Unsure), [American, British]);
        assert_eq!(accent(&s, Spoken::Exactly(American)), Accent::American);
        s.follow_voice = false;
        s.accent = Accent::American;
        assert_eq!(shown(&s, Spoken::Exactly(British)), [American]);
        let why = stand_in("Australian", British);
        assert!(why.contains("Australian English") && why.contains("British"));
    }

    #[test]
    fn nothing_shows_before_the_pack_is_in() {
        let mut sounds = Sounds::default();
        let shown = PronunciationSettings::default();
        let unsure = (&shown, Spoken::Unsure);
        assert_eq!(sounds.lines("yacht", unsure, 0.0), None);
        assert!(sounds.wanted.is_empty());
    }
}
