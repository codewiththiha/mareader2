//! What each pane shows, and the per-frame traffic with the session.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Context, Rect, Ui, Vec2};
use mareader_appearance::Rgb;
use mareader_library::{BookId, Spot};
use mareader_pdf::Bounds;
use mareader_session::{
    Anchor, Budget, DocId, Document, Entry, Event, Inserts, OpenTarget, Session, Source,
    SpinePlace, Ticket, Typography, Want, typography,
};
use mareader_settings::{PaperArea, TextSettings};
use mareader_workspace::{Edge, PaneId};

use crate::command::{Command, Commands};
use crate::gpu::{Gpu, Quad};
use crate::layout::{Arrow, ViewMode};
use crate::loading_mark;
use crate::pages::{PageCache, PageTexture};
use crate::paper::Papers;
use crate::pdf_view::{self, Demand, PdfView, Sinks, Urgency, ViewContext};
use crate::reading_place;
use crate::selection::{Selection, Sheet};
use crate::study::{self, Asked, Gloss};
use crate::texts::TextLayers;
use crate::theme::Tokens;
use crate::thumbnails::{self, ThumbState};
use crate::toast::Toast;
use crate::watch::{Reloads, Watch};
use crate::zoom::{ZoomCommand, ZoomState};

/// Uploads per frame, so a burst of pages never stalls a scroll.
const UPLOADS: Budget = Budget {
    items: 8,
    bytes: 16 << 20,
};

/// What the caches freed at rest goes back to the system, off the frame.
fn give_back() {
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::thread::Builder::new()
        .name("give-back".into())
        .spawn(|| {
            // Once the GPU's deferred frees have landed.
            std::thread::sleep(std::time::Duration::from_secs(1));
            mareader_memory::give_back();
        });
}

/// Where an open lands, measured from its pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    #[default]
    Into,
    /// A new pane to the right, else below when the right has no room.
    Beside,
    /// A new pane on exactly this edge, as a drop asks.
    Edge(Edge),
}

/// A request to show a file in a pane (the active one by default).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRequest {
    pub source: Source,
    pub name: Option<String>,
    pub pane: Option<PaneId>,
    /// Into that pane, or a new one split from it.
    pub placement: Placement,
    /// The page to show once open; past the end, the last.
    pub page: usize,
    /// A book's place to show instead, found again in today's type.
    pub place: Option<Anchor>,
    /// A stable EPUB location that avoids parsing the whole prefix.
    pub spine: Option<SpinePlace>,
    /// The last known count, for a preview's global page display.
    pub pages: usize,
    /// Read as this library book, a twin perhaps; none: the file's own.
    pub book: Option<BookId>,
}

/// A document's current global position and resume data.
pub struct DocumentPlace {
    /// The source file.
    pub path: PathBuf,
    /// The global page currently shown.
    pub page: usize,
    /// A legacy block anchor, when the document is complete.
    pub anchor: Option<Anchor>,
    /// A stable EPUB location, when available.
    pub spine: Option<SpinePlace>,
    /// The known or estimated full page count.
    pub pages: usize,
}

impl OpenRequest {
    pub fn path(path: PathBuf) -> Self {
        Self {
            source: Source::Path(path),
            name: None,
            pane: None,
            placement: Placement::Into,
            page: 0,
            place: None,
            spine: None,
            pages: 0,
            book: None,
        }
    }

    pub fn bytes(bytes: Vec<u8>, name: impl Into<String>) -> Self {
        Self {
            source: Source::Bytes(Arc::new(bytes)),
            name: Some(name.into()),
            pane: None,
            placement: Placement::Into,
            page: 0,
            place: None,
            spine: None,
            pages: 0,
            book: None,
        }
    }

    /// The file it reads from, if it reads from one.
    fn file(&self) -> Option<PathBuf> {
        match &self.source {
            Source::Path(p) => Some(p.clone()),
            Source::Bytes(_) => None,
        }
    }

    /// What to call it while it opens.
    fn label(&self) -> String {
        let path = match &self.source {
            Source::Path(p) => Some(p.to_string_lossy().into_owned()),
            Source::Bytes(_) => None,
        };
        let name = self.name.clone().or(path);
        name.as_deref()
            .and_then(mareader_pdf::file_stem)
            .unwrap_or_else(|| "document".into())
    }
}

#[derive(Debug)]
enum Content {
    /// Waiting for `ticket`; then it shows `page`.
    Opening {
        ticket: Ticket,
        name: String,
        file: Option<PathBuf>,
        page: usize,
        place: Option<Anchor>,
        spine: Option<SpinePlace>,
        pages: usize,
        book: Option<BookId>,
    },
    Pdf(Box<PdfView>),
}

/// One pane's document as the page AI sees it, as of last frame.
#[derive(Clone, Debug)]
pub struct AiView {
    pub doc: DocId,
    /// Names the pages the AI counts in: a book's own, results aside.
    pub layout: u64,
    pub document: Arc<Document>,
    /// What a result is set in.
    pub typo: Arc<Typography>,
    pub visible: std::ops::Range<usize>,
    /// The page read now.
    pub current: usize,
    pub zoom: f32,
}

/// One pane's document as navigation sees it, as of last frame.
#[derive(Clone, Debug)]
pub struct Reading {
    pub pane: PaneId,
    pub doc: DocId,
    pub document: Arc<Document>,
    pub current: usize,
    pub mode: ViewMode,
    pub progress: f32,
    pub name: String,
    /// The page read, on screen last frame.
    pub page: Option<Rect>,
}

impl Reading {
    pub fn count(&self) -> usize {
        self.document.display_page_count()
    }

    pub fn global_current(&self) -> usize {
        self.document.display_page(self.current)
    }
}

/// A count of what the documents hold; see [`Documents::meter`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Meter {
    pub documents: usize,
    pub pages: usize,
    /// Parts of pages too big for one texture.
    pub tiles: usize,
    /// The rasters' bytes, as the cache counts them against its cap.
    pub page_bytes: usize,
    pub texts: usize,
    /// Jobs queued or running.
    pub jobs: usize,
}

pub struct Documents {
    session: Option<Session>,
    workers: usize,
    pages: PageCache,
    texts: TextLayers,
    papers: Papers,
    panes: BTreeMap<PaneId, Content>,
    demand: Demand,
    gpu: Option<Gpu>,
    drawn: Vec<Quad>,
    /// What each pane's pages told the study tools this frame.
    gloss: Vec<(PaneId, Gloss)>,
    /// What books are set in: the window's type, or a pane's own.
    window: Arc<Typography>,
    own: BTreeMap<PaneId, Arc<Typography>>,
    /// The re-set each pane last asked for: of which document, in what.
    asked: BTreeMap<PaneId, (DocId, Arc<Typography>, u64)>,
    /// The AI results each pane's book shows in its pages.
    inserts: BTreeMap<PaneId, Arc<Inserts>>,
    /// Each paged pane's page size, so its next book opens at it.
    sheets: BTreeMap<PaneId, (f32, f32)>,
    /// When input or work last came, and whether caches shrank since.
    busy_at: f64,
    rested: bool,
    /// The pane read aloud, and where the words heard start.
    said: Option<(PaneId, Anchor)>,
    /// Open files, looked at for saves made elsewhere.
    watch: Option<Watch>,
    /// Files changed on disk, opening again.
    reloads: Reloads<Ticket, DocId>,
    /// The documents the watch was last given, in pane order.
    followed: Vec<DocId>,
}

