//! Pages summarized, translated or simplified by the AI, shown over them.

mod cards;
mod extract;
mod follow;
pub mod menu;
pub mod paint;

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use egui::{Context, Pos2, Rect, vec2};
use mareader_library::{AiPage, AiPageId, AiTask, BookId, Library, NewAiPage, PageSpan};
use mareader_reflow::{PAD, PAGE_HEIGHT, PAGE_WIDTH, Typography};
use mareader_session::{Anchor, Document, Insert, Inserts, Scale};
use mareader_settings::{AiSettings, PageTask, SummaryStyle, language_name};
use mareader_study::page::{Out, Piece, Rewritten};
use mareader_study::{Failure, Why};
use mareader_workspace::PaneId;

use crate::command::{Command, Commands};
use crate::documents::{AiView, Documents};
use crate::gpu::Gpu;
use crate::toast::Toast;

use extract::Source;
pub use follow::Around;
pub use paint::{Overlay, Pill, PillPress};

/// Seconds a result takes to fade in over its page.
const FADE_IN: f32 = 0.3;
/// Seconds between settings of a result while it streams in.
const RESET: f64 = 0.4;
/// Results kept laid out for a book's pages before they are made again.
const MAX_MADE: usize = 64;

/// One result: kept with the book, or still being written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Saved(AiPageId),
    Run(u64),
}

/// How a task runs: on this page, or on as the reader reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct How {
    pub continuous: bool,
    /// The choice becomes the task's setting.
    pub remember: bool,
}

/// What the page and bubble menus ask of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Run {
        pane: PaneId,
        page: usize,
        task: PageTask,
        how: How,
    },
    /// Translates into language `tag`, remembered for next time.
    Translate {
        pane: PaneId,
        page: usize,
        tag: String,
        how: How,
    },
    /// Ends the pane's continuous run.
    StopFollow(PaneId),
    /// Shows a result over the page; none shows the page itself.
    Show {
        pane: PaneId,
        page: usize,
        slot: Option<Slot>,
    },
    Redo {
        pane: PaneId,
        page: usize,
        slot: Slot,
    },
    Copy(Slot),
    Delete(Slot),
}

/// A page to work on, as the app gathers it.
pub struct Asked<'a> {
    pub pane: PaneId,
    pub page: usize,
    pub book: Option<BookId>,
    /// The document's name: a session's results are kept by it.
    pub name: &'a str,
    pub documents: &'a Documents,
    pub ai: &'a crate::ai::Ai,
    pub settings: &'a AiSettings,
}

/// Who a run is for, and what it asks with.
struct Who<'a> {
    pane: PaneId,
    layout: u64,
    book: Option<BookId>,
    name: &'a str,
    ai: &'a crate::ai::Ai,
    settings: &'a AiSettings,
}

/// What pdf_view draws of the page AI in one pane.
#[derive(Clone, Default)]
pub struct PaneAi {
    pub overlays: Vec<Overlay>,
    pub pill: Option<Pill>,
}

/// A run's line to its writer.
enum Handle {
    #[cfg(not(target_arch = "wasm32"))]
    Live(mareader_assist::Rewriting),
    /// Tests and shots: the answer, given on the first look.
    Canned(Option<Rewritten>),
}

/// A result being written.
struct Run {
    id: u64,
    /// The layout its page is counted in: a book's own pages.
    layout: u64,
    page: usize,
    book: Option<BookId>,
    name: String,
    task: AiTask,
    span: PageSpan,
    source: Arc<Source>,
    by: String,
    handle: Option<Handle>,
    /// What has come, as last set, and how many times it came.
    shown: Option<Arc<Rewritten>>,
    latest: Option<Rewritten>,
    shown_at: f64,
    generation: u64,
    failed: Option<String>,
    /// The kept result it will replace.
    redo_of: Option<AiPageId>,
}

/// A pane's layout, and each page's bubbles with their own pages.
type Drawn = (u64, Vec<(usize, Vec<(Slot, usize)>)>);

/// A pane, a layout and one of its own pages.
type Key = (PaneId, u64, usize);

/// A result to place: its task, span, words, source and identity.
type What = (AiTask, PageSpan, Arc<Rewritten>, Option<Arc<Source>>, u64);

/// The result a pane shows over a page, and since when.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Showing {
    slot: Slot,
    since: f64,
}

/// A book's results by the page each starts on, for one layout.
struct Index {
    key: u64,
    pages: HashMap<usize, Vec<AiPageId>>,
}

/// What the page menu shows.
#[derive(Clone, Debug, PartialEq)]
enum Step {
    Tasks,
    Languages,
    /// This page or continuous, for a task and maybe a language.
    How(PageTask, Option<String>),
}

