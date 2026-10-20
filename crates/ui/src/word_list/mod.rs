//! Word lists of picked pages: read, levelled, looked up, set out.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::Context;
use mareader_dict::{Found, Reply, Ticket};
use mareader_session::Document;
use mareader_settings::WordList;

use crate::dict::Dict;
use crate::toast::Toast;
use crate::vocab::Vocab;

pub mod build;

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

enum Stage {
    Reading(Receiver<Vec<Page>>),
    Levels { next: usize },
    Dictionary { next: usize, ticket: Option<Ticket> },
}

struct Job {
    title: String,
    level: u8,
    list: WordList,
    stage: Stage,
    pages: Vec<Page>,
    keys: Vec<String>,
    bands: HashMap<String, u8>,
    words: Vec<Word>,
    languages: Vec<String>,
    found: HashMap<(String, String), Found>,
}

/// The one list being made; a newer one replaces it.
#[derive(Default)]
pub struct WordLists {
    job: Option<Job>,
}

impl WordLists {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Starts on `ask`; `poll` carries it through.
    pub fn start(&mut self, ctx: &Context, ask: Ask) {
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
        self.job = Some(Job {
            title: ask.title,
            level: ask.level,
            list: ask.list,
            stage: Stage::Reading(rx),
            pages: Vec::new(),
            keys: Vec::new(),
            bands: HashMap::new(),
            words: Vec::new(),
            languages: Vec::new(),
            found: HashMap::new(),
        });
    }

    /// Moves the list on; done, it is made or a toast says why not.
    pub fn poll(&mut self, vocab: &mut Vocab, dict: &mut Dict) -> Option<Result<Made, Toast>> {
        let job = self.job.as_mut()?;
        let step = job.step(vocab, dict);
        if step.is_some() {
            self.job = None;
        }
        step
    }
}

impl Job {
    fn step(&mut self, vocab: &mut Vocab, dict: &mut Dict) -> Option<Result<Made, Toast>> {
        loop {
            match &mut self.stage {
                Stage::Reading(rx) => match rx.try_recv() {
                    Ok(pages) => {
                        self.keys = build::keys(&pages);
                        self.pages = pages;
                        self.stage = Stage::Levels { next: 0 };
                    }
                    Err(TryRecvError::Empty) => return None,
                    Err(TryRecvError::Disconnected) => {
                        return Some(Err(Toast::info("Couldn't read those pages")));
                    }
                },
                Stage::Levels { next } => {
                    if !vocab.ready() {
                        let why =
                            "Word levels aren't downloaded yet. Get them in Settings › Vocabulary.";
                        return Some(Err(Toast::info(why)));
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
                            return Some(Err(Toast::info(why)));
                        }
                        let ready = dict.languages();
                        let asked = self.list.languages.iter();
                        let usable = asked.filter(|l| ready.contains(&l.as_str())).cloned();
                        self.languages = usable.collect();
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
                        return None;
                    }
                    *next += round.len();
                }
                Stage::Dictionary { next, ticket } => {
                    if let Some(t) = *ticket {
                        let reply: Reply = dict.take_words(t)?;
                        for found in reply.found {
                            self.found
                                .insert((found.word.clone(), found.to.clone()), found);
                        }
                        *next += DICT_ROUND;
                        *ticket = None;
                    }
                    let round = self.words.get(*next..).unwrap_or_default();
                    if self.languages.is_empty() || round.is_empty() {
                        return Some(Ok(self.made()));
                    }
                    let round = round.iter().take(DICT_ROUND);
                    let words = round.map(|w| w.key.clone()).collect();
                    let languages = self.languages.clone();
                    *ticket = Some(dict.ask_words(words, languages, self.list.similar));
                    return None;
                }
            }
        }
    }

    fn made(&self) -> Made {
        let first = self.pages.first().map_or(0, |p| p.page);
        let last = self.pages.last().map_or(first, |p| p.page);
        let sheet = Sheet {
            title: &self.title,
            level: self.level,
            words: &self.words,
            found: &self.found,
            languages: &self.languages,
            pages: (first, last),
        };
        let asked = self.list.languages.iter();
        let missing = asked.filter(|l| !self.languages.contains(l)).cloned();
        Made {
            markdown: build::render(&sheet, &self.list),
            words: self.words.len(),
            missing: missing.collect(),
        }
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