/// Seconds without input or work before caches shrink to the pages in view.
const REST_AFTER: f64 = 3.0;
/// Air above the line read when a book set anew lands on it.
const LAND_AIR: f32 = 8.0;

/// Where `view` reads in `doc`, to find once the book is set anew.
fn reading_place(
    view: &PdfView,
    doc: Option<&Document>,
    texts: &TextLayers,
    id: DocId,
) -> Option<Anchor> {
    let doc = doc?;
    // Unmoved since the last re-set: the same place, so it never drifts.
    if let Some(at) = view.place() {
        return Some(at);
    }
    // A strip reads from its top line, its sentence's start if near.
    let line = view.reading_line().and_then(|(page, y)| {
        let band = doc.bands().and_then(|b| b.get(page)).map_or(0.0, |b| b.y);
        let held = texts.get(id, page);
        let layer = held.or_else(|| doc.text_layer(page).ok().map(Arc::new))?;
        reading_place::line_at(&layer, y + band)
    });
    line.or_else(|| doc.anchor(view.current))
}

impl Default for Documents {
    fn default() -> Self {
        // A browser has no threads to spare; jobs run between frames.
        let workers = if cfg!(target_arch = "wasm32") {
            0
        } else {
            Session::default_workers()
        };
        Self {
            session: None,
            workers,
            pages: PageCache::default(),
            texts: TextLayers::default(),
            papers: Papers::default(),
            panes: BTreeMap::new(),
            demand: Demand::new(),
            gpu: None,
            drawn: Vec::new(),
            gloss: Vec::new(),
            window: Arc::new(typography(&TextSettings::default())),
            own: BTreeMap::new(),
            asked: BTreeMap::new(),
            inserts: BTreeMap::new(),
            sheets: BTreeMap::new(),
            busy_at: 0.0,
            rested: true,
            said: None,
            watch: None,
            reloads: Reloads::default(),
            followed: Vec::new(),
        }
    }
}

impl Documents {
    /// Zero runs every job on the UI thread: deterministic, for tests.
    pub fn set_workers(&mut self, workers: usize) {
        if self.session.is_none() {
            self.workers = workers;
        }
    }

    /// Created on first use, when there is a context to wake.
    fn session(&mut self, ctx: &Context) -> &mut Session {
        let workers = self.workers;
        if self.session.is_none() {
            let ctx = ctx.clone();
            self.watch = Watch::new(move || ctx.request_repaint());
        }
        self.session.get_or_insert_with(|| {
            let ctx = ctx.clone();
            Session::new(workers, move || ctx.request_repaint())
        })
    }

    /// Each pane's book type: its `own` text, or `window`.
    pub fn set_text(&mut self, window: &TextSettings, own: &[(PaneId, &TextSettings)]) {
        let fresh = |old: Option<&Arc<Typography>>, text: &TextSettings| {
            let typo = typography(text);
            match old {
                Some(old) if **old == typo => Arc::clone(old),
                _ => Arc::new(typo),
            }
        };
        self.window = fresh(Some(&self.window), window);
        let own = own
            .iter()
            .map(|(pane, text)| (*pane, fresh(self.own.get(pane), text)));
        self.own = own.collect();
        self.retype();
    }

    /// The type `pane`'s book is set in now; none for a PDF or nothing.
    pub fn typography(&self, pane: PaneId) -> Option<Arc<Typography>> {
        let Some(Content::Pdf(view)) = self.panes.get(&pane) else {
            return None;
        };
        let entry = self.session.as_ref()?.get(view.doc)?;
        entry.doc.is_reflowable().then(|| Arc::clone(&entry.typo))
    }

    fn typo(&self, pane: PaneId) -> &Arc<Typography> {
        self.own.get(&pane).unwrap_or(&self.window)
    }

    /// `pane`'s book type on its pane-sized page, if it has one.
    fn want(&self, pane: PaneId) -> Arc<Typography> {
        let typo = self.typo(pane);
        match self.sheets.get(&pane).copied() {
            Some(sheet) if typo.sheet != Some(sheet) => Arc::new(Typography {
                sheet: Some(sheet),
                ..(**typo).clone()
            }),
            _ => Arc::clone(typo),
        }
    }