#[derive(Clone, Debug, PartialEq)]
struct Menu {
    pane: PaneId,
    page: usize,
    at: Pos2,
    step: Step,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BubbleMenu {
    pane: PaneId,
    page: usize,
    slot: Slot,
    at: Pos2,
}

/// Results of documents outside the library, for the session.
struct Loose(Library);

impl Default for Loose {
    fn default() -> Self {
        Self(Library::new(mareader_library::DeviceId(0)))
    }
}

/// The page AI's state across panes.
#[derive(Default)]
pub struct PageAi {
    loose: Loose,
    runs: Vec<Run>,
    next_run: u64,
    showing: HashMap<Key, Showing>,
    cards: cards::Cards,
    /// By layout: a book's results laid in keep its own pages' places.
    index: HashMap<u64, Index>,
    /// The layout each pane's results were last counted in.
    aligned: HashMap<PaneId, u64>,
    /// Results as laid into a book's pages, by id.
    made: HashMap<AiPageId, Arc<Insert>>,
    /// Each book result's text, from where to where, as last indexed.
    spans: HashMap<AiPageId, (Anchor, Anchor)>,
    /// What each pane's book was last asked to lay in.
    wants: HashMap<PaneId, Arc<Inserts>>,
    /// Results read once, by id.
    parsed: HashMap<AiPageId, Option<Arc<Rewritten>>>,
    /// Each pane's bubbles as last drawn, to tell which was pressed.
    drawn: HashMap<PaneId, Drawn>,
    menu: Option<Menu>,
    menu_rect: Option<Rect>,
    filter: String,
    bubble_menu: Option<BubbleMenu>,
    bubble_rect: Option<Rect>,
    /// Each pane's continuous run.
    follows: HashMap<PaneId, follow::Follow>,
    /// The menu's "Remember my choice".
    remember: bool,
    /// Tests: runs answer at once with the page's own text.
    canned: bool,
    /// Tests: pages ahead are read on the frame.
    inline: bool,
}

/// The task a menu row means, as kept; Translate needs a language.
pub fn task_for(task: PageTask, ai: &AiSettings) -> Option<AiTask> {
    Some(match task {
        PageTask::Summarize => match ai.tasks.summarize.style {
            SummaryStyle::Paragraphs => AiTask::Summary,
            SummaryStyle::Points => AiTask::Points,
        },
        PageTask::Translate => AiTask::Translate {
            language: ai.tasks.translate.language.clone()?,
        },
        PageTask::Simplify => AiTask::Simplify,
    })
}

/// How a bubble names its result.
fn label(task: &AiTask) -> String {
    match task {
        AiTask::Summary => "Summary".to_owned(),
        AiTask::Points => "Key points".to_owned(),
        AiTask::Translate { language } => {
            let name = language_name(language).unwrap_or(language);
            format!("Translation · {name}")
        }
        AiTask::Simplify => "Simplified".to_owned(),
    }
}

/// The settings' task a kept task belongs to.
pub fn page_task(task: &AiTask) -> PageTask {
    match task {
        AiTask::Summary | AiTask::Points => PageTask::Summarize,
        AiTask::Translate { .. } => PageTask::Translate,
        AiTask::Simplify => PageTask::Simplify,
    }
}

/// Why a page couldn't be done, in the reader's words.
fn say(f: &Failure) -> String {
    let why = match f.why {
        Why::NotHere => "No AI model is set up for this. Choose one in Settings › AI.",
        why => why.message(),
    };
    match &f.detail {
        Some(d) => format!("{why} ({d})"),
        None => why.to_owned(),
    }
}

/// The own page of a book showing `at`.
fn base_page(document: &Document, at: [usize; 2]) -> Option<usize> {
    match document {
        Document::Epub { book, .. } => Some(book.base_locate(anchor(at))),
        Document::Pdf(_) => None,
    }
}

/// A kept span's end, as a place in the book.
fn anchor([block, offset]: [usize; 2]) -> Anchor {
    Anchor { block, offset }
}

fn hash(v: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Own page `page`'s text, to send or show beside a result.
fn source_of(
    view: &AiView,
    documents: &Documents,
    page: usize,
) -> Result<(Source, PageSpan), &'static str> {
    match &*view.document {
        Document::Epub { book, .. } => {
            let blocks = book.blocks();
            let span = extract::span(&book.base_excerpts(page), blocks).ok_or("blank")?;
            let source = extract::book(blocks, span);
            let [from, to] = span;
            Ok((source, PageSpan::Blocks { from, to }))
        }
        Document::Pdf(_) => {
            let layer = documents.text_layer(view.doc, page).ok_or("loading")?;
            let near: Vec<_> = [page.checked_sub(1), Some(page + 1)]
                .into_iter()
                .flatten()
                .filter_map(|p| documents.text_layer(view.doc, p))
                .collect();
            let near: Vec<_> = near.iter().map(|l| &l.text).collect();
            Ok((extract::pdf(&layer.text, &near), PageSpan::Page { page }))
        }
    }
}

impl PageAi {
    /// The book's library: the app's, else the session's for `name`.
    fn shelf<'a>(
        &'a mut self,
        main: &'a mut Library,
        book: Option<BookId>,
        name: &str,
    ) -> (&'a mut Library, BookId) {
        match book.filter(|b| main.book(*b).is_some()) {
            Some(id) => (main, id),
            None => {
                let id = crate::study::loose_book(&mut self.loose.0, name);
                (&mut self.loose.0, id)
            }
        }
    }

