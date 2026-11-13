//! Pages summarized, translated or simplified by the AI, shown over them.

mod cards;
pub mod menu;
pub mod paint;

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use egui::{Context, Id, Pos2, Rect, vec2};
use mareader_library::{AiPage, AiPageId, AiTask, BookId, Library, NewAiPage, PageSpan};
use mareader_reflow::{PAGE_WIDTH, Typography};
use mareader_session::{Anchor, Document, Insert, Inserts, Scale};
use mareader_settings::{AiSettings, PageTask, SummaryStyle, language_name};
use mareader_study::page::Rewritten;
use mareader_study::{Failure, Why};
use mareader_workspace::PaneId;

use crate::documents::{AiView, Documents};
use crate::gpu::Gpu;
use crate::ui_command::{Commands, Sent, UiCommand};
use mareader_engine::page_ai::extract::{self, Source};
pub use mareader_engine::page_ai::follow::{Follow, State, allowance, doing};
pub use mareader_engine::page_ai::groups::{
    Batch, Group, Read, TRIES, groups, room_for, size_for, span_of,
};
pub use mareader_engine::page_ai::ledger::Ledger;
pub use mareader_engine::page_ai::state::{
    Around, Asked, Cover, Drawn, Handle, Index, Key, Loose, News, Run, Showing, Who, canned, poll,
};
pub use mareader_engine::page_ai::view::{group_span, page_task, source_of};
use mareader_engine::stable_hash::hash;
pub use mareader_engine::study::loose::{loose_book, loose_id};
use mareader_engine::toast::Toast;
pub use paint::{Overlay, Pill, PillPress};

/// Seconds a result takes to fade in over its page.
const FADE_IN: f32 = 0.3;
/// Where the menu's Read aloud row says it is hovered, for its word.
fn aloud_hot(pane: PaneId) -> Id {
    Id::new(("page-ai-aloud-hot", pane))
}

/// Seconds the menu's page wash takes to come and to go.
const WASH_FADE: f32 = 0.22;
/// Seconds between settings of a result while it streams in.
const RESET: f64 = 0.4;
/// Results kept laid out for a book's pages before they are made again.
const MAX_MADE: usize = 64;

use mareader_engine::page_ai::Slot;

/// What pdf_view draws of the page AI in one pane.
#[derive(Clone, Default)]
pub struct PaneAi {
    pub overlays: Vec<Overlay>,
    pub pill: Option<Pill>,
    /// The pages the open menu would ask about, washed to show it.
    pub wash: Option<paint::Wash>,
}

/// A result to place: its task, span, words, source and identity.
type What = (AiTask, PageSpan, Arc<Rewritten>, Option<Arc<Source>>, u64);

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
    /// Picked pages, asked as groups; none for the page's own menu.
    pages: Vec<usize>,
    /// Where reading aloud would start, if the book can be read.
    aloud: Option<mareader_engine::read_aloud::ReadFrom>,
    /// Where on its page it was opened; none for picked pages.
    pin: Option<Pin>,
    /// When it first showed, for its entrance.
    born: Option<f64>,
}

/// A right-click's place on its page, so its menu rides along with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    /// The click, in page points.
    pub point: Pos2,
    /// The word under it, in page points, and its text.
    pub word: Option<mareader_pdf::Bounds>,
    pub text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BubbleMenu {
    pane: PaneId,
    page: usize,
    slot: Slot,
    at: Pos2,
}