    /// Opens card `card`'s transcript in `pane`'s book; false if none shows.
    pub fn open_transcript(&mut self, pane: PaneId, card: usize, track: Option<usize>) -> bool {
        let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) else {
            return false;
        };
        view.transcripts.open(card, track);
        true
    }

    /// The transcripts `pane`'s book has open, as results; none if none.
    pub fn transcripts(
        &mut self,
        pane: PaneId,
        wanted: &[String],
    ) -> Option<(u64, Vec<Arc<mareader_epub::Insert>>)> {
        let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) else {
            return None;
        };
        if view.transcripts.is_empty() {
            return None;
        }
        let entry = self.session.as_ref()?.get(view.doc)?;
        let Document::Epub { book, .. } = &*entry.doc else {
            return None;
        };
        Some(view.transcripts.inserts(book, wanted))
    }

    /// What triggers show in `pane`'s book, as results; none if untouched.
    pub fn triggered(&mut self, pane: PaneId) -> Option<(u64, Vec<Arc<mareader_epub::Insert>>)> {
        let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) else {
            return None;
        };
        let entry = self.session.as_ref()?.get(view.doc)?;
        let Document::Epub { book, .. } = &*entry.doc else {
            return None;
        };
        view.triggers.inserts(book)
    }

    /// Each pane showing a book: its document and its scripted pages.
    pub fn scripted(&mut self) -> Vec<(PaneId, Arc<Document>, &mut crate::scripts::Paged)> {
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let views = self.panes.iter_mut().filter_map(|(pane, content)| {
            let Content::Pdf(view) = content else {
                return None;
            };
            let entry = session.get(view.doc)?;
            let book = matches!(&*entry.doc, Document::Epub { .. });
            let doc = Arc::clone(&entry.doc);
            book.then_some((*pane, doc, &mut view.triggers.script))
        });
        views.collect()
    }

    /// Whether `pane`'s scripts have made their pages and taken every click.
    pub fn scripts_idle(&self, pane: PaneId) -> bool {
        let Some(Content::Pdf(view)) = self.panes.get(&pane) else {
            return false;
        };
        let paged = &view.triggers.script;
        !paged.pages.is_empty() && paged.asks.is_empty()
    }

    /// What `pane`'s scripts have made, for a screenshot to report.
    pub fn scripts_state(&self, pane: PaneId) -> String {
        let Some(Content::Pdf(view)) = self.panes.get(&pane) else {
            return "no book in the pane".into();
        };
        let p = &view.triggers.script;
        let (pages, asks, changes) = (p.pages.len(), p.asks.len(), p.changes);
        format!(
            "{changes} changes, {pages} pages, {asks} asks; {:?}",
            p.errors
        )
    }

    /// Has `pane`'s scripts click what `selector` matches; false if not yet.
    pub fn pick_script(&mut self, pane: PaneId, selector: &str) -> bool {
        let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) else {
            return false;
        };
        let paged = &mut view.triggers.script;
        let Some(&chapter) = paged.pages.keys().next() else {
            return false;
        };
        let ask = crate::scripts::Ask::Pick(selector.to_owned());
        paged.asks.push((chapter, ask));
        true
    }

    /// Shows or hides trigger target `target` in `pane`; false if no pane.
    pub fn set_trigger(&mut self, pane: PaneId, target: usize, visible: bool) -> bool {
        let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) else {
            return false;
        };
        view.triggers.set(target, visible);
        true
    }

    /// Clips triggers were clicked to play, each with its book.
    pub fn take_clips(&mut self) -> Vec<(Arc<Document>, crate::trigger::ClipAction)> {
        let mut out = Vec::new();
        for content in self.panes.values_mut() {
            let Content::Pdf(view) = content else {
                continue;
            };
            let clips = std::mem::take(&mut view.triggers.clips);
            let entry = self.session.as_ref().and_then(|s| s.get(view.doc));
            if let Some(entry) = entry {
                out.extend(clips.into_iter().map(|c| (Arc::clone(&entry.doc), c)));
            }
        }
        out
    }

    /// The results `pane`'s book should show in its pages.
    pub fn set_inserts(&mut self, pane: PaneId, inserts: Arc<Inserts>) {
        if self.inserts.get(&pane).map_or(0, |i| i.key) == inserts.key {
            return;
        }
        match inserts.key {
            0 => self.inserts.remove(&pane),
            _ => self.inserts.insert(pane, inserts),
        };
        self.retype();
    }

    /// Hands what `old` drew to `new` once no pane shows `old`.
    fn hand_over(&mut self, old: DocId, new: DocId) {
        let shown = (self.panes.values()).any(|c| matches!(c, Content::Pdf(v) if v.doc == old));
        if !shown {
            self.adopt_page_rasters(old, new);
            self.adopt_text_layers(old, new);
        }
    }

    /// Partial links may resolve later, so only page rasters are reused.
    fn adopt_page_rasters(&mut self, old: DocId, new: DocId) {
        let Some((was, is)) = self.adoption_docs(old, new) else {
            return;
        };
        self.pages.adopt(old, new, |page| was.same_page(page, &is));
    }

    fn adopt_text_layers(&mut self, old: DocId, new: DocId) {
        let Some((was, is)) = self.adoption_docs(old, new) else {
            return;
        };
        self.texts.adopt(old, new, |page| was.same_page(page, &is));
    }

    fn adoption_docs(&self, old: DocId, new: DocId) -> Option<(Arc<Document>, Arc<Document>)> {
        let session = self.session.as_ref()?;
        let (was, is) = (session.get(old)?, session.get(new)?);
        Some((Arc::clone(&was.doc), Arc::clone(&is.doc)))
    }

    /// Moves each book whose type is stale onto a twin in its type, or asks.
    fn retype(&mut self) {
        let wants: BTreeMap<PaneId, Arc<Typography>> =
            self.panes.keys().map(|p| (*p, self.want(*p))).collect();
        let Some(session) = &mut self.session else {
            return;
        };
        let (mut moves, mut asks) = (Vec::new(), Vec::new());
        for (pane, content) in &self.panes {
            let Content::Pdf(view) = content else {
                continue;
            };
            let Some(want) = wants.get(pane) else {
                continue;
            };
            let inserts = self.inserts.get(pane).cloned().unwrap_or_default();
            let Some(entry) = session.get(view.doc) else {
                continue;
            };
            let key = inserts.key;
            let looks = |e: &Entry| *e.typo == **want && e.doc.inserts_key() == key;
            if !entry.doc.is_reflowable() || looks(entry) {
                continue;
            }
            // Another pane on the file shows it so already.
            let twin = self.panes.values().find_map(|c| match c {
                Content::Pdf(v) => {
                    let e = session.get(v.doc)?;
                    let same = entry.path.is_some() && e.path == entry.path;
                    (same && looks(e)).then_some(v.doc)
                }
                Content::Opening { .. } => None,
            });
            match twin {
                Some(twin) => moves.push((*pane, view.doc, twin)),
                None => asks.push((*pane, view.doc, (Arc::clone(want), inserts))),
            }
        }
        for (pane, doc, (want, inserts)) in asks {
            // One request serves every pane that wants it.
            let key = inserts.key;
            let same =
                |(d, t, k): &(DocId, Arc<Typography>, u64)| *d == doc && **t == *want && *k == key;
            if self.asked.values().any(same) {
                continue;
            }
            session.reset(doc, Arc::clone(&want), Some(inserts), pane.0);
            self.asked.insert(pane, (doc, want, key));
        }
        for (pane, old, new) in moves {
            self.retarget(pane, old, new);
            self.hand_over(old, new);
            self.release(old);
        }
    }

    /// Moves `pane` from `old` to `new`, the same book in other type.
    fn retarget(&mut self, pane: PaneId, old: DocId, new: DocId) {
        let Some(session) = &self.session else {
            return;
        };
        let doc = session.get(new).map(|e| &*e.doc);
        let previous = session.get(old).map(|e| &*e.doc);
        let said = self.said.filter(|(p, _)| *p == pane).map(|(_, a)| a);
        let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) else {
            return;
        };
        let count = doc.map_or(0, |d| d.page_count());
        // Results laid in or out: every other page stays as it was.
        if let (Some(was), Some(is)) = (previous, doc)
            && view.respliced(new, count, |p| was.same_page(p, is))
        {
            self.asked.remove(&pane);
            return;
        }
        let heard = said.filter(|a| {
            previous
                .and_then(|d| d.locate(*a))
                .is_some_and(|p| view.visible.contains(&p))
        });
        let old_place = heard.or_else(|| reading_place(view, previous, &self.texts, old));
        let stable = old_place.and_then(|at| previous.and_then(|d| d.spine_place(at)));
        let mapped_place = stable
            .as_ref()
            .and_then(|at| doc.and_then(|d| d.anchor_spine(at)))
            .or_else(|| {
                previous
                    .is_some_and(Document::is_complete)
                    .then_some(old_place)
                    .flatten()
            });
        let page = stable
            .as_ref()
            .and_then(|at| doc.and_then(|d| d.locate_spine(at)))
            .or_else(|| mapped_place.and_then(|at| doc?.locate(at)));
        view.retypeset(new, page.unwrap_or(view.current), count, mapped_place);
        // A strip shows the place's line on top in the new pages' first frame.
        if let (Some(doc), Some(at), Some(page)) = (doc, mapped_place, page)
            && let Ok(layer) = doc.text_layer(page)
            && let Some(y) = reading_place::top_of(&layer, at)
        {
            let band = doc.bands().and_then(|b| b.get(page)).map_or(0.0, |b| b.y);
            // Read-aloud's line clears where the find bar floats.
            let air = if heard.is_some() {
                pdf_view::FIND_CLEARANCE
            } else {
                LAND_AIR
            };
            view.land(page, y - band, air);
        }
        self.asked.remove(&pane);
    }

    /// Where `pane` reads now, as a book set anew would find it.
    pub fn place_read(&self, pane: PaneId) -> Option<Anchor> {
        let view = self.view(pane)?;
        let doc = self.session.as_ref()?.get(view.doc).map(|e| &*e.doc);
        reading_place(view, doc, &self.texts, view.doc)
    }

    /// Keeps where read-aloud is, for a book set anew to land there.
    pub fn hold_said(&mut self, said: Option<(PaneId, Anchor)>) {
        self.said = said;
    }

    /// Starts opening into `pane`; a refusal comes back as a note.
    pub fn open(&mut self, ctx: &Context, pane: PaneId, request: OpenRequest) -> Option<Toast> {
        let mut request = request;
        // One spelling per file, so two panes on it share one parse.
        if let Source::Path(p) = &request.source
            && let Ok(real) = std::fs::canonicalize(p)
        {
            request.source = Source::Path(real);
        }
        let name = request.label();
        let (file, page, place, spine, pages) = (
            request.file(),
            request.page,
            request.place,
            request.spine,
            request.pages,
        );
        let book = request.book;
        let typo = self.want(pane);
        let target = OpenTarget {
            page,
            place,
            spine: spine.clone(),
            pages,
        };
        if let Some(doc) = file.as_ref().and_then(|f| self.shown(f, &typo)) {
            let entry = self.session.as_ref().and_then(|s| s.get(doc));
            let page = entry.and_then(|entry| entry.doc.target_page(&target));
            let complete = entry.is_some_and(|entry| entry.doc.is_complete());
            if complete || page.is_some() {
                let count = entry.map_or(0, |entry| entry.doc.page_count());
                let page = page.unwrap_or_else(|| landing(entry.map(|e| &*e.doc), &target));
                let place = entry.and_then(|entry| entry.doc.target_anchor(&target));
                let mut view = PdfView::new(doc);
                view.book = book;
                view.go_to(page, count);
                view.hold_place(place);
                self.set(pane, Some(Content::Pdf(Box::new(view))));
                return None;
            }
        }
        let ticket = self
            .session(ctx)
            .open_at(request.source, request.name, typo, target);
        let opening = Content::Opening {
            ticket,
            name,
            file,
            page,
            place,
            spine,
            pages,
            book,
        };
        self.set(pane, Some(opening));
        None
    }

    /// Whether a document is still opening.
    pub fn opening(&self) -> bool {
        (self.panes.values()).any(|c| matches!(c, Content::Opening { .. }))
            || self.session.as_ref().is_some_and(Session::opening)
    }

    pub fn close_pane(&mut self, pane: PaneId) {
        self.set(pane, None);
        self.sheets.remove(&pane);
    }

    /// Replaces a pane's content, closing a document nobody shows.
    fn set(&mut self, pane: PaneId, content: Option<Content>) {
        let old = match content {
            Some(c) => self.panes.insert(pane, c),
            None => self.panes.remove(&pane),
        };
        self.asked.remove(&pane);
        self.inserts.remove(&pane);
        if let Some(Content::Pdf(view)) = old {
            self.release(view.doc);
        }
    }

    fn release(&mut self, doc: DocId) {
        let shown = self
            .panes
            .values()
            .any(|c| matches!(c, Content::Pdf(v) if v.doc == doc));
        if !shown {
            if let Some(s) = &mut self.session {
                s.close(doc);
            }
            self.pages.forget(doc);
            self.texts.forget(doc);
            self.papers.forget(doc);
        }
    }

    /// The open document read from `file`, preferably set in `typo`.
    fn shown(&self, file: &std::path::Path, typo: &Typography) -> Option<DocId> {
        let session = self.session.as_ref()?;
        let open = self.panes.values().filter_map(|c| match c {
            Content::Pdf(v) => {
                let entry = session.get(v.doc)?;
                (entry.path.as_deref() == Some(file)).then_some((v.doc, entry))
            }
            Content::Opening { .. } => None,
        });
        let (mut first, mut matched) = (None, None);
        for (doc, entry) in open {
            first = first.or(Some(doc));
            let plain = *entry.typo == *typo && entry.doc.inserts_key() == 0;
            if plain || !entry.doc.is_reflowable() {
                matched = Some(doc);
                break;
            }
        }
        matched.or(first)
    }

    /// Takes finished work: documents into their panes, pages to the GPU.
    pub fn receive(&mut self, ctx: &Context, out: &mut Commands) {
        let Some(session) = &mut self.session else {
            return;
        };
        let mut transitions = Vec::new();
        let mut reloaded = Vec::new();
        for event in session.poll(UPLOADS) {
            match event {
                Event::Rendered { key, result } => match result {
                    Ok(raster) => {
                        let texture = match &self.gpu {
                            Some(gpu) => gpu.upload(&raster).map(PageTexture::Gpu),
                            None => Some(PageTexture::egui(ctx, key, &raster)),
                        };
                        match texture {
                            Some(t) => self.pages.insert(key, t, raster.bytes()),
                            None => self.pages.fail(key),
                        }
                    }
                    Err(_) => self.pages.fail(key),
                },
                Event::Text { doc, page, result } => self.texts.insert(doc, page, result),
                Event::Paper { doc, page, result } => self.papers.insert(doc, page, result),
                Event::Retypeset { old, new, .. } => transitions.push((true, old, new)),
                Event::Preview { ticket, doc } => {
                    let waiting = self.panes.iter().find_map(|(id, content)| match content {
                        Content::Opening {
                            ticket: pending,
                            name,
                            page,
                            place,
                            spine,
                            pages,
                            book,
                            ..
                        } if *pending == ticket => Some((
                            *id,
                            name.clone(),
                            OpenTarget {
                                page: *page,
                                place: *place,
                                spine: spine.clone(),
                                pages: *pages,
                            },
                            *book,
                        )),
                        _ => None,
                    });
                    if let Some((pane, _, target, book)) = waiting {
                        let entry = session.get(doc);
                        let count = entry.map_or(0, |e| e.doc.page_count());
                        let page = landing(entry.map(|e| &*e.doc), &target);
                        let place = entry.and_then(|e| e.doc.target_anchor(&target));
                        let mut view = PdfView::new(doc);
                        view.book = book;
                        if page > 0 {
                            view.go_to(page, count);
                        }
                        view.hold_place(place);
                        self.panes.insert(pane, Content::Pdf(Box::new(view)));
                    } else {
                        session.close(doc);
                    }
                }
                Event::Completed { old, new, .. } => transitions.push((false, old, new)),
                Event::Incomplete { doc, error, .. } => {
                    let name = session
                        .get(doc)
                        .map_or_else(|| "document".to_string(), |e| e.name.clone());
                    out.push(Command::Notify(open_incomplete(&name, &error)));
                }
                Event::Opened { ticket, result } if self.reloads.has(&ticket) => {
                    if let Some((old, path, again)) = self.reloads.finish(&ticket) {
                        // Unreadable mid-save: the old one stays.
                        reloaded.push((old, result.ok(), path, again));
                    }
                }
                Event::Opened { ticket, result } => {
                    let waiting = self.panes.iter().find_map(|(id, content)| match content {
                        Content::Opening {
                            ticket: pending,
                            name,
                            page,
                            place,
                            spine,
                            pages,
                            book,
                            ..
                        } if *pending == ticket => Some((
                            *id,
                            name.clone(),
                            OpenTarget {
                                page: *page,
                                place: *place,
                                spine: spine.clone(),
                                pages: *pages,
                            },
                            *book,
                        )),
                        _ => None,
                    });
                    match (waiting, result) {
                        (Some((pane, name, target, book)), Ok(doc)) => {
                            let entry = session.get(doc);
                            if entry.is_some_and(|e| e.doc.cut_short()) {
                                out.push(Command::Notify(cut_short(&name)));
                            }
                            let count = entry.map_or(0, |e| e.doc.page_count());
                            let page = landing(entry.map(|e| &*e.doc), &target);
                            let place = entry.and_then(|e| e.doc.target_anchor(&target));
                            let mut view = PdfView::new(doc);
                            view.book = book;
                            if page > 0 {
                                view.go_to(page, count);
                            }
                            view.hold_place(place);
                            self.panes.insert(pane, Content::Pdf(Box::new(view)));
                        }
                        (Some((pane, name, _, _)), Err(e)) => {
                            self.panes.remove(&pane);
                            out.push(Command::Notify(open_failed(&name, &e)));
                        }
                        // The pane closed or moved on while this opened.
                        (None, Ok(doc)) => session.close(doc),
                        (None, Err(_)) => {}
                    }
                }
            }
        }
        for (old, new, path, again) in reloaded {
            if let Some(new) = new {
                self.reloaded(old, new);
            }
            if again {
                self.reload(&path);
            }
        }
        for (is_retype, old, new) in transitions {
            if is_retype {
                self.retyped(old, new);
            } else {
                self.adopt_page_rasters(old, new);
                self.reloaded(old, new);
            }
        }
        self.follow_files();
    }

    /// Hands the watch the files shown, and opens again those saved since.
    fn follow_files(&mut self) {
        let (Some(session), Some(watch)) = (&self.session, &mut self.watch) else {
            return;
        };
        let shown = || {
            self.panes.values().filter_map(|c| match c {
                Content::Pdf(v) => Some(v.doc),
                Content::Opening { .. } => None,
            })
        };
        // Most frames show what the last did: nothing to hand over.
        if !shown().eq(self.followed.iter().copied()) {
            self.followed = shown().collect();
            let files = self.followed.iter();
            watch.follow(
                files
                    .filter_map(|d| session.get(*d)?.path.clone())
                    .collect(),
            );
        }
        for path in watch.changed() {
            self.reload(&path);
        }
    }

    /// Opens `path`'s documents again; panes keep the old ones meanwhile.
    pub fn reload(&mut self, path: &std::path::Path) {
        let Some(session) = &mut self.session else {
            return;
        };
        let mut docs: Vec<DocId> = (self.panes.values())
            .filter_map(|c| match c {
                Content::Pdf(v) => Some(v.doc),
                Content::Opening { .. } => None,
            })
            .filter(|d| {
                session
                    .get(*d)
                    .is_some_and(|e| e.path.as_deref() == Some(path))
            })
            .collect();
        docs.sort();
        docs.dedup();
        for doc in docs {
            if !self.reloads.should_start(doc) {
                continue;
            }
            let Some(entry) = session.get(doc) else {
                continue;
            };
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            let typo = Arc::clone(&entry.typo);
            let ticket = session.open(Source::Path(path.to_path_buf()), name, typo);
            self.reloads.started(ticket, doc, path.to_path_buf());
        }
    }

    /// Moves every pane on `old` onto `new`, its file as saved since.
    fn reloaded(&mut self, old: DocId, new: DocId) {
        let panes: Vec<PaneId> = (self.panes.iter())
            .filter(|(_, c)| matches!(c, Content::Pdf(v) if v.doc == old))
            .map(|(pane, _)| *pane)
            .collect();
        let count = (self.session.as_ref())
            .and_then(|s| s.get(new))
            .filter(|e| !e.doc.is_reflowable())
            .map(|e| e.doc.page_count());
        for pane in panes {
            match (count, self.panes.get_mut(&pane)) {
                // Pages stay pages: the same one, scrolled as it was.
                (Some(count), Some(Content::Pdf(view))) => {
                    if !view.respliced(new, count, |p| (p < count).then_some(p)) {
                        view.retypeset(new, count, count, None);
                    }
                }
                _ => self.retarget(pane, old, new),
            }
        }
        self.release(new);
        self.release(old);
        // AI results go back into the pages they were laid in.
        self.retype();
    }

    /// Moves the panes on `old` that want `new`'s look onto it.
    fn retyped(&mut self, old: DocId, new: DocId) {
        let session = self.session.as_ref();
        let Some((typo, key)) = session
            .and_then(|s| s.get(new))
            .map(|e| (Arc::clone(&e.typo), e.doc.inserts_key()))
        else {
            return;
        };
        let waiting: Vec<PaneId> = (self.panes.iter())
            .filter(|(pane, c)| {
                let on_old = matches!(c, Content::Pdf(v) if v.doc == old);
                let wants = self.inserts.get(*pane).map_or(0, |i| i.key) == key;
                on_old && wants && *self.want(**pane) == *typo
            })
            .map(|(pane, _)| *pane)
            .collect();
        for pane in waiting {
            self.retarget(pane, old, new);
        }
        self.hand_over(old, new);
        self.release(new);
        self.release(old);
    }

    /// Draws the pane's document; false leaves the pane to its empty state.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        pane: PaneId,
        cx: &ViewContext,
        vocab: Option<&mut crate::vocab::Vocab>,
    ) -> bool {
        let entry = match (self.panes.get(&pane), &self.session) {
            (Some(Content::Pdf(v)), Some(s)) => s.get(v.doc).cloned(),
            (Some(Content::Opening { .. }), _) => {
                opening(ui, &cx.tokens);
                return true;
            }
            _ => None,
        };
        let (Some(entry), Some(Content::Pdf(view))) = (entry, self.panes.get_mut(&pane)) else {
            return false;
        };
        let mut gloss = Vec::new();
        let io = Sinks {
            pages: &mut self.pages,
            demand: &mut self.demand,
            texts: &mut self.texts,
            drawn: &mut self.drawn,
            pass: self.gpu.as_ref().map(|g| &g.pass),
            gloss: &mut gloss,
            vocab,
        };
        pdf_view::show(ui, view, &entry.doc, io, cx);
        // A settled pane size sets its book anew on pages that fit.
        let moved = std::mem::take(&mut view.sheet_moved);
        if moved {
            match view.sheet() {
                Some(sheet) => self.sheets.insert(pane, sheet),
                None => self.sheets.remove(&pane),
            };
        }
        self.gloss.extend(gloss.into_iter().map(|g| (pane, g)));
        if cx.sample {
            let count = entry.doc.page_count();
            for page in self.papers.wanted(view.doc, view.visible.clone(), count) {
                let want = Want::Paper {
                    doc: view.doc,
                    page,
                };
                self.demand.push((Urgency::Paper, want));
            }
        }
        if moved {
            self.retype();
        }
        true
    }

    /// The paper behind `pane`'s pages where it is looked at, if known.
    pub fn paper(&self, pane: PaneId, area: PaperArea) -> Option<Rgb> {
        let Some(Content::Pdf(view)) = self.panes.get(&pane) else {
            return None;
        };
        self.papers.at(view.doc, area, view.paper_at())
    }

    /// What `pane` reads, if it shows an open document.
    pub fn reading(&self, pane: PaneId) -> Option<Reading> {
        let Some(Content::Pdf(view)) = self.panes.get(&pane) else {
            return None;
        };
        let entry = self.session.as_ref()?.get(view.doc)?;
        Some(Reading {
            pane,
            doc: view.doc,
            document: Arc::clone(&entry.doc),
            current: view.current,
            mode: view.mode,
            progress: view.progress,
            name: entry.name.clone(),
            page: view
                .sheets
                .iter()
                .find(|s| s.page == view.current)
                .map(|s| s.rect),
        })
    }

    /// What the page AI reads of `pane`'s document.
    pub fn ai_view(&self, pane: PaneId) -> Option<AiView> {
        let Some(Content::Pdf(view)) = self.panes.get(&pane) else {
            return None;
        };
        let entry = self.session.as_ref()?.get(view.doc)?;
        // A book keeps the type it was set in; a PDF takes the pane's.
        let typo = match entry.doc.is_reflowable() {
            true => Arc::clone(&entry.typo),
            false => Arc::clone(self.typo(pane)),
        };
        // A PDF's pages never move: its own name serves.
        let layout = entry.doc.layout().unwrap_or(view.doc.get() | 1 << 63);
        Some(AiView {
            doc: view.doc,
            layout,
            document: Arc::clone(&entry.doc),
            typo,
            visible: view.visible.clone(),
            current: view.current,
            zoom: view.zoom.sharp(),
        })
    }

    /// Page `page`'s text, if it has come.
    pub fn text_layer(&self, doc: DocId, page: usize) -> Option<Arc<mareader_session::TextLayer>> {
        self.texts.get(doc, page)
    }

    /// The thumbnail grid for `pane`'s document; false if it has none.
    pub fn thumbnails(
        &mut self,
        ui: &mut Ui,
        pane: PaneId,
        state: &mut ThumbState,
        recentre: bool,
        out: &mut Commands,
    ) -> bool {
        let Some(reading) = self.reading(pane) else {
            state.forget_book();
            return false;
        };
        let io = Sinks {
            pages: &mut self.pages,
            demand: &mut self.demand,
            texts: &mut self.texts,
            drawn: &mut Vec::new(),
            pass: self.gpu.as_ref().map(|g| &g.pass),
            gloss: &mut Vec::new(),
            vocab: None,
        };
        thumbnails::show(ui, &reading, io, state, recentre, out);
        true
    }

    /// Pages go through our page pass once egui's renderer is known.
    pub fn set_gpu(&mut self, gpu: Gpu) {
        self.gpu = Some(gpu);
    }

    pub fn gpu(&self) -> Option<&Gpu> {
        self.gpu.as_ref()
    }

    /// The GPU pages drawn this frame, leaving none for the next.
    pub fn take_drawn(&mut self) -> Vec<Quad> {
        std::mem::take(&mut self.drawn)
    }

    /// Publishes this frame's demand, most urgent first; trims the cache.
    pub fn end_frame(&mut self, ctx: &Context) {
        let mut demand = std::mem::take(&mut self.demand);
        demand.sort_by_key(|(urgency, _)| *urgency);
        if let Some(s) = &mut self.session {
            s.want(demand.into_iter().map(|(_, want)| want));
        }
        self.settle(ctx);
        self.pages.end_frame();
        self.texts.end_frame();
    }

    /// A while after the last input or job, caches shrink to what's shown.
    fn settle(&mut self, ctx: &Context) {
        let (now, input) = ctx.input(|i| {
            let p = &i.pointer;
            (
                i.time,
                !i.events.is_empty() || p.is_moving() || p.any_down(),
            )
        });
        let working = self.session.as_ref().is_some_and(|s| s.pending() > 0);
        if input || working {
            self.busy_at = now;
            self.rested = false;
            // A frame once it has gone quiet, to rest in.
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(REST_AFTER));
        } else if !self.rested && now - self.busy_at >= REST_AFTER {
            self.rested = true;
            if let Some(s) = &self.session {
                s.rest();
            }
            self.pages.rest();
            self.texts.rest();
            give_back();
        }
    }

    /// The name of what `pane` shows, for the title bar.
    pub fn title(&self, pane: PaneId) -> Option<&str> {
        match self.panes.get(&pane)? {
            Content::Opening { name, .. } => Some(name),
            Content::Pdf(v) => self.session.as_ref()?.get(v.doc).map(|e| e.name.as_str()),
        }
    }

    /// The file `pane` shows, or is opening.
    pub fn path(&self, pane: PaneId) -> Option<&std::path::Path> {
        match self.panes.get(&pane)? {
            Content::Opening { file, .. } => file.as_deref(),
            Content::Pdf(v) => self.session.as_ref()?.get(v.doc)?.path.as_deref(),
        }
    }

    /// The library book `pane` reads as, or will once open.
    pub fn book(&self, pane: PaneId) -> Option<BookId> {
        match self.panes.get(&pane)? {
            Content::Opening { book, .. } => *book,
            Content::Pdf(v) => v.book,
        }
    }

    fn view(&self, pane: PaneId) -> Option<&PdfView> {
        match self.panes.get(&pane)? {
            Content::Pdf(v) => Some(v.as_ref()),
            Content::Opening { .. } => None,
        }
    }

    /// What the panes' pages told the study tools this frame.
    pub fn take_gloss(&mut self) -> Vec<(PaneId, Gloss)> {
        std::mem::take(&mut self.gloss)
    }

    /// The selected word and its place on screen; none unless one word.
    pub fn asked(&self, pane: PaneId) -> Option<(Asked, Rect)> {
        let view = self.view(pane)?;
        let sel = view.selection.filter(|s| !s.is_empty())?;
        let doc = &self.session.as_ref()?.get(view.doc)?.doc;
        let layer = |page| {
            let held = self.texts.get(view.doc, page);
            held.or_else(|| doc.text_layer(page).ok().map(Arc::new))
        };
        let asked = study::words::asked(sel, layer)?;
        let sheet = view.sheets.iter().find(|s| s.page == asked.page)?;
        let held = layer(asked.page)?;
        let boxes = held.text.line_boxes(asked.chars.clone());
        let on_screen = boxes.map(|b| sheet.to_screen(b));
        let rect = on_screen.reduce(|a, b| a.union(b))?;
        Some((asked, rect))
    }

    /// Where `chars` of `page` were drawn last frame, if on screen.
    pub fn hit_box(&self, pane: PaneId, page: usize, chars: Range<usize>) -> Option<Rect> {
        let Some(Content::Pdf(view)) = self.panes.get(&pane) else {
            return None;
        };
        let sheet = view.sheets.iter().find(|s| s.page == page)?;
        let layer = self.texts.get(view.doc, page)?;
        let spot = layer.text.line_boxes(chars).reduce(Bounds::union)?;
        Some(sheet.to_screen(spot))
    }

    /// Brings a hit into `pane`'s view; false while its page's text is due.
    pub fn reveal_hit(&mut self, pane: PaneId, page: usize, chars: Range<usize>) -> bool {
        self.bring(pane, page, chars, 0.0)
    }

    /// Brings `chars` into view, `clear` above the pane's foot.
    fn bring(&mut self, pane: PaneId, page: usize, chars: Range<usize>, clear: f32) -> bool {
        let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) else {
            return true;
        };
        let Some(entry) = self.session.as_ref().and_then(|s| s.get(view.doc)) else {
            return true;
        };
        let layer = self.texts.get(view.doc, page);
        let spot = layer.and_then(|l| l.text.line_boxes(chars).reduce(Bounds::union));
        let band = entry.doc.bands().and_then(|b| b.get(page));
        let band = band.map_or(Vec2::ZERO, |b| egui::vec2(b.x, b.y));
        view.seek(page, spot, band, entry.doc.page_count(), clear)
    }

    /// Brings read-aloud's line up, `clear` above the foot; false until done.
    pub fn reveal_said(
        &mut self,
        pane: PaneId,
        page: usize,
        chars: Range<usize>,
        clear: f32,
    ) -> bool {
        let gliding = match self.panes.get(&pane) {
            Some(Content::Pdf(view)) => view.gliding(),
            _ => return true,
        };
        !gliding && self.bring(pane, page, chars, clear)
    }

    /// The word at `chars` of `page`, as Explain asks for it.
    pub fn asked_at(&self, pane: PaneId, page: usize, chars: Range<usize>) -> Option<Asked> {
        let view = self.view(pane)?;
        let layer = self.texts.get(view.doc, page)?;
        study::words::asked_at(&layer, page, chars)
    }

    /// The pane's selected text for a lookup, and where it shows.
    pub fn looked_up(&self, pane: PaneId) -> Option<(String, Rect)> {
        let view = self.view(pane)?;
        let sel = view.selection.filter(|s| !s.is_empty())?;
        let doc = &self.session.as_ref()?.get(view.doc)?.doc;
        let layer = |page| {
            let held = self.texts.get(view.doc, page);
            held.or_else(|| doc.text_layer(page).ok().map(Arc::new))
        };
        let on_sheet = |sheet: &crate::selection::Sheet| {
            let l = layer(sheet.page)?;
            let range = sel.on_page(sheet.page, l.text.len())?;
            let boxes = l.text.line_boxes(range).map(|b| sheet.to_screen(b));
            boxes.reduce(|a, b| a.union(b))
        };
        let rect = view
            .sheets
            .iter()
            .filter_map(on_sheet)
            .reduce(|a, b| a.union(b))?;
        Some((sel.text(layer), rect))
    }

    /// The pane's selection as a highlight's spot and text.
    pub fn highlighted(&self, pane: PaneId) -> Option<(Spot, String)> {
        let view = self.view(pane)?;
        let sel = view.selection.filter(|s| !s.is_empty())?;
        let doc = &self.session.as_ref()?.get(view.doc)?.doc;
        let layer = |page| {
            let held = self.texts.get(view.doc, page);
            held.or_else(|| doc.text_layer(page).ok().map(Arc::new))
        };
        let spot = study::words::span(sel, layer)?;
        Some((spot, sel.text(layer)))
    }

    /// Selects the first `text` on `page` in the pane; false if absent.
    pub fn select(&mut self, pane: PaneId, page: usize, text: &str) -> bool {
        let doc = self.view(pane).map(|v| v.doc);
        let entry = doc.and_then(|d| self.session.as_ref()?.get(d));
        let Some(layer) = entry.and_then(|e| e.doc.text_layer(page).ok()) else {
            return false;
        };
        let chars: Vec<char> = layer.text.chars().iter().map(|c| c.ch).collect();
        let needle: Vec<char> = text.chars().collect();
        let found = chars
            .windows(needle.len().max(1))
            .position(|w| w == needle.as_slice());
        match (found, self.panes.get_mut(&pane)) {
            (Some(at), Some(Content::Pdf(v))) => {
                v.selection = Some(Selection::within(page, at..at + needle.len()));
                true
            }
            _ => false,
        }
    }

    /// The pane's selection, if any text is selected.
    pub fn selection(&self, pane: PaneId) -> Option<Selection> {
        self.view(pane)?.selection.filter(|s| !s.is_empty())
    }

    /// Clears the pane's selection; false if there was none.
    pub fn clear_selection(&mut self, pane: PaneId) -> bool {
        match self.panes.get_mut(&pane) {
            Some(Content::Pdf(v)) => v.selection.take().is_some_and(|s| !s.is_empty()),
            _ => false,
        }
    }

    /// Pins where reading will start in `pane`, in place of any other.
    pub fn place_read_marker(&mut self, pane: PaneId, spot: mareader_speech::Spot, now: f64) {
        self.dismiss_read_markers(now);
        self.clear_selection(pane);
        if let Some(Content::Pdf(v)) = self.panes.get_mut(&pane) {
            v.marker = Some(crate::read_aloud::marker::Marker::new(spot));
        }
    }

    /// Brings each new pin's word into view, once the page holds still.
    pub fn reveal_read_markers(&mut self) {
        let due: Vec<(PaneId, mareader_speech::Spot)> = (self.panes.iter())
            .filter_map(|(pane, content)| match content {
                Content::Pdf(v) => (v.marker.as_ref())
                    .filter(|m| !m.revealed && !m.leaving())
                    .map(|m| (*pane, m.spot)),
                Content::Opening { .. } => None,
            })
            .collect();
        for (pane, spot) in due {
            let Some(view) = self.view(pane) else {
                continue;
            };
            let layer = self.texts.get(view.doc, spot.page);
            let span = layer.and_then(|l| crate::read_aloud::marker::word_span(&l.text, spot.char));
            let chars = span.unwrap_or(spot.char..spot.char + 1);
            if self.reveal_said(pane, spot.page, chars, 0.0)
                && let Some(Content::Pdf(v)) = self.panes.get_mut(&pane)
                && let Some(m) = &mut v.marker
            {
                m.revealed = true;
            }
        }
    }

    /// Where a right-click at `at` sat on `page`, and the word it was on.
    pub fn menu_pin(
        &self,
        pane: PaneId,
        page: usize,
        at: egui::Pos2,
        caret: Option<usize>,
    ) -> Option<crate::page_ai::Pin> {
        let view = self.view(pane)?;
        let sheet = view.sheets.iter().find(|s| s.page == page)?;
        let (x, y) = sheet.to_page(at);
        let layer = self.texts.get(view.doc, page);
        let span = (layer.as_ref().zip(caret))
            .and_then(|(l, c)| crate::read_aloud::marker::word_span(&l.text, c));
        let word = (layer.as_ref().zip(span.clone()))
            .and_then(|(l, s)| l.text.line_boxes(s).reduce(Bounds::union));
        let text = (layer.as_ref().zip(span)).map(|(l, s)| study::words::text_of(l, s));
        Some(crate::page_ai::Pin {
            point: egui::pos2(x, y),
            word,
            text,
        })
    }

    /// Where the pane's pin waits to start reading, if it does.
    pub fn read_marker(&self, pane: PaneId) -> Option<mareader_speech::Spot> {
        let m = self.view(pane)?.marker.as_ref()?;
        (!m.leaving()).then_some(m.spot)
    }

    /// Fades every waiting pin out; true if one was waiting.
    pub fn dismiss_read_markers(&mut self, now: f64) -> bool {
        let mut any = false;
        for content in self.panes.values_mut() {
            if let Content::Pdf(v) = content
                && let Some(m) = &mut v.marker
            {
                any |= m.dismiss(now);
            }
        }
        any
    }

    /// How far the pane's document was scrolled last frame.
    pub fn offset(&self, pane: PaneId) -> Option<egui::Vec2> {
        self.view(pane).map(PdfView::offset)
    }

    /// The pane's pages on screen last frame.
    pub fn sheets(&self, pane: PaneId) -> &[Sheet] {
        self.view(pane).map_or(&[], |v| v.sheets.as_slice())
    }

    /// Text layers held, for tests and meters.
    pub fn text_layers(&self) -> usize {
        self.texts.len()
    }

    pub fn zoom(&mut self, pane: PaneId, command: ZoomCommand, now: f64) {
        if let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) {
            view.command(command, now);
        }
    }

    pub fn set_mode(&mut self, pane: PaneId, mode: ViewMode, now: f64) {
        if let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) {
            view.set_mode(mode, now);
        }
    }

    pub fn arrow(&mut self, pane: PaneId, arrow: Arrow, now: f64) {
        let count = self.page_count(pane).unwrap_or(0);
        if let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) {
            view.arrow(arrow, count, now);
        }
    }

    pub fn page_key(&mut self, pane: PaneId, forward: bool) {
        let count = self.page_count(pane).unwrap_or(0);
        if let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) {
            view.page_key(forward, count);
        }
    }

    pub fn go_to(&mut self, pane: PaneId, page: usize) {
        let count = self.page_count(pane).unwrap_or(0);
        if let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) {
            view.go_to(page, count);
        }
    }

    pub fn turn(&mut self, pane: PaneId, forward: bool) {
        let count = self.page_count(pane).unwrap_or(0);
        if let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) {
            view.turn(forward, count);
        }
    }

    /// Auto-scroll on or off in `pane`; false where it can't run.
    pub fn toggle_drift(&mut self, pane: PaneId, now: f64) -> bool {
        match self.panes.get_mut(&pane) {
            Some(Content::Pdf(view)) => view.toggle_drift(now),
            _ => false,
        }
    }

    /// Stops `pane`'s auto-scroll; true if it was running.
    pub fn stop_drift(&mut self, pane: PaneId) -> bool {
        match self.panes.get_mut(&pane) {
            Some(Content::Pdf(view)) => view.stop_drift(),
            _ => false,
        }
    }

    /// Whether `pane` auto-scrolls, if it shows a document.
    pub fn drifting(&self, pane: PaneId) -> Option<bool> {
        match self.panes.get(&pane)? {
            Content::Pdf(v) => Some(v.drifting()),
            Content::Opening { .. } => None,
        }
    }

    /// The mode `pane` shows its document in.
    pub fn mode(&self, pane: PaneId) -> Option<ViewMode> {
        match self.panes.get(&pane)? {
            Content::Pdf(v) => Some(v.mode),
            Content::Opening { .. } => None,
        }
    }

    /// The zoom `pane` shows, if it shows a document.
    pub fn zoom_state(&self, pane: PaneId) -> Option<ZoomState> {
        match self.panes.get(&pane)? {
            Content::Pdf(v) => Some(v.zoom.state()),
            Content::Opening { .. } => None,
        }
    }

    /// The page under `pane`'s centre last frame.
    pub fn current(&self, pane: PaneId) -> Option<usize> {
        match self.panes.get(&pane)? {
            Content::Pdf(v) => Some(v.current),
            Content::Opening { .. } => None,
        }
    }

    /// Pages `pane` showed last frame.
    pub fn visible(&self, pane: PaneId) -> Option<std::ops::Range<usize>> {
        match self.panes.get(&pane)? {
            Content::Pdf(v) => Some(v.visible.clone()),
            Content::Opening { .. } => None,
        }
    }

    /// Whether `pane` shows a book whose text reflows.
    pub fn reflowable(&self, pane: PaneId) -> bool {
        let Some(Content::Pdf(v)) = self.panes.get(&pane) else {
            return false;
        };
        let entry = self.session.as_ref().and_then(|s| s.get(v.doc));
        entry.is_some_and(|e| e.doc.is_reflowable())
    }

    pub fn page_count(&self, pane: PaneId) -> Option<usize> {
        match self.panes.get(&pane)? {
            Content::Pdf(v) => self
                .session
                .as_ref()?
                .get(v.doc)
                .map(|e| e.doc.page_count()),
            Content::Opening { .. } => None,
        }
    }

    /// Resume data for the open file in `pane`.
    pub fn place(&self, pane: PaneId) -> Option<DocumentPlace> {
        match self.panes.get(&pane)? {
            Content::Opening {
                file,
                page,
                place,
                spine,
                pages,
                ..
            } => Some(DocumentPlace {
                path: file.clone()?,
                page: *page,
                anchor: *place,
                spine: spine.clone(),
                pages: *pages,
            }),
            Content::Pdf(view) => {
                let entry = self.session.as_ref()?.get(view.doc)?;
                let anchor = view.place().or_else(|| entry.doc.anchor(view.current));
                let spine = anchor.and_then(|at| entry.doc.spine_place(at));
                let legacy = entry.doc.is_complete().then_some(anchor).flatten();
                Some(DocumentPlace {
                    path: entry.path.clone()?,
                    page: entry.doc.global_page(view.current),
                    anchor: legacy,
                    spine,
                    pages: entry.doc.global_page_count(),
                })
            }
        }
    }

    /// What the documents hold right now, for meters and the memory audit.
    pub fn meter(&self) -> Meter {
        Meter {
            documents: self.session.as_ref().map_or(0, Session::len),
            pages: self.pages.len(),
            tiles: self.pages.tiles(),
            page_bytes: self.pages.bytes(),
            texts: self.texts.len(),
            jobs: self.session.as_ref().map_or(0, Session::pending),
        }
    }

    /// The document open as `doc`, as it is set now.
    pub fn document(&self, doc: DocId) -> Option<&Arc<Document>> {
        self.session.as_ref()?.get(doc).map(|e| &e.doc)
    }

    /// Open documents and cached page textures, for tests and meters.
    pub fn stats(&self) -> (usize, usize) {
        (
            self.session.as_ref().map_or(0, Session::len),
            self.pages.len(),
        )
    }
}

