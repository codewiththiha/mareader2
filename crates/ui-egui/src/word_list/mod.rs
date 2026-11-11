//! Word lists of picked pages: read, levelled, looked up, set out.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::Context;
use mareader_dict::{Found, Ticket};
use mareader_session::Document;
use mareader_settings::{WordList, WordOutput};

use crate::dict::Dict;
use crate::toast::Toast;
use crate::vocab::Vocab;

pub mod build;
pub mod preview;
pub mod window;

pub use build::{Page, Sheet, Word};

/// Pages this few, or a book's, are read in the frame.
const INLINE: usize = 4;
/// Keys asked for levels at a time; the level cache holds 8,192.
const LEVEL_ROUND: usize = 2_000;
/// Words asked of the dictionaries at a time, so cards still answer.
const DICT_ROUND: usize = 48;

/// What to make a list of.
pub struct Ask {
    pub document: Arc<Document>,
    pub title: String,
    /// The picked pages as shown, in reading order.
    pub pages: Vec<usize>,
    pub level: u8,
    pub list: WordList,
}

/// A finished list.
#[derive(Clone, Debug, Default)]
pub struct Made {
    pub markdown: String,
    pub words: usize,
    /// Languages asked for that have no dictionary.
    pub missing: Vec<String>,
}

/// Where a list has got to.
#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    Reading,
    /// Word levels: keys answered of all.
    Levels(usize, usize),
    /// Meanings: words looked up of all.
    Meanings(usize, usize),
    Ready,
}

impl Progress {
    /// What the preview says while the list is made.
    pub fn said(&self) -> String {
        match self {
            Self::Reading => "Reading the pages…".into(),
            Self::Levels(..) => "Finding the hard words…".into(),
            Self::Meanings(done, all) => format!("Looking up meanings · {done} of {all}"),
            Self::Ready => String::new(),
        }
    }

    /// How far along, 0 to 1.
    pub fn fraction(&self) -> f32 {
        let part = |done: usize, all: usize| done as f32 / all.max(1) as f32;
        match self {
            Self::Reading => 0.05,
            Self::Levels(done, all) => 0.1 + 0.4 * part(*done, *all),
            Self::Meanings(done, all) => 0.5 + 0.5 * part(*done, *all),
            Self::Ready => 1.0,
        }
    }
}

enum Stage {
    Reading(Receiver<Vec<Page>>),
    Levels { next: usize },
    Dictionary { next: usize, ticket: Option<Ticket> },
    Ready,
}

/// One list's pages, levels and meanings, gathered step by step.
pub struct Gather {
    title: String,
    level: u8,
    list: WordList,
    stage: Stage,
    pages: Vec<Page>,
    keys: Vec<String>,
    bands: HashMap<String, u8>,
    words: Vec<Word>,
    /// The languages and near words the meanings were asked with.
    asked: (Vec<String>, bool),
    found: HashMap<(String, String), Found>,
}

/// The one quick list being made; a newer one replaces it.
#[derive(Default)]
pub struct WordLists {
    job: Option<Gather>,
}

impl WordLists {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Starts on `ask`; `poll` carries it through.
    pub fn start(&mut self, ctx: &Context, ask: Ask) {
        self.job = Some(Gather::new(ctx, ask));
    }

    /// Moves the list on; done, the list and its options, or a toast.
    pub fn poll(
        &mut self,
        vocab: &mut Vocab,
        dict: &mut Dict,
    ) -> Option<Result<(Made, WordList), Toast>> {
        let job = self.job.as_mut()?;
        let done = match job.step(vocab, dict) {
            Ok(Progress::Ready) => Ok((job.made(), job.list.clone())),
            Ok(_) => return None,
            Err(toast) => Err(toast),
        };
        self.job = None;
        Some(done)
    }
}