/// The page AI's state across panes.
#[derive(Default)]
/// The page AI's state across panes.
#[derive(Default)]
pub struct PageAi {
    /// What a run keeps; the pixels stay here beside it.
    pub ledger: Ledger,
    cards: cards::Cards,
    menu: Option<Menu>,
    menu_rect: Option<Rect>,
    bubble_menu: Option<BubbleMenu>,
    bubble_rect: Option<Rect>,
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

/// The own pages a span covers, first and last.
fn pages_of(document: &Document, span: &PageSpan) -> Option<(usize, usize)> {
    match (document, span) {
        (_, PageSpan::Page { page }) => Some((*page, *page)),
        (_, PageSpan::Pages { from, to }) => Some((*from, *to)),
        (Document::Epub { book, .. }, PageSpan::Blocks { from, to }) => {
            let first = book.base_locate(anchor(*from));
            // An end is just past its last character, maybe on the next page.
            let [block, offset] = *to;
            let end = Anchor {
                block,
                offset: offset.saturating_sub(1),
            };
            Some((first, book.base_locate(end).max(first)))
        }
        (Document::Pdf(_), PageSpan::Blocks { .. }) => None,
    }
}

/// What a group's page past its result says, as a block to set.
fn group_note(task: &AiTask, (from, to): (usize, usize)) -> Arc<Vec<mareader_reflow::Block>> {
    use mareader_reflow::{Block, Content, Emphasis, Role, Text};
    let done = match task {
        AiTask::Summary | AiTask::Points => "summarized",
        AiTask::Translate { .. } => "translated",
        AiTask::Simplify => "simplified",
    };
    let words = format!(
        "Pages {}–{} are {done} together, starting on page {}.",
        from + 1,
        to + 1,
        from + 1
    );
    let mut text = Text::new(Role::Paragraph);
    let italic = Emphasis {
        italic: true,
        ..Emphasis::default()
    };
    text.push(&words, italic);
    Arc::new(vec![Block {
        content: Content::Text(text),
        frame: Default::default(),
        break_before: false,
    }])
}

/// "pages 3–7", counted from one.
fn pages_label((from, to): (usize, usize)) -> String {
    format!("pages {}–{}", from + 1, to + 1)
}

/// A kept span's end, as a place in the book.
fn anchor([block, offset]: [usize; 2]) -> Anchor {
    Anchor { block, offset }
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
                let id = loose_book(&mut self.ledger.loose.0, name);
                (&mut self.ledger.loose.0, id)
            }
        }
    }

    fn saved<'a>(&'a self, main: &'a Library, id: AiPageId) -> Option<&'a AiPage> {
        main.ai_page(id).or_else(|| self.ledger.loose.0.ai_page(id))
    }

    /// Starts or carries on `pane`'s run; what it says goes to `out`.
    pub fn follow(
        &mut self,
        asked: Asked<'_>,
        main: &Library,
        task: AiTask,
        fresh: bool,
        now: f64,
        out: &mut Commands,
    ) {
        self.ledger.follow(asked, main, task, fresh, now);
        self.toasts(out);
    }

    /// Carries a stopped run on from where it was; its words go to `out`.
    pub fn resume(&mut self, asked: Asked<'_>, main: &Library, now: f64, out: &mut Commands) {
        self.ledger.resume(asked, main, now);
        self.toasts(out);
    }

    /// Moves each run along one frame, and says what the groups asked.
    pub fn tick(&mut self, ctx: &Context, a: &Around<'_>, out: &mut Commands) {
        self.ledger.repaint = crate::repaint::of(ctx);
        let now = ctx.input(|i| i.time);
        self.ledger.tick(a, now);
        self.toasts(out);
    }

    /// What the runs asked to be said, as toasts.
    fn toasts(&mut self, out: &mut Commands) {
        for why in self.ledger.notes.drain(..) {
            out.push(Sent::Ui(UiCommand::Notify(Toast::info(why))));
        }
    }

    /// Writes `task` for the page; it shows over the page as it comes.
    pub fn run(&mut self, asked: Asked<'_>, task: AiTask, now: f64, out: &mut Commands) {
        self.ledger.redo(asked, task, None, now);
        self.toasts(out);
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
        let (task, old, span) = match slot {
            Slot::Saved(id) => match self.saved(main, id) {
                Some(p) => (p.task.clone(), Some(id), p.span.clone()),
                None => return,
            },
            Slot::Run(id) => match self.ledger.runs.iter().find(|r| r.id == id) {
                Some(r) => (r.task.clone(), r.redo_of, r.span.clone()),
                None => return,
            },
        };
        // A group is asked again whole, beside other groups under way.
        let group = asked.documents.ai_view(asked.pane).and_then(|view| {
            let (from, to) = pages_of(&view.document, &span)?;
            (to > from).then_some((view.layout, from, to))
        });
        if let Some((layout, from, to)) = group {
            if let Slot::Run(id) = slot {
                self.ledger.runs.retain(|r| r.id != id);
            }
            self.ledger.redo_group(asked, layout, (task, from, to), old);
            return;
        }
        self.ledger.redo(asked, task, old, now);
        self.toasts(out);
    }

    /// Takes in what the AI wrote; kept results go to the book.
    pub fn receive(
        &mut self,
        ctx: &Context,
        main: &mut Library,
        gpu: Option<&Gpu>,
        out: &mut Commands,
    ) {
        self.ledger.repaint = crate::repaint::of(ctx);
        let now = ctx.input(|i| i.time);
        let mut done = Vec::new();
        for run in &mut self.ledger.runs {
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
                        let batch = (self.ledger.batches.values())
                            .find(|b| b.runs.iter().any(|(id, _)| *id == run.id));
                        // A busy model takes the group later, as slots free.
                        if f.why == Why::InUse && batch.is_some() {
                            run.requeue = true;
                            continue;
                        }
                        let mut said = say(&f);
                        if (self.ledger.follows.values())
                            .any(|w| w.runs.contains(&run.id))
                        {
                            said.push_str(" Continuous mode stopped.");
                        }
                        if batch.is_some_and(|b| b.waiting_any()) {
                            said.push_str(" The pages left weren't asked.");
                        }
                        out.push(Sent::Ui(UiCommand::Notify(Toast::info(said.clone()))));
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
        self.ledger
            .showing
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
        let Some(i) = self.ledger.runs.iter().position(|r| r.id == id) else {
            return;
        };
        let run = self.ledger.runs.remove(i);
        let new = NewAiPage {
            task: run.task.clone(),
            span: run.span.clone(),
            source: run.source.key(),
            result: serde_json::to_string(&result).unwrap_or_default(),
            by: run.by.clone(),
        };
        let wall = mareader_engine::library::wall_ms();
        let (lib, book) = self.shelf(main, run.book, &run.name);
        let Some(saved) = lib.add_ai_page(book, new, wall) else {
            return;
        };
        if let Some(old) = run.redo_of.filter(|o| *o != saved) {
            lib.set_ai_pages_alive(&[old], false, wall);
        }
        self.ledger.parsed.insert(saved, Some(Arc::new(result)));
        match run.span {
            PageSpan::Blocks { from, to } => {
                self.ledger.spans.insert(saved, (anchor(from), anchor(to)));
            }
            PageSpan::Page { page } => {
                self.ledger.covers.insert(saved, (page, page));
            }
            PageSpan::Pages { from, to } => {
                self.ledger.covers.insert(saved, (from, to));
            }
        }
        let was = |s: &Showing| {
            s.slot == Slot::Run(id) || run.redo_of.is_some_and(|o| s.slot == Slot::Saved(o))
        };
        let keys: Vec<(Key, f64)> = (self.ledger.showing.iter())
            .filter(|(_, s)| was(s))
            .map(|(k, s)| (*k, s.since))
            .collect();
        for (key, since) in keys {
            self.ledger.show_on(key, Slot::Saved(saved), since);
        }
    }

    /// A kept result, read once.
    fn result(&mut self, main: &Library, id: AiPageId) -> Option<Arc<Rewritten>> {
        if let Some(r) = self.ledger.parsed.get(&id) {
            return r.clone();
        }
        let page = self.saved(main, id)?;
        let read = serde_json::from_str::<Rewritten>(&page.result)
            .ok()
            .map(Arc::new);
        self.ledger.parsed.insert(id, read.clone());
        read
    }

    /// Each result's page in `view`'s layout, read again on change.
    fn index(
        &mut self,
        main: &Library,
        view: &AiView,
        (book, name): (Option<BookId>, &str),
    ) -> Arc<HashMap<usize, Vec<AiPageId>>> {
        let (lib, id) = self.ledger.lib(main, book, name);
        let stamp = (lib.ai_pages_stamp(), id);
        let kept = self.ledger.index.get(&view.layout);
        if let Some(index) = kept.filter(|i| i.stamp == stamp) {
            return Arc::clone(&index.pages);
        }
        let pages = lib.ai_pages(id);
        let mut h = DefaultHasher::new();
        pages.iter().for_each(|p| p.id.hash(&mut h));
        let key = h.finish();
        let found = self.ledger.index.get(&view.layout);
        if found.is_some_and(|i| i.key == key) {
            let index = self.ledger.index.get_mut(&view.layout);
            return (index.map(|i| {
                i.stamp = stamp;
                Arc::clone(&i.pages)
            }))
            .unwrap_or_default();
        }
        let mut map: HashMap<usize, Vec<AiPageId>> = HashMap::new();
        let mut spans = Vec::new();
        let mut covers = Vec::new();
        for p in &pages {
            match p.span {
                PageSpan::Blocks { from, to } => spans.push((p.id, (anchor(from), anchor(to)))),
                PageSpan::Page { page } => covers.push((p.id, (page, page))),
                PageSpan::Pages { from, to } => covers.push((p.id, (from, to))),
            }
            let page = match p.span {
                PageSpan::Page { page } | PageSpan::Pages { from: page, .. } => Some(page),
                PageSpan::Blocks { from, .. } => base_page(&view.document, from),
            };
            if let Some(page) = page.filter(|p| *p < view.document.base_count()) {
                map.entry(page).or_default().push(p.id);
            }
        }
        if self.ledger.index.len() > 8 {
            self.ledger.index.clear();
            self.ledger.spans.clear();
            self.ledger.covers.clear();
        }
        self.ledger.spans.extend(spans);
        self.ledger.covers.extend(covers);
        let pages = Arc::new(map);
        let index = Index {
            key,
            stamp,
            pages: Arc::clone(&pages),
        };
        self.ledger.index.insert(view.layout, index);
        pages
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
        let pill = (self.ledger.pill(pane, now))
            .or_else(|| self.ledger.group_pill(pane));
        let wash = self.wash(ctx, pane);
        let Some(view) = documents.ai_view(pane) else {
            self.ledger.drawn.remove(&pane);
            return PaneAi {
                overlays: Vec::new(),
                pill,
                wash,
            };
        };
        self.prune(main);
        self.align(main, pane, &view, (book, name));
        let index = self.index(main, &view, (book, name));
        let groups = self.spread_groups(main, pane, &view);
        let mut overlays = Vec::new();
        let mut drawn = Vec::new();
        for page in view.visible.clone() {
            // A group's later pages carry on its result.
            let own = view.document.to_base(page);
            let over = (groups.iter()).find(|(from, to, _)| *from < own && own <= *to);
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
            if slots.is_empty() && over.is_none() {
                // A result's later pages: its menu works there too.
                let own = view.document.to_base(page);
                let key = (pane, view.layout, own);
                if view.document.base_pages(page).is_empty()
                    && let Some(shown) = self.ledger.showing.get(&key)
                {
                    drawn.push((page, vec![(shown.slot, own)]));
                }
                continue;
            }
            for (n, b) in bubbles.iter_mut().enumerate() {
                b.label = (n + 1).to_string();
            }
            let mut card =
                card.and_then(|(own, slot)| self.shown(ctx, pane, &view, (page, own), slot, main));
            if let (None, Some(&(from, _, slot))) = (&card, over) {
                card = self.shown(ctx, pane, &view, (page, from), slot, main);
                slots.push((slot, from));
            }
            overlays.push(Overlay {
                page,
                bubbles,
                card,
            });
            drawn.push((page, slots));
        }
        self.ledger.drawn.insert(pane, (view.layout, drawn));
        PaneAi {
            overlays,
            pill,
            wash,
        }
    }

    /// Groups shown over their pages: first and last own page, and the slot.
    fn spread_groups(
        &self,
        main: &Library,
        pane: PaneId,
        view: &AiView,
    ) -> Vec<(usize, usize, Slot)> {
        // A book lays kept groups in; only runs still writing spread.
        if view.document.is_reflowable() {
            return (self.ledger.showing.iter())
                .filter(|((p, l, _), _)| *p == pane && *l == view.layout)
                .filter(|(_, s)| self.carded(pane, view, s.slot))
                .filter_map(|(_, s)| {
                    let Slot::Run(id) = s.slot else {
                        return None;
                    };
                    let run = self.ledger.runs.iter().find(|r| r.id == id)?;
                    (run.last > run.page).then_some((run.page, run.last, s.slot))
                })
                .collect();
        }
        let span = |slot: Slot| match slot {
            Slot::Saved(id) => self.saved(main, id).map(|p| p.span.clone()),
            Slot::Run(id) => self
                .runs
                .iter()
                .find(|r| r.id == id)
                .map(|r| r.span.clone()),
        };
        (self.ledger.showing.iter())
            .filter(|((p, l, _), _)| *p == pane && *l == view.layout)
            .filter_map(|(_, s)| match span(s.slot)? {
                PageSpan::Pages { from, to } => Some((from, to, s.slot)),
                _ => None,
            })
            .collect()
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
        let runs = (self.ledger.runs.iter())
            .filter(|r| r.layout == view.layout && r.page == own)
            .filter(|r| r.redo_of.is_none_or(|o| !saved.contains(&o)));
        let shown = self.ledger.showing.get(&(pane, view.layout, own));
        let active = shown.map(|s| s.slot);
        let here = match &*view.document {
            Document::Epub { book, .. } => extract::span(&book.base_excerpts(own), book.blocks()),
            Document::Pdf(_) => None,
        };
        let mut out = Vec::new();
        for id in saved {
            let Some(p) = self.saved(main, *id) else {
                continue;
            };
            let redoing = self.ledger.runs.iter().find(|r| r.redo_of == Some(*id));
            let covered = pages_of(&view.document, &p.span).filter(|(a, b)| b > a);
            // A group is checked against all the pages it was asked for.
            let stale = match (&p.span, here, covered) {
                (PageSpan::Blocks { from, to }, Some(_), Some((_, last))) => {
                    group_span(&view.document, (own, last)) != Some([*from, *to])
                }
                (PageSpan::Blocks { from, to }, Some(span), None) => [*from, *to] != span,
                _ => false,
            };
            let state = match (redoing, stale) {
                (Some(r), _) if r.failed.is_none() => paint::State::Busy,
                (_, true) => paint::State::Stale,
                _ => paint::State::Ready,
            };
            let slot = Slot::Saved(*id);
            let mut tip = label(&p.task);
            if let Some(pages) = covered {
                tip = format!("{tip} · {}", pages_label(pages));
            }
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
            let mut what = label(&r.task);
            if r.last > r.page {
                what = format!("{what} · {}", pages_label((r.page, r.last)));
            }
            let (state, tip) = match &r.failed {
                Some(why) => (paint::State::Failed, format!("{what} · {why}")),
                None => (paint::State::Busy, format!("{what} · writing…")),
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
        if !view.document.is_reflowable() {
            return true;
        }
        let id = match slot {
            Slot::Saved(id) => id,
            // A redo writes behind the laid result; its bubble turns.
            Slot::Run(run) => {
                let run = self.ledger.runs.iter().find(|r| r.id == run);
                match run.and_then(|r| r.redo_of) {
                    Some(old) if view.document.shows(old.0) => return false,
                    _ => return true,
                }
            }
        };
        let asked = self.ledger.wants.get(&pane).map_or(0, |w| w.key);
        let laid = view.document.inserts_key() == asked;
        !laid && !view.document.shows(id.0)
    }

    /// Re-counts `pane`'s shown results once new type moves its pages.
    fn align(&mut self, main: &Library, pane: PaneId, view: &AiView, who: (Option<BookId>, &str)) {
        let old = self.ledger.aligned.insert(pane, view.layout);
        if old.is_none_or(|o| o == view.layout) {
            return;
        }
        let (lib, book) = self.ledger.lib(main, who.0, who.1);
        let page_of = |span: &PageSpan| match span {
            PageSpan::Page { page } | PageSpan::Pages { from: page, .. } => Some(*page),
            PageSpan::Blocks { from, .. } => base_page(&view.document, *from),
        };
        let moved: Vec<(Key, Option<usize>)> = (self.ledger.showing.iter())
            .filter(|((p, l, _), _)| *p == pane && *l != view.layout)
            .map(|(key, shown)| {
                let page = match shown.slot {
                    Slot::Saved(id) => {
                        let p = lib.ai_page(id).filter(|p| p.book.value == book);
                        p.and_then(|p| page_of(&p.span))
                    }
                    Slot::Run(id) => (self.ledger.runs.iter())
                        .find(|r| r.id == id && r.book == who.0 && r.name == who.1)
                        .and_then(|r| page_of(&r.span)),
                };
                (*key, page)
            })
            .collect();
        for (key, page) in moved {
            let Some(shown) = self.ledger.showing.remove(&key) else {
                continue;
            };
            let Some(page) = page else {
                continue;
            };
            if let Slot::Run(id) = shown.slot
                && let Some(r) = self.ledger.runs.iter_mut().find(|r| r.id == id)
            {
                (r.layout, r.page) = (view.layout, page);
            }
            self.ledger.showing.insert((pane, view.layout, page), shown);
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
            self.ledger.wants.remove(&pane);
            return None;
        };
        self.prune(main);
        self.align(main, pane, &view, who);
        // A redo keeps the result it redoes in the pages until done.
        let mut ids: Vec<AiPageId> = (self.ledger.showing.iter())
            .filter(|((p, l, _), _)| *p == pane && *l == view.layout)
            .filter_map(|(_, s)| match s.slot {
                Slot::Saved(id) => Some(id),
                Slot::Run(id) => self.ledger.runs.iter().find(|r| r.id == id)?.redo_of,
            })
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let key = match ids.is_empty() {
            true => 0,
            false => hash(("inserts", &ids)).max(1),
        };
        if let Some(w) = self.ledger.wants.get(&pane).filter(|w| w.key == key) {
            return Some(Arc::clone(w));
        }
        let mut list = Vec::new();
        for id in ids {
            if let Some(made) = self.ledger.made.get(&id) {
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
            if self.ledger.made.len() > MAX_MADE {
                self.ledger.made.clear();
            }
            self.ledger.made.insert(id, Arc::clone(&made));
            list.push(made);
        }
        let inserts = Arc::new(Inserts { key, list });
        self.ledger.wants.insert(pane, Arc::clone(&inserts));
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
        let since = self.ledger.showing.get(&(pane, view.layout, own))?.since;
        let old = match slot {
            Slot::Saved(id) => id,
            Slot::Run(id) => {
                let r = self.ledger.runs.iter().find(|r| r.id == id)?;
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
            Document::Epub { book, .. } if !book.fixed(page) => {
                let [x, y, w, h] = book.text_area(page);
                let width = book.page_size().0;
                (vec2(x, y), vec2(w, h), width)
            }
            _ => {
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
        let (at, mut room, width) = Self::room(view, page);
        // A group runs over its pages; a PDF's take its first page's room.
        let group = pages_of(&view.document, &span).filter(|(from, to)| to > from);
        let (pages, part, note) = match group {
            Some((from, to)) => {
                if !view.document.is_reflowable() {
                    room = Self::room(view, from).1;
                }
                let own = view.document.to_base(page);
                let note = group_note(&task, (from, to));
                (to - from + 1, own.saturating_sub(from), Some(note))
            }
            None => (1, 0, None),
        };
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
                        PageSpan::Page { .. } | PageSpan::Pages { .. } => Arc::default(),
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
                pages,
                part,
                note,
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
        let runs = &self.ledger.runs;
        let failed = match slot {
            Slot::Run(id) => runs.iter().any(|r| r.id == id && r.failed.is_some()),
            Slot::Saved(_) => false,
        };
        if failed {
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
        match self.ledger.showing.get(&key) {
            Some(s) if s.slot == slot => {
                self.ledger.showing.remove(&key);
            }
            _ => self.ledger.show_on(key, slot, now),
        }
    }

    /// The bubble `index` on page `page`: its layout, slot and own page.
    fn slot_at(&self, pane: PaneId, page: usize, index: usize) -> Option<(u64, Slot, usize)> {
        let (layout, pages) = self.ledger.drawn.get(&pane)?;
        let slots = &pages.iter().find(|(p, _)| *p == page)?.1;
        let (slot, own) = *slots.get(index)?;
        Some((*layout, slot, own))
    }

    /// Shows `slot` over the page, or the page itself.
    pub fn show_result(&mut self, pane: PaneId, page: usize, slot: Option<Slot>, now: f64) {
        let Some((layout, pages)) = self.ledger.drawn.get(&pane) else {
            return;
        };
        let layout = *layout;
        let slots = pages.iter().find(|(p, _)| *p == page);
        let slots = slots.map(|(_, s)| s.clone()).unwrap_or_default();
        match slot {
            Some(slot) => {
                if let Some((_, own)) = slots.iter().find(|(s, _)| *s == slot) {
                    self.ledger.show_on((pane, layout, *own), slot, now);
                }
            }
            None => {
                for (_, own) in slots {
                    self.ledger.showing.remove(&(pane, layout, own));
                }
            }
        }
    }

    /// A result's text, for the clipboard.
    pub fn text_of(&mut self, main: &Library, slot: Slot) -> Option<String> {
        let result = match slot {
            Slot::Saved(id) => self.result(main, id)?,
            Slot::Run(id) => self
                .ledger
                .runs
                .iter()
                .find(|r| r.id == id)?
                .shown
                .clone()?,
        };
        Some(extract::plain(&result))
    }

    /// Deletes a kept result, or stops one being written.
    pub fn delete(&mut self, main: &mut Library, slot: Slot) {
        match slot {
            Slot::Saved(id) => {
                let wall = mareader_engine::library::wall_ms();
                if main.set_ai_pages_alive(&[id], false, wall) == 0 {
                    self.ledger.loose.0.set_ai_pages_alive(&[id], false, wall);
                }
                self.forget(id);
                self.ledger.runs.retain(|r| r.redo_of != Some(id));
            }
            Slot::Run(id) => self.ledger.runs.retain(|r| r.id != id),
        }
        self.ledger.showing.retain(|_, s| s.slot != slot);
    }

    /// Drops a gone result's text from every cache.
    fn forget(&mut self, id: AiPageId) {
        self.ledger.parsed.remove(&id);
        self.ledger.made.remove(&id);
        self.ledger.spans.remove(&id);
        self.ledger.covers.remove(&id);
    }

    /// Takes down results deleted or erased elsewhere, text and all.
    fn prune(&mut self, main: &Library) {
        let gone: Vec<AiPageId> = (self.ledger.showing.values())
            .filter_map(|s| match s.slot {
                Slot::Saved(id) => self.saved(main, id).is_none().then_some(id),
                Slot::Run(_) => None,
            })
            .collect();
        for id in gone {
            self.forget(id);
            self.ledger.showing.retain(|_, s| s.slot != Slot::Saved(id));
        }
    }

    /// Forgets a closed pane: what it showed, drew and followed.
    pub fn close_pane(&mut self, pane: PaneId) {
        self.ledger.stop_follow(pane);
        self.ledger.stop_groups(pane);
        self.ledger.showing.retain(|(p, ..), _| *p != pane);
        self.ledger.drawn.remove(&pane);
        self.ledger.aligned.remove(&pane);
        self.ledger.wants.remove(&pane);
        if self.menu.as_ref().is_some_and(|m| m.pane == pane) {
            self.menu = None;
        }
        if self.bubble_menu.as_ref().is_some_and(|m| m.pane == pane) {
            self.bubble_menu = None;
        }
    }

    /// The menu's pages in `pane`, fading in with it and out after.
    fn wash(&mut self, ctx: &Context, pane: PaneId) -> Option<paint::Wash> {
        let open = self.menu.as_ref().is_some_and(|m| m.pane == pane);
        let id = Id::new(("page-ai-wash", pane));
        let alpha = ctx.animate_bool_with_time(id, open, WASH_FADE);
        let (at, pages) = self.ledger.washed.as_ref()?;
        if *at != pane {
            return None;
        }
        if alpha <= 0.0 && !open {
            self.ledger.washed = None;
            return None;
        }
        let hovered = ctx.data(|d| d.get_temp::<bool>(aloud_hot(pane)));
        let id = aloud_hot(pane).with("lit");
        let hot = ctx.animate_bool_with_time(id, open && hovered == Some(true), 0.15);
        Some(paint::Wash {
            pages: pages.clone(),
            alpha,
            word: self.ledger.washed_word,
            hot,
        })
    }

    /// Ties the page's open menu to where it was opened on its page.
    pub fn pin_menu(&mut self, pin: Pin) {
        if let Some(m) = &mut self.menu {
            m.pin = Some(pin);
        }
    }

    /// Where the page's menu was drawn last frame; for tests.
    pub fn menu_rect(&self) -> Option<Rect> {
        self.menu.as_ref().and(self.menu_rect)
    }

    /// The right-clicked word lit in `pane`, at or in it; for tests.
    pub fn washed_word(&self, pane: PaneId) -> Option<mareader_speech::Spot> {
        self.washed(pane).and(self.ledger.washed_word)
    }

    /// The pages washed in `pane`, while the menu shows or fades; for tests.
    pub fn washed(&self, pane: PaneId) -> Option<&[usize]> {
        let (at, pages) = self.ledger.washed.as_ref()?;
        (*at == pane).then_some(pages.as_slice())
    }

    /// Opens the page's menu at a right-click; `aloud` offers read-aloud.
    pub fn open_menu(
        &mut self,
        pane: PaneId,
        page: usize,
        at: Pos2,
        aloud: Option<mareader_engine::read_aloud::ReadFrom>,
    ) {
        self.bubble_menu = None;
        self.ledger.filter.clear();
        self.ledger.remember = false;
        self.ledger.washed = Some((pane, vec![page]));
        self.ledger.washed_word = aloud.and_then(|a| a.at);
        self.menu = Some(Menu {
            pane,
            page,
            at,
            step: Step::Tasks,
            pages: Vec::new(),
            aloud,
            pin: None,
            born: None,
        });
    }

    /// Opens the menu for picked pages at its languages, at `at`.
    pub fn open_pages_menu(&mut self, pane: PaneId, pages: Vec<usize>, [x, y]: [f32; 2]) {
        let at = Pos2 { x, y };
        let Some(&page) = pages.first() else {
            return;
        };
        self.bubble_menu = None;
        self.ledger.filter.clear();
        self.ledger.washed = Some((pane, pages.clone()));
        self.ledger.washed_word = None;
        self.menu = Some(Menu {
            pane,
            page,
            at,
            step: Step::Languages,
            pages,
            aloud: None,
            pin: None,
            born: None,
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
        let wall = mareader_engine::library::wall_ms();
        let (lib, book) = self.shelf(main, asked.book, asked.name);
        let Some(id) = lib.add_ai_page(book, new, wall) else {
            return false;
        };
        let slot = Slot::Saved(id);
        let key = (asked.pane, view.layout, page);
        self.ledger
            .showing
            .insert(key, Showing { slot, since: now });
        true
    }

    /// Whether `pane` shows a result over a page on screen now.
    pub fn on_screen(&self, pane: PaneId) -> bool {
        let Some((layout, pages)) = self.ledger.drawn.get(&pane) else {
            return false;
        };
        let mut slots = pages.iter().flat_map(|(_, slots)| slots);
        slots.any(|(_, own)| self.ledger.showing.contains_key(&(pane, *layout, *own)))
    }

    /// Tests: the kept results' tasks and spans, oldest first.
    pub fn kept(&self, main: &Library, who: (Option<BookId>, &str)) -> Vec<(AiTask, PageSpan)> {
        let (lib, book) = self.ledger.lib(main, who.0, who.1);
        let pages = lib.ai_pages(book).into_iter();
        pages.map(|p| (p.task.clone(), p.span.clone())).collect()
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
        self.ledger.inline = inline;
    }

    /// Tests and shots: runs show their words but never finish.
    pub fn set_held(&mut self, held: bool) {
        self.ledger.held = held;
    }

    /// Tests: runs answer at once, in capitals, with no AI.
    pub fn set_canned(&mut self, canned: bool) {
        self.ledger.canned = canned;
    }

    /// Whether `pane` shows a result over `page`.
    pub fn shows(&self, pane: PaneId, page: usize) -> bool {
        self.showing_on(pane, page).is_some()
    }

    /// Whether the page shows a result in `pane`, for the menu.
    fn showing_on(&self, pane: PaneId, page: usize) -> Option<Slot> {
        let (layout, pages) = self.ledger.drawn.get(&pane)?;
        let Some((_, slots)) = pages.iter().find(|(p, _)| *p == page) else {
            // Off screen: a book's pages there are taken as its own.
            let shown = self.ledger.showing.get(&(pane, *layout, page));
            return shown.map(|s| s.slot);
        };
        slots.iter().find_map(|(_, own)| {
            let shown = self.ledger.showing.get(&(pane, *layout, *own))?;
            Some(shown.slot)
        })
    }
}