const OPENING_MARK: f32 = 48.0;

fn opening_mark_rect(rect: Rect) -> Rect {
    Rect::from_center_size(rect.center(), Vec2::splat(OPENING_MARK))
}

fn opening(ui: &mut Ui, t: &Tokens) {
    let mark = opening_mark_rect(ui.max_rect());
    loading_mark::paint(ui, mark, t.muted);
}

#[cfg(test)]
mod opening_mark_tests {
    use super::*;

    #[test]
    fn the_opening_mark_is_large_and_centered() {
        let pane = Rect::from_min_size(egui::pos2(20.0, 30.0), Vec2::new(400.0, 300.0));
        let mark = opening_mark_rect(pane);
        assert_eq!(mark.center(), pane.center());
        assert_eq!(mark.size(), Vec2::splat(OPENING_MARK));
    }
}

/// Where an opened document starts: its stable place, else its page.
fn landing(doc: Option<&Document>, target: &OpenTarget) -> usize {
    doc.and_then(|document| document.target_page(target))
        .unwrap_or(target.page)
}
/// Why `name` didn't open, and what to try, as one toast.
fn open_failed(name: &str, e: &mareader_session::OpenError) -> Toast {
    Toast::error(match e.hint() {
        Some(hint) => format!("Couldn't open {name}: {e}. {hint}"),
        None => format!("Couldn't open {name}: {e}."),
    })
}

fn open_incomplete(name: &str, e: &mareader_session::OpenError) -> Toast {
    Toast::warning(format!(
        "Couldn't finish loading {name}: {e}. Pages already opened remain available."
    ))
}

/// `name` opened, but its end is missing.
fn cut_short(name: &str) -> Toast {
    Toast::warning(format!(
        "{name} looks cut short; some pages may be missing."
    ))
}

#[cfg(test)]
mod open_notes {
    use super::*;
    use mareader_pdf::OpenError as Pdf;
    use mareader_session::OpenError;

    #[test]
    fn a_failure_says_why_and_what_to_try() {
        let locked = open_failed("deed.pdf", &OpenError::Pdf(Pdf::Locked));
        assert!(
            locked
                .message
                .starts_with("Couldn't open deed.pdf: the PDF is locked")
        );
        assert!(locked.message.ends_with("save an unlocked copy."));
        let none = open_failed("a.pdf", &OpenError::Pdf(Pdf::NoPages));
        assert_eq!(none.message, "Couldn't open a.pdf: the PDF has no pages.");
        assert_eq!(cut_short("a.pdf").tone, crate::toast::Tone::Warning);
    }
}