impl Gather {
    pub fn new(ctx: &Context, ask: Ask) -> Self {
        let (tx, rx) = channel();
        let inline = ask.pages.len() <= INLINE || ask.document.is_reflowable();
        let ctx = ctx.clone();
        let (document, pages) = (ask.document, ask.pages);
        let work = move || {
            let _ = tx.send(read(&document, &pages));
            ctx.request_repaint();
        };
        if inline || cfg!(target_arch = "wasm32") {
            work();
        } else {
            let named = std::thread::Builder::new().name("word-list".into());
            // Unspawned, the list never comes; the reader still works.
            let _ = named.spawn(work);
        }
        Self {
            title: ask.title,
            level: ask.level,
            list: ask.list,
            stage: Stage::Reading(rx),
            pages: Vec::new(),
            keys: Vec::new(),
            bands: HashMap::new(),
            words: Vec::new(),
            asked: (Vec::new(), false),
            found: HashMap::new(),
        }
    }

    pub fn list(&self) -> &WordList {
        &self.list
    }

    /// New options: set out again, or asked again for new meanings.
    pub fn set_list(&mut self, list: WordList) {
        let more_languages = list.languages.iter().any(|l| !self.asked.0.contains(l));
        let more_near = list.similar && !self.asked.1;
        self.list = list;
        if matches!(self.stage, Stage::Ready) && (more_languages || more_near) {
            self.stage = Stage::Dictionary {
                next: 0,
                ticket: None,
            };
            self.asked = (Vec::new(), false);
        }
    }

    /// Moves on as far as answers allow; a toast if it can't go on.
    pub fn step(&mut self, vocab: &mut Vocab, dict: &mut Dict) -> Result<Progress, Toast> {
        loop {
            match &mut self.stage {
                Stage::Reading(rx) => match rx.try_recv() {
                    Ok(pages) => {
                        self.keys = build::keys(&pages);
                        self.pages = pages;
                        self.stage = Stage::Levels { next: 0 };
                    }
                    Err(TryRecvError::Empty) => return Ok(Progress::Reading),
                    Err(TryRecvError::Disconnected) => {
                        return Err(Toast::info("Couldn't read those pages"));
                    }
                },
                Stage::Levels { next } => {
                    if vocab.coming() {
                        return Ok(Progress::Levels(0, self.keys.len()));
                    }
                    if !vocab.ready() {
                        let why =
                            "Word levels aren't downloaded yet. Get them in Settings › Vocabulary.";
                        return Err(Toast::info(why));
                    }
                    let round = self.keys.get(*next..).unwrap_or_default();
                    let round = round
                        .get(..LEVEL_ROUND.min(round.len()))
                        .unwrap_or_default();
                    if round.is_empty() {
                        self.words = build::hard(&self.pages, &self.bands, self.level);
                        if self.words.is_empty() {
                            let level = mareader_cefr::label(self.level);
                            let why = format!("No words above {level} on these pages");
                            return Err(Toast::info(why));
                        }
                        self.stage = Stage::Dictionary {
                            next: 0,
                            ticket: None,
                        };
                        continue;
                    }
                    let bands = vocab.bands(round);
                    let mut all = true;
                    for (key, band) in round.iter().zip(bands) {
                        match band {
                            Some(b) => {
                                self.bands.insert(key.clone(), b);
                            }
                            None => all = false,
                        }
                    }
                    if !all {
                        return Ok(Progress::Levels(*next, self.keys.len()));
                    }
                    *next += round.len();
                }
                Stage::Dictionary { next, ticket } => {
                    if self.asked.0.is_empty() && *next == 0 && ticket.is_none() {
                        let ready = dict.languages();
                        let wanted = self.list.languages.iter();
                        let usable = wanted.filter(|l| ready.contains(&l.as_str())).cloned();
                        self.asked = (usable.collect(), self.list.similar);
                    }
                    let all = self.words.len();
                    if let Some(t) = *ticket {
                        let Some(reply) = dict.take_words(t) else {
                            return Ok(Progress::Meanings(*next, all));
                        };
                        for found in reply.found {
                            self.found
                                .insert((found.word.clone(), found.to.clone()), found);
                        }
                        *next += DICT_ROUND;
                        *ticket = None;
                    }
                    let round = self.words.get(*next..).unwrap_or_default();
                    if self.asked.0.is_empty() || round.is_empty() {
                        self.stage = Stage::Ready;
                        return Ok(Progress::Ready);
                    }
                    let words = round.iter().take(DICT_ROUND).map(|w| w.key.clone());
                    let (languages, near) = (self.asked.0.clone(), self.asked.1);
                    *ticket = Some(dict.ask_words(words.collect(), languages, near));
                    return Ok(Progress::Meanings((*next).min(all), all));
                }
                Stage::Ready => return Ok(Progress::Ready),
            }
        }
    }