    fn lib<'a>(
        &'a self,
        main: &'a Library,
        book: Option<BookId>,
        name: &str,
    ) -> (&'a Library, BookId) {
        match book.filter(|b| main.book(*b).is_some()) {
            Some(id) => (main, id),
            None => (&self.loose.0, crate::study::loose_id(name)),
        }
    }

    fn saved<'a>(&'a self, main: &'a Library, id: AiPageId) -> Option<&'a AiPage> {
        main.ai_page(id).or_else(|| self.loose.0.ai_page(id))
    }

    /// Writes `task` for the page; it shows over the page as it comes.
    pub fn run(&mut self, asked: Asked<'_>, task: AiTask, now: f64, out: &mut Commands) {
        self.redo(asked, task, None, now, out);
    }

    /// Writes the page's text; the same ask under way just shows.
    fn begin(
        &mut self,
        who: &Who<'_>,
        page: usize,
        task: AiTask,
        (source, span): (Source, PageSpan),
        redo_of: Option<AiPageId>,
        now: f64,
    ) -> Result<u64, &'static str> {
        let key = (who.pane, who.layout, page);
        let same = |r: &&Run| r.layout == who.layout && r.task == task && r.span == span;
        if let Some(run) = self.runs.iter().find(same).filter(|r| r.failed.is_none()) {
            let slot = Slot::Run(run.id);
            self.showing.insert(key, Showing { slot, since: now });
            return Ok(run.id);
        }
        self.runs
            .retain(|r| !(r.layout == who.layout && r.task == task && r.span == span));
        let handle = match self.canned {
            true => Handle::Canned(Some(canned(&source))),
            false => start(who.ai, &task, &source)?,
        };
        let preset = who.settings.page_preset(page_task(&task));
        let by = preset.map_or_else(String::new, |p| match p.name.trim() {
            "" => p.model.clone(),
            name => format!("{name} · {}", p.model),
        });
        self.next_run += 1;
        let id = self.next_run;
        self.runs.push(Run {
            id,
            layout: who.layout,
            page,
            book: who.book,
            name: who.name.to_owned(),
            task,
            span,
            source: Arc::new(source),
            by,
            handle: Some(handle),
            shown: None,
            latest: None,
            shown_at: 0.0,
            generation: 0,
            failed: None,
            redo_of,
        });
        let slot = Slot::Run(id);
        self.showing.insert(key, Showing { slot, since: now });
        Ok(id)
    }

    fn redo(
        &mut self,
        asked: Asked<'_>,
        task: AiTask,
        redo_of: Option<AiPageId>,
        now: f64,
        out: &mut Commands,
    ) -> Option<u64> {
        let view = asked.documents.ai_view(asked.pane)?;
        let page = view.document.to_base(asked.page);
        let note = |s: &str| Command::Notify(Toast::info(s.to_owned()));
        let found = match source_of(&view, asked.documents, page) {
            Ok(found) => found,
            Err("loading") => {
                out.push(note(
                    "This page's text is still loading. Try again in a moment.",
                ));
                return None;
            }
            Err(_) => {
                out.push(note("This page has no text to work on."));
                return None;
            }
        };
        if found.0.is_blank() {
            let scan = matches!(*view.document, Document::Pdf(_));
            let why = match scan {
                true => "This page has no text to work on. It may be a scanned image.",
                false => "This page has no text to work on.",
            };
            out.push(note(why));
            return None;
        }
        let who = Who {
            pane: asked.pane,
            layout: view.layout,
            book: asked.book,
            name: asked.name,
            ai: asked.ai,
            settings: asked.settings,
        };
        match self.begin(&who, page, task, found, redo_of, now) {
            Ok(id) => Some(id),
            Err(why) => {
                out.push(note(why));
                None
            }
        }
    }

    /// Writes a result's task again, replacing it once done.
    pub fn again(
        &mut self,
        asked: Asked<'_>,
        main: &Library,
        slot: Slot,
        now: f64,
        out: &mut Commands,
    ) {
        let (task, old) = match slot {
            Slot::Saved(id) => match self.saved(main, id) {
                Some(p) => (p.task.clone(), Some(id)),
                None => return,
            },
            Slot::Run(id) => match self.runs.iter().find(|r| r.id == id) {
                Some(r) => (r.task.clone(), r.redo_of),
                None => return,
            },
        };
        self.redo(asked, task, old, now, out);
    }

    /// Takes in what the AI wrote; kept results go to the book.
    pub fn receive(
        &mut self,
        ctx: &Context,
        main: &mut Library,
        gpu: Option<&Gpu>,
        out: &mut Commands,
    ) {
        let now = ctx.input(|i| i.time);
        let mut done = Vec::new();
        for run in &mut self.runs {
            for news in poll(run.handle.as_mut()) {
                match news {
                    News::Partial(r) => {
                        run.latest = Some(r);
                        run.generation += 1;
                    }
                    News::Done(Ok(r)) => {
                        run.handle = None;
                        done.push((run.id, r));
                    }
                    News::Done(Err(f)) => {
                        run.handle = None;
                        let mut said = say(&f);
                        if self.follows.values().any(|w| w.runs.contains(&run.id)) {
                            said.push_str(" Continuous mode stopped.");
                        }
                        out.push(Command::Notify(Toast::info(said.clone())));
                        run.failed = Some(said);
                    }
                }
            }
            // A streaming result is set again only so often.
            if run.latest.is_some() && now - run.shown_at >= RESET {
                run.shown = run.latest.take().map(Arc::new);
                run.shown_at = now;
            }
            if run.latest.is_some() {
                ctx.request_repaint_after_secs(RESET as f32);
            }
        }
        // A failed run that was shown leaves the page as it was.
        let failed: Vec<u64> = self
            .runs
            .iter()
            .filter(|r| r.failed.is_some())
            .map(|r| r.id)
            .collect();
        self.showing
            .retain(|_, s| !matches!(s.slot, Slot::Run(id) if failed.contains(&id)));
        for (id, result) in done {
            self.keep(main, id, result);
        }
        if self.cards.receive(ctx, gpu, now) {
            ctx.request_repaint();
        }
    }

    /// Saves run `id`'s result; whatever showed it shows the saved one.
    fn keep(&mut self, main: &mut Library, id: u64, result: Rewritten) {
        let Some(i) = self.runs.iter().position(|r| r.id == id) else {
            return;
        };
        let run = self.runs.remove(i);
        let new = NewAiPage {
            task: run.task.clone(),
            span: run.span.clone(),
            source: run.source.key(),
            result: serde_json::to_string(&result).unwrap_or_default(),
            by: run.by.clone(),
        };
        let wall = crate::library::wall_ms();
        let (lib, book) = self.shelf(main, run.book, &run.name);
        let Some(saved) = lib.add_ai_page(book, new, wall) else {
            return;
        };
        if let Some(old) = run.redo_of.filter(|o| *o != saved) {
            lib.set_ai_pages_alive(&[old], false, wall);
        }
        self.parsed.insert(saved, Some(Arc::new(result)));
        if let PageSpan::Blocks { from, to } = run.span {
            self.spans.insert(saved, (anchor(from), anchor(to)));
        }
        let was = |s: &Showing| {
            s.slot == Slot::Run(id) || run.redo_of.is_some_and(|o| s.slot == Slot::Saved(o))
        };
        let keys: Vec<(Key, f64)> = (self.showing.iter())
            .filter(|(_, s)| was(s))
            .map(|(k, s)| (*k, s.since))
            .collect();
        for (key, since) in keys {
            self.show_on(key, Slot::Saved(saved), since);
        }
    }

    /// A kept result, read once.
    fn result(&mut self, main: &Library, id: AiPageId) -> Option<Arc<Rewritten>> {
        if let Some(r) = self.parsed.get(&id) {
            return r.clone();
        }
        let page = self.saved(main, id)?;
        let read = serde_json::from_str::<Rewritten>(&page.result)
            .ok()
            .map(Arc::new);
        self.parsed.insert(id, read.clone());
        read
    }

    /// Each result's page in `view`'s layout, read again on change.
    fn index(
        &mut self,
        main: &Library,
        view: &AiView,
        (book, name): (Option<BookId>, &str),
    ) -> HashMap<usize, Vec<AiPageId>> {
        let (lib, id) = self.lib(main, book, name);
        let pages = lib.ai_pages(id);
        let key = hash(pages.iter().map(|p| p.id).collect::<Vec<_>>());
        if let Some(index) = self.index.get(&view.layout).filter(|i| i.key == key) {
            return index.pages.clone();
        }
        let mut map: HashMap<usize, Vec<AiPageId>> = HashMap::new();
        let mut spans = Vec::new();
        for p in &pages {
            if let PageSpan::Blocks { from, to } = p.span {
                spans.push((p.id, (anchor(from), anchor(to))));
            }
            let page = match p.span {
                PageSpan::Page { page } => Some(page),
                PageSpan::Blocks { from, .. } => base_page(&view.document, from),
            };
            if let Some(page) = page.filter(|p| *p < view.document.base_count()) {
                map.entry(page).or_default().push(p.id);
            }
        }
        self.spans.extend(spans);
        if self.index.len() > 8 {
            self.index.clear();
        }
        let pages = map.clone();
        self.index.insert(view.layout, Index { key, pages });
        map
    }

    /// What `pane`'s pages on screen show of the AI this frame.
    pub fn overlays(
        &mut self,
        ctx: &Context,
        pane: PaneId,
        main: &Library,
        documents: &Documents,
        (book, name): (Option<BookId>, &str),
    ) -> PaneAi {
        let now = ctx.input(|i| i.time);
        let pill = self.pill(pane, now);
        let Some(view) = documents.ai_view(pane) else {
            self.drawn.remove(&pane);
            return PaneAi {
                overlays: Vec::new(),
                pill,
            };
        };
        self.align(main, pane, &view, (book, name));
        let index = self.index(main, &view, (book, name));
        let mut overlays = Vec::new();
        let mut drawn = Vec::new();
        for page in view.visible.clone() {
            // A result's first page carries the bubbles of the pages it took.
            let mut slots = Vec::new();
            let mut bubbles = Vec::new();
            let mut card = None;
            for own in view.document.base_pages(page) {
                let shown = self.bubbles(main, &view, (pane, own), &index);
                for (slot, bubble) in shown {
                    let active = bubble.active;
                    bubbles.push(bubble);
                    slots.push((slot, own));
                    if active && card.is_none() && self.carded(pane, &view, slot) {
                        card = Some((own, slot));
                    }
                }
            }
            if slots.is_empty() {
                // A result's later pages: its menu works there too.
                let own = view.document.to_base(page);
                let key = (pane, view.layout, own);
                if view.document.base_pages(page).is_empty()
                    && let Some(shown) = self.showing.get(&key)
                {
                    drawn.push((page, vec![(shown.slot, own)]));
                }
                continue;
            }
            for (n, b) in bubbles.iter_mut().enumerate() {
                b.label = (n + 1).to_string();
            }
            let card =
                card.and_then(|(own, slot)| self.shown(ctx, pane, &view, (page, own), slot, main));
            overlays.push(Overlay {
                page,
                bubbles,
                card,
            });
            drawn.push((page, slots));
        }
        self.drawn.insert(pane, (view.layout, drawn));
        PaneAi { overlays, pill }
    }

    /// Own page `own`'s results as bubbles, kept ones first.
    fn bubbles(
        &self,
        main: &Library,
        view: &AiView,
        (pane, own): (PaneId, usize),
        index: &HashMap<usize, Vec<AiPageId>>,
    ) -> Vec<(Slot, paint::Bubble)> {
        let saved = index.get(&own).map_or(&[][..], Vec::as_slice);
        let runs = (self.runs.iter())
            .filter(|r| r.layout == view.layout && r.page == own)
            .filter(|r| r.redo_of.is_none_or(|o| !saved.contains(&o)));
        let active = self.showing.get(&(pane, view.layout, own)).map(|s| s.slot);
        let here = match &*view.document {
            Document::Epub { book, .. } => extract::span(&book.base_excerpts(own), book.blocks()),
            Document::Pdf(_) => None,
        };
        let mut out = Vec::new();
        for id in saved {
            let Some(p) = self.saved(main, *id) else {
                continue;
            };
            let redoing = self.runs.iter().find(|r| r.redo_of == Some(*id));
            let stale = match (&p.span, here) {
                (PageSpan::Blocks { from, to }, Some(span)) => [*from, *to] != span,
                _ => false,
            };
            let state = match (redoing, stale) {
                (Some(r), _) if r.failed.is_none() => paint::State::Busy,
                (_, true) => paint::State::Stale,
                _ => paint::State::Ready,
            };
            let slot = Slot::Saved(*id);
            let mut tip = label(&p.task);
            if state == paint::State::Stale {
                tip.push_str(" · made for another text size");
            }
            let active =
                active == Some(slot) || redoing.is_some_and(|r| active == Some(Slot::Run(r.id)));
            let bubble = paint::Bubble {
                label: String::new(),
                tip,
                active,
                state,
            };
            out.push((slot, bubble));
        }
        for r in runs {
            let slot = Slot::Run(r.id);
            let (state, tip) = match &r.failed {
                Some(why) => (paint::State::Failed, format!("{} · {why}", label(&r.task))),
                None => (paint::State::Busy, format!("{} · writing…", label(&r.task))),
            };
            let bubble = paint::Bubble {
                label: String::new(),
                tip,
                active: active == Some(slot),
                state,
            };
            out.push((slot, bubble));
        }
        out
    }

    /// Whether `slot` shows as a card: a book's, until laid in.
    fn carded(&self, pane: PaneId, view: &AiView, slot: Slot) -> bool {
        let Slot::Saved(id) = slot else {
            return true;
        };
        if !view.document.is_reflowable() {
            return true;
        }
        let asked = self.wants.get(&pane).map_or(0, |w| w.key);
        let laid = view.document.inserts_key() == asked;
        !laid && !view.document.shows(id.0)
    }

    /// Re-counts `pane`'s shown results once new type moves its pages.
    fn align(&mut self, main: &Library, pane: PaneId, view: &AiView, who: (Option<BookId>, &str)) {
        let old = self.aligned.insert(pane, view.layout);
        if old.is_none_or(|o| o == view.layout) {
            return;
        }
        let (lib, book) = self.lib(main, who.0, who.1);
        let page_of = |span: &PageSpan| match span {
            PageSpan::Page { page } => Some(*page),
            PageSpan::Blocks { from, .. } => base_page(&view.document, *from),
        };
        let moved: Vec<(Key, Option<usize>)> = (self.showing.iter())
            .filter(|((p, l, _), _)| *p == pane && *l != view.layout)
            .map(|(key, shown)| {
                let page = match shown.slot {
                    Slot::Saved(id) => {
                        let p = lib.ai_page(id).filter(|p| p.book.value == book);
                        p.and_then(|p| page_of(&p.span))
                    }
                    Slot::Run(id) => (self.runs.iter())
                        .find(|r| r.id == id && r.book == who.0 && r.name == who.1)
                        .and_then(|r| page_of(&r.span)),
                };
                (*key, page)
            })
            .collect();
        for (key, page) in moved {
            let Some(shown) = self.showing.remove(&key) else {
                continue;
            };
            let Some(page) = page else {
                continue;
            };
            if let Slot::Run(id) = shown.slot
                && let Some(r) = self.runs.iter_mut().find(|r| r.id == id)
            {
                (r.layout, r.page) = (view.layout, page);
            }
            self.showing.insert((pane, view.layout, page), shown);
        }
    }

    /// The kept results `pane`'s book lays in; none for a PDF.
    pub fn inserts(
        &mut self,
        main: &Library,
        pane: PaneId,
        documents: &Documents,
        who: (Option<BookId>, &str),
    ) -> Option<Arc<Inserts>> {
        let view = documents.ai_view(pane)?;
        let Document::Epub { book, .. } = &*view.document else {
            self.wants.remove(&pane);
            return None;
        };
        self.align(main, pane, &view, who);
        let mut ids: Vec<AiPageId> = (self.showing.iter())
            .filter(|((p, l, _), _)| *p == pane && *l == view.layout)
            .filter_map(|(_, s)| match s.slot {
                Slot::Saved(id) => Some(id),
                Slot::Run(_) => None,
            })
            .collect();
        ids.sort_unstable();
        let key = match ids.is_empty() {
            true => 0,
            false => hash(("inserts", &ids)).max(1),
        };
        if let Some(w) = self.wants.get(&pane).filter(|w| w.key == key) {
            return Some(Arc::clone(w));
        }
        let mut list = Vec::new();
        for id in ids {
            if let Some(made) = self.made.get(&id) {
                list.push(Arc::clone(made));
                continue;
            }
            let Some(p) = self.saved(main, id) else {
                continue;
            };
            let (task, span) = (p.task.clone(), p.span.clone());
            let PageSpan::Blocks { from, to } = span else {
                continue;
            };
            let Some(result) = self.result(main, id) else {
                continue;
            };
            let source = extract::book(book.blocks(), [from, to]);
            let blocks = extract::blocks(&result, &source, book.blocks());
            let language = match task {
                AiTask::Translate { language } => Some(language),
                _ => None,
            };
            let made = Arc::new(Insert {
                id: id.0,
                from: anchor(from),
                to: anchor(to),
                blocks: Arc::new(blocks),
                language,
            });
            if self.made.len() > MAX_MADE {
                self.made.clear();
            }
            self.made.insert(id, Arc::clone(&made));
            list.push(made);
        }
        let inserts = Arc::new(Inserts { key, list });
        self.wants.insert(pane, Arc::clone(&inserts));
        Some(inserts)
    }

    /// The card of `slot` on `page`, set for the pane's type and zoom.
    fn shown(
        &mut self,
        ctx: &Context,
        pane: PaneId,
        view: &AiView,
        (page, own): (usize, usize),
        slot: Slot,
        main: &Library,
    ) -> Option<paint::Shown> {
        let since = self.showing.get(&(pane, view.layout, own))?.since;
        let old = match slot {
            Slot::Saved(id) => id,
            Slot::Run(id) => {
                let r = self.runs.iter().find(|r| r.id == id)?;
                match (&r.shown, r.redo_of) {
                    (Some(shown), _) => {
                        let ident = hash((id, r.shown_at.to_bits()));
                        let source = Some(Arc::clone(&r.source));
                        let what = (
                            r.task.clone(),
                            r.span.clone(),
                            Arc::clone(shown),
                            source,
                            ident,
                        );
                        return self.place(ctx, pane, view, page, what, since);
                    }
                    // A redo shows what it replaces until its own words come.
                    (None, Some(old)) => old,
                    (None, None) => return Some(self.blank_card(view, page, since, ctx)),
                }
            }
        };
        let p = self.saved(main, old)?;
        let (task, span) = (p.task.clone(), p.span.clone());
        let result = self.result(main, old)?;
        self.place(
            ctx,
            pane,
            view,
            page,
            (task, span, result, None, hash(old)),
            since,
        )
    }

    fn room(view: &AiView, page: usize) -> (egui::Vec2, egui::Vec2, f32) {
        match &*view.document {
            Document::Epub { book, .. } => {
                let (x, w) = book.column(page);
                let width = book.page_size().0;
                (vec2(x, PAD), vec2(w, PAGE_HEIGHT - 2.0 * PAD), width)
            }
            Document::Pdf(_) => {
                let size = view.document.page_sizes().get(page).copied();
                let (w, h) = size.map_or((612.0, 792.0), |s| (s.width, s.height));
                let m = 0.08 * w.min(h);
                (vec2(m, m), vec2(w - 2.0 * m, h - 2.0 * m), w)
            }
        }
    }

    fn alpha(ctx: &Context, since: f64) -> f32 {
        let fade = crate::motion::secs(ctx, FADE_IN);
        let now = ctx.input(|i| i.time);
        let a = if fade <= 0.0 {
            1.0
        } else {
            ((now - since) as f32 / fade).clamp(0.0, 1.0)
        };
        if a < 1.0 {
            ctx.request_repaint();
        }
        a
    }

    /// Paper alone, while the first words are on their way.
    fn blank_card(&self, view: &AiView, page: usize, since: f64, ctx: &Context) -> paint::Shown {
        let (at, room, width) = Self::room(view, page);
        paint::Shown {
            card: None,
            at,
            room,
            alpha: Self::alpha(ctx, since),
            width,
        }
    }

    fn place(
        &mut self,
        ctx: &Context,
        pane: PaneId,
        view: &AiView,
        page: usize,
        (task, span, result, source, ident): What,
        since: f64,
    ) -> Option<paint::Shown> {
        let (at, room, width) = Self::room(view, page);
        let mut typo: Typography = (*view.typo).clone();
        if matches!(*view.document, Document::Pdf(_)) {
            typo.font_size *= width / PAGE_WIDTH;
        }
        if let AiTask::Translate { language } = &task {
            typo.language = language.clone();
        }
        let ppp = ctx.pixels_per_point();
        let scale = Scale::covering(view.zoom * ppp).value();
        let slot = hash((pane, view.layout, page, &task, &span));
        let key = hash((
            ident,
            format!("{typo:?}"),
            room.x.to_bits(),
            room.y.to_bits(),
            scale.to_bits(),
        ));
        let document = Arc::clone(&view.document);
        let make = || {
            let blocks = match &*document {
                Document::Epub { book, .. } => {
                    let source = source.unwrap_or_else(|| match &span {
                        PageSpan::Blocks { from, to } => {
                            Arc::new(extract::book(book.blocks(), [*from, *to]))
                        }
                        PageSpan::Page { .. } => Arc::default(),
                    });
                    extract::blocks(&result, &source, book.blocks())
                }
                Document::Pdf(_) => extract::blocks(&result, &Source::default(), &[]),
            };
            let book = matches!(*document, Document::Epub { .. }).then(|| Arc::clone(&document));
            cards::Job {
                blocks: Arc::new(blocks),
                typo,
                room: (room.x, room.y),
                scale,
                book,
            }
        };
        let now = ctx.input(|i| i.time);
        let card = self.cards.get(slot, key, now, make);
        Some(paint::Shown {
            card,
            at,
            room,
            alpha: Self::alpha(ctx, since),
            width,
        })
    }

    /// A bubble was pressed: its result shows, or the page again.
    pub fn pick(&mut self, pane: PaneId, page: usize, index: usize, at: Pos2, now: f64) {
        let Some((layout, slot, own)) = self.slot_at(pane, page, index) else {
            return;
        };
        if let Slot::Run(id) = slot
            && self.runs.iter().any(|r| r.id == id && r.failed.is_some())
        {
            self.menu = None;
            self.bubble_menu = Some(BubbleMenu {
                pane,
                page,
                slot,
                at,
            });
            return;
        }
        let key = (pane, layout, own);
        match self.showing.get(&key) {
            Some(s) if s.slot == slot => {
                self.showing.remove(&key);
            }
            _ => self.show_on(key, slot, now),
        }
    }

    /// Shows `slot` on own page `key`; others of the same text give way.
    fn show_on(&mut self, key: Key, slot: Slot, now: f64) {
        let span = |slot: Slot| match slot {
            Slot::Saved(id) => self.spans.get(&id).copied(),
            Slot::Run(_) => None,
        };
        // Only one result of any text is laid into the pages.
        if let Some((from, to)) = span(slot) {
            let (pane, layout, _) = key;
            let overlaps: Vec<Key> = (self.showing.iter())
                .filter(|((p, l, _), _)| *p == pane && *l == layout)
                .filter(|(_, s)| span(s.slot).is_some_and(|(a, b)| a < to && from < b))
                .map(|(k, _)| *k)
                .collect();
            for k in overlaps {
                self.showing.remove(&k);
            }
        }
        self.showing.insert(key, Showing { slot, since: now });
    }

    /// The bubble `index` on page `page`: its layout, slot and own page.
    fn slot_at(&self, pane: PaneId, page: usize, index: usize) -> Option<(u64, Slot, usize)> {
        let (layout, pages) = self.drawn.get(&pane)?;
        let slots = &pages.iter().find(|(p, _)| *p == page)?.1;
        let (slot, own) = *slots.get(index)?;
        Some((*layout, slot, own))
    }

    /// Shows `slot` over the page, or the page itself.
    pub fn show_result(&mut self, pane: PaneId, page: usize, slot: Option<Slot>, now: f64) {
        let Some((layout, pages)) = self.drawn.get(&pane) else {
            return;
        };
        let layout = *layout;
        let slots = pages.iter().find(|(p, _)| *p == page);
        let slots = slots.map(|(_, s)| s.clone()).unwrap_or_default();
        match slot {
            Some(slot) => {
                if let Some((_, own)) = slots.iter().find(|(s, _)| *s == slot) {
                    self.show_on((pane, layout, *own), slot, now);
                }
            }
            None => {
                for (_, own) in slots {
                    self.showing.remove(&(pane, layout, own));
                }
            }
        }
    }

    /// A result's text, for the clipboard.
    pub fn text_of(&mut self, main: &Library, slot: Slot) -> Option<String> {
        let result = match slot {
            Slot::Saved(id) => self.result(main, id)?,
            Slot::Run(id) => self.runs.iter().find(|r| r.id == id)?.shown.clone()?,
        };
        Some(extract::plain(&result))
    }

    /// Deletes a kept result, or stops one being written.
    pub fn delete(&mut self, main: &mut Library, slot: Slot) {
        match slot {
            Slot::Saved(id) => {
                let wall = crate::library::wall_ms();
                if main.set_ai_pages_alive(&[id], false, wall) == 0 {
                    self.loose.0.set_ai_pages_alive(&[id], false, wall);
                }
                self.parsed.remove(&id);
                self.runs.retain(|r| r.redo_of != Some(id));
            }
            Slot::Run(id) => self.runs.retain(|r| r.id != id),
        }
        self.showing.retain(|_, s| s.slot != slot);
    }

    /// Opens the page's menu, at a right-click.
    pub fn open_menu(&mut self, pane: PaneId, page: usize, at: Pos2) {
        self.bubble_menu = None;
        self.filter.clear();
        self.remember = false;
        self.menu = Some(Menu {
            pane,
            page,
            at,
            step: Step::Tasks,
        });
    }

    /// Opens a bubble's menu, at a right-click.
    pub fn open_bubble_menu(&mut self, pane: PaneId, page: usize, index: usize, at: Pos2) {
        if let Some((_, slot, _)) = self.slot_at(pane, page, index) {
            self.menu = None;
            self.bubble_menu = Some(BubbleMenu {
                pane,
                page,
                slot,
                at,
            });
        }
    }

    /// A menu is open, so Escape is the page AI's.
    pub fn escape(&mut self) -> bool {
        let open = self.menu.is_some() || self.bubble_menu.is_some();
        self.menu = None;
        self.bubble_menu = None;
        open
    }

    /// Shots and tests: keeps and shows a result; false before text.
    pub fn seed(&mut self, asked: Asked<'_>, main: &mut Library, task: AiTask, now: f64) -> bool {
        let Some(view) = asked.documents.ai_view(asked.pane) else {
            return false;
        };
        let page = view.document.to_base(asked.page);
        let Ok((source, span)) = source_of(&view, asked.documents, page) else {
            return false;
        };
        let result = canned(&source);
        let new = NewAiPage {
            task,
            span,
            source: source.key(),
            result: serde_json::to_string(&result).unwrap_or_default(),
            by: "Canned · test".to_owned(),
        };
        let wall = crate::library::wall_ms();
        let (lib, book) = self.shelf(main, asked.book, asked.name);
        let Some(id) = lib.add_ai_page(book, new, wall) else {
            return false;
        };
        let slot = Slot::Saved(id);
        let key = (asked.pane, view.layout, page);
        self.showing.insert(key, Showing { slot, since: now });
        true
    }

    /// Whether `pane`'s document has a kept result or one being written.
    pub fn any(
        &self,
        main: &Library,
        pane: PaneId,
        documents: &Documents,
        who: (Option<BookId>, &str),
    ) -> bool {
        let Some(view) = documents.ai_view(pane) else {
            return false;
        };
        let (lib, book) = self.lib(main, who.0, who.1);
        lib.has_ai_pages(book) || self.runs.iter().any(|r| r.layout == view.layout)
    }

    /// Shots: whether the page's text is in, to be worked on.
    pub fn can_read(&self, asked: &Asked<'_>) -> bool {
        let view = asked.documents.ai_view(asked.pane);
        let page = |v: &AiView| v.document.to_base(asked.page);
        view.is_some_and(|v| source_of(&v, asked.documents, page(&v)).is_ok())
    }

    /// Tests: results are set on the frame, not on a thread.
    pub fn set_inline(&mut self, inline: bool) {
        self.cards.set_inline(inline);
        self.inline = inline;
    }

    /// Tests: runs answer at once, in capitals, with no AI.
    pub fn set_canned(&mut self, canned: bool) {
        self.canned = canned;
    }

    /// Whether `pane` shows a result over `page`.
    pub fn shows(&self, pane: PaneId, page: usize) -> bool {
        self.showing_on(pane, page).is_some()
    }

    /// Whether the page shows a result in `pane`, for the menu.
    fn showing_on(&self, pane: PaneId, page: usize) -> Option<Slot> {
        let (layout, pages) = self.drawn.get(&pane)?;
        let Some((_, slots)) = pages.iter().find(|(p, _)| *p == page) else {
            // Off screen: a book's pages there are taken as its own.
            return self.showing.get(&(pane, *layout, page)).map(|s| s.slot);
        };
        slots.iter().find_map(|(_, own)| {
            let shown = self.showing.get(&(pane, *layout, *own))?;
            Some(shown.slot)
        })
    }
}