    /// The list as it stands, with the options set now.
    pub fn made(&self) -> Made {
        let first = self.pages.first().map_or(0, |p| p.page);
        let last = self.pages.last().map_or(first, |p| p.page);
        let wanted = self.list.languages.iter();
        let languages: Vec<String> = wanted
            .filter(|l| self.asked.0.contains(l))
            .cloned()
            .collect();
        let mut list = self.list.clone();
        list.similar &= self.asked.1;
        let sheet = Sheet {
            title: &self.title,
            level: self.level,
            words: &self.words,
            found: &self.found,
            languages: &languages,
            pages: (first, last),
        };
        let asked = self.list.languages.iter();
        let missing = asked.filter(|l| !languages.contains(l)).cloned();
        Made {
            markdown: build::render(&sheet, &list),
            words: self.words.len(),
            missing: missing.collect(),
        }
    }
}

/// Copies or saves `made`; a toast for now, as saving toasts later.
pub fn deliver(
    ctx: &Context,
    files: &crate::files::Files,
    title: &str,
    made: &Made,
    output: WordOutput,
) -> Option<Toast> {
    let stem = file_stem(title);
    match output {
        WordOutput::Copy => {
            ctx.copy_text(made.markdown.clone());
            Some(Toast::info(copied(made)))
        }
        WordOutput::Markdown => {
            let name = format!("{stem}.md");
            let bytes = made.markdown.clone().into_bytes();
            files.save_as(ctx, ("Save word list", name), ("Markdown", "md"), bytes);
            None
        }
        WordOutput::Epub => {
            let name = format!("{stem}.epub");
            let none = |_: &str| None;
            let format = mareader_epub::Format::Markdown;
            match mareader_epub::convert(made.markdown.as_bytes(), format, Some(&name), &none) {
                Ok(bytes) => {
                    files.save_as(ctx, ("Save word list", name), ("EPUB", "epub"), bytes);
                    None
                }
                Err(e) => Some(Toast::error(format!("Couldn't make the EPUB: {e}"))),
            }
        }
    }
}

/// A file name for a book's word list, safe on every system.
pub fn file_stem(title: &str) -> String {
    let bad = |c: char| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|');
    let clean: String = title
        .chars()
        .map(|c| if bad(c) { ' ' } else { c })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    match clean.is_empty() {
        true => "Word list".into(),
        false => format!("{clean} - words"),
    }
}

/// What the toast says once a list is copied.
pub fn copied(made: &Made) -> String {
    let mut said = match made.words {
        1 => "Copied a list of 1 word".to_owned(),
        n => format!("Copied a list of {n} words"),
    };
    if !made.missing.is_empty() {
        let names: Vec<String> = made
            .missing
            .iter()
            .map(|l| mareader_dict::language(l))
            .collect();
        let names = names.join(" or ");
        said.push_str(&format!(
            ". No {names} dictionary yet: add it in Settings › Dictionary."
        ));
    }
    said
}

/// The picked pages' text, each own page once, with its chapter.
fn read(document: &Document, pages: &[usize]) -> Vec<Page> {
    let outline = document.outline();
    let top = outline.iter().map(|e| e.depth).min();
    let chapters: Vec<_> = outline.iter().filter(|e| Some(e.depth) == top).collect();
    let mut out: Vec<Page> = Vec::new();
    for &shown in pages {
        let page = document.to_base(shown);
        if out.iter().any(|p| p.page == page) {
            continue;
        }
        let Ok(layer) = document.text_layer(shown) else {
            continue;
        };
        let (text, _) = crate::study::words::read(&layer);
        let mut before = chapters.iter().filter(|e| e.page <= shown);
        let chapter = before.next_back().map(|e| e.title.trim().to_owned());
        let text = text.into_iter().collect();
        out.push(Page {
            page,
            chapter,
            text,
        });
    }
    out
}