/// The page's own text in capitals: a stand-in answer.
fn canned(source: &Source) -> Rewritten {
    let outs = source
        .pieces
        .iter()
        .enumerate()
        .map(|(i, piece)| match piece {
            Piece::Text { role, text, .. } => Out::Text {
                role: *role,
                text: text.to_uppercase(),
                code: Vec::new(),
                from: Some(i),
            },
            Piece::Kept(_) => Out::Kept(i),
        });
    Rewritten {
        outs: outs.collect(),
        missing: 0,
    }
}

/// News from a run.
enum News {
    Partial(Rewritten),
    Done(Result<Rewritten, Failure>),
}

#[cfg(not(target_arch = "wasm32"))]
fn start(ai: &crate::ai::Ai, task: &AiTask, source: &Source) -> Result<Handle, &'static str> {
    let rewrite = match task {
        AiTask::Summary => mareader_assist::Rewrite::Summarize(SummaryStyle::Paragraphs),
        AiTask::Points => mareader_assist::Rewrite::Summarize(SummaryStyle::Points),
        AiTask::Translate { language } => {
            let name = language_name(language).unwrap_or(language);
            mareader_assist::Rewrite::Translate(name.to_owned())
        }
        AiTask::Simplify => mareader_assist::Rewrite::Simplify,
    };
    let job = mareader_assist::PageJob {
        rewrite,
        pieces: source.pieces.clone(),
    };
    let off = "The AI service couldn't start. Restart the app to try again.";
    ai.rewrite(job).map(Handle::Live).ok_or(off)
}

#[cfg(target_arch = "wasm32")]
fn start(_: &crate::ai::Ai, _: &AiTask, _: &Source) -> Result<Handle, &'static str> {
    Err("Page AI needs the desktop app.")
}

fn poll(handle: Option<&mut Handle>) -> Vec<News> {
    match handle {
        None => Vec::new(),
        Some(Handle::Canned(answer)) => answer
            .take()
            .map(|r| News::Done(Ok(r)))
            .into_iter()
            .collect(),
        #[cfg(not(target_arch = "wasm32"))]
        Some(Handle::Live(live)) => {
            let news = live.poll().into_iter().map(|n| match n {
                mareader_assist::Rewrote::Partial(r) => News::Partial(r),
                mareader_assist::Rewrote::Done(r) => News::Done(r),
            });
            news.collect()
        }
    }
}
