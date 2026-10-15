//! What each pane shows, and the per-frame traffic with the session.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Context, Rect, Ui};
use mareader_appearance::Rgb;
use mareader_library::BookId;
use mareader_session::{
    Anchor, Budget, DocId, Document, Event, Session, Source, Ticket, Typography, Want, typography,
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
use crate::selection::{Selection, Sheet};
use crate::texts::TextLayers;
use crate::theme::{self, Tokens};
use crate::thumbnails::{self, ThumbState};
use crate::toast::Toast;
use crate::zoom::{ZoomCommand, ZoomState};

/// Uploads per frame, so a burst of pages never stalls a scroll.
const UPLOADS: Budget = Budget {
    items: 8,
    bytes: 16 << 20,
};

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
    /// Read as this library book, a twin perhaps; none: the file's own.
    pub book: Option<BookId>,
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
        book: Option<BookId>,
    },
    Pdf(Box<PdfView>),
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
        self.document.page_count()
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
    /// What books are set in: the window's type, or a pane's own.
    window: Arc<Typography>,
    own: BTreeMap<PaneId, Arc<Typography>>,
    /// The re-set each pane last asked for: of which document, in what.
    asked: BTreeMap<PaneId, (DocId, Arc<Typography>)>,
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
            window: Arc::new(typography(&TextSettings::default())),
            own: BTreeMap::new(),
            asked: BTreeMap::new(),
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

    fn typo(&self, pane: PaneId) -> &Arc<Typography> {
        self.own.get(&pane).unwrap_or(&self.window)
    }

    /// Moves each book whose type is stale onto a twin in its type, or asks.
    fn retype(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        let (mut moves, mut asks) = (Vec::new(), Vec::new());
        for (pane, content) in &self.panes {
            let Content::Pdf(view) = content else {
                continue;
            };
            let want = self.own.get(pane).unwrap_or(&self.window);
            let Some(entry) = session.get(view.doc) else {
                continue;
            };
            if !entry.doc.is_reflowable() || *entry.typo == **want {
                continue;
            }
            // Another pane on the file shows it in this type already.
            let twin = self.panes.values().find_map(|c| match c {
                Content::Pdf(v) => {
                    let e = session.get(v.doc)?;
                    let same = entry.path.is_some() && e.path == entry.path;
                    (same && *e.typo == **want).then_some(v.doc)
                }
                Content::Opening { .. } => None,
            });
            match twin {
                Some(twin) => moves.push((*pane, view.doc, twin)),
                None => asks.push((*pane, view.doc, Arc::clone(want))),
            }
        }
        for (pane, doc, want) in asks {
            // One request serves every pane that wants it.
            if self.asked.values().any(|(d, t)| *d == doc && **t == *want) {
                continue;
            }
            session.retypeset(doc, Arc::clone(&want), pane.0);
            self.asked.insert(pane, (doc, want));
        }
        for (pane, old, new) in moves {
            self.retarget(pane, old, new);
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
        let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) else {
            return;
        };
        let count = doc.map_or(0, |d| d.page_count());
        // Unmoved since the last re-set: the same place.
        let place = view.place().or_else(|| previous?.anchor(view.current));
        let page = place.and_then(|a| doc?.locate(a));
        view.retypeset(new, page.unwrap_or(view.current), count, place);
        self.asked.remove(&pane);
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
        let (file, page, place) = (request.file(), request.page, request.place);
        let book = request.book;
        let typo = Arc::clone(self.typo(pane));
        if let Some(doc) = file.as_ref().and_then(|f| self.shown(f, &typo)) {
            let entry = self.session.as_ref().and_then(|s| s.get(doc));
            let count = entry.map_or(0, |e| e.doc.page_count());
            let page = landing(entry.map(|e| &*e.doc), page, place);
            let mut view = PdfView::new(doc);
            view.book = book;
            view.go_to(page, count);
            view.hold_place(place);
            self.set(pane, Some(Content::Pdf(Box::new(view))));
            return None;
        }
        let name = request.label();
        let ticket = self.session(ctx).open(request.source, request.name, typo);
        let opening = Content::Opening {
            ticket,
            name,
            file,
            page,
            place,
            book,
        };
        self.set(pane, Some(opening));
        None
    }

    pub fn close_pane(&mut self, pane: PaneId) {
        self.set(pane, None);
    }

    /// Replaces a pane's content, closing a document nobody shows.
    fn set(&mut self, pane: PaneId, content: Option<Content>) {
        let old = match content {
            Some(c) => self.panes.insert(pane, c),
            None => self.panes.remove(&pane),
        };
        self.asked.remove(&pane);
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
            if *entry.typo == *typo || !entry.doc.is_reflowable() {
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
        let mut retyped = Vec::new();
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
                Event::Retypeset { old, new, .. } => retyped.push((old, new)),
                Event::Opened { ticket, result } => {
                    let waiting = self.panes.iter().find_map(|(id, c)| match c {
                        Content::Opening {
                            ticket: t,
                            name,
                            page,
                            place,
                            book,
                            ..
                        } if *t == ticket => Some((*id, name.clone(), (*page, *place, *book))),
                        _ => None,
                    });
                    match (waiting, result) {
                        (Some((pane, _, (page, place, book))), Ok(doc)) => {
                            let mut view = PdfView::new(doc);
                            view.book = book;
                            let entry = session.get(doc);
                            let count = entry.map_or(0, |e| e.doc.page_count());
                            let page = landing(entry.map(|e| &*e.doc), page, place);
                            if page > 0 {
                                view.go_to(page, count);
                            }
                            view.hold_place(place);
                            self.panes.insert(pane, Content::Pdf(Box::new(view)));
                        }
                        (Some((pane, name, _)), Err(e)) => {
                            self.panes.remove(&pane);
                            let note = format!("Couldn't open {name}: {e}.");
                            out.push(Command::Notify(Toast::error(note)));
                        }
                        // The pane closed or moved on while this opened.
                        (None, Ok(doc)) => session.close(doc),
                        (None, Err(_)) => {}
                    }
                }
            }
        }
        for (old, new) in retyped {
            self.retyped(old, new);
        }
    }

    /// Moves the panes on `old` that want `new`'s type onto it.
    fn retyped(&mut self, old: DocId, new: DocId) {
        let session = self.session.as_ref();
        let Some(typo) = session
            .and_then(|s| s.get(new))
            .map(|e| Arc::clone(&e.typo))
        else {
            return;
        };
        let waiting: Vec<PaneId> = (self.panes.iter())
            .filter(|(pane, c)| {
                let on_old = matches!(c, Content::Pdf(v) if v.doc == old);
                on_old && **self.typo(**pane) == *typo
            })
            .map(|(pane, _)| *pane)
            .collect();
        for pane in waiting {
            self.retarget(pane, old, new);
        }
        self.release(new);
        self.release(old);
    }

    /// Draws the pane's document; false leaves the pane to its empty state.
    pub fn show(&mut self, ui: &mut Ui, pane: PaneId, cx: &ViewContext) -> bool {
        let entry = match (self.panes.get(&pane), &self.session) {
            (Some(Content::Pdf(v)), Some(s)) => s.get(v.doc).cloned(),
            (Some(Content::Opening { name, .. }), _) => {
                opening(ui, name, &cx.tokens);
                return true;
            }
            _ => None,
        };
        let (Some(entry), Some(Content::Pdf(view))) = (entry, self.panes.get_mut(&pane)) else {
            return false;
        };
        let io = Sinks {
            pages: &mut self.pages,
            demand: &mut self.demand,
            texts: &mut self.texts,
            drawn: &mut self.drawn,
            pass: self.gpu.as_ref().map(|g| &g.pass),
        };
        pdf_view::show(ui, view, &entry.doc, io, cx);
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
            return false;
        };
        let io = Sinks {
            pages: &mut self.pages,
            demand: &mut self.demand,
            texts: &mut self.texts,
            drawn: &mut Vec::new(),
            pass: self.gpu.as_ref().map(|g| &g.pass),
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
    pub fn end_frame(&mut self) {
        let mut demand = std::mem::take(&mut self.demand);
        demand.sort_by_key(|(urgency, _)| *urgency);
        if let Some(s) = &mut self.session {
            s.want(demand.into_iter().map(|(_, want)| want));
        }
        self.pages.end_frame();
        self.texts.end_frame();
    }

    /// The name of what `pane` shows, for the title bar.
    pub fn title(&self, pane: PaneId) -> Option<&str> {
        match self.panes.get(&pane)? {
            Content::Opening { name, .. } => Some(name),
            Content::Pdf(v) => self.session.as_ref()?.get(v.doc).map(|e| e.name.as_str()),
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

    /// `fit` fits the paged modes' width: the auto-scale setting.
    pub fn set_mode(&mut self, pane: PaneId, mode: ViewMode, fit: bool, now: f64) {
        if let Some(Content::Pdf(view)) = self.panes.get_mut(&pane) {
            view.set_mode(mode, fit, now);
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

    /// The file `pane` reads, its page and a book's place in it.
    pub fn place(&self, pane: PaneId) -> Option<(PathBuf, usize, Option<Anchor>)> {
        match self.panes.get(&pane)? {
            Content::Opening {
                file, page, place, ..
            } => Some((file.clone()?, *page, *place)),
            Content::Pdf(v) => {
                let entry = self.session.as_ref()?.get(v.doc)?;
                // Unmoved since a re-set: the place read before it.
                let place = v.place().or_else(|| entry.doc.anchor(v.current));
                Some((entry.path.clone()?, v.current, place))
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

    /// Open documents and cached page textures, for tests and meters.
    pub fn stats(&self) -> (usize, usize) {
        (
            self.session.as_ref().map_or(0, Session::len),
            self.pages.len(),
        )
    }
}

fn opening(ui: &mut Ui, name: &str, t: &Tokens) {
    let c = ui.max_rect().center();
    let mark = egui::Rect::from_center_size(c - egui::vec2(0.0, 16.0), egui::vec2(22.0, 22.0));
    loading_mark::paint(ui, mark, t.muted);
    ui.painter().text(
        c + egui::vec2(0.0, 18.0),
        egui::Align2::CENTER_CENTER,
        format!("Opening {name}…"),
        theme::medium(13.0),
        t.muted,
    );
}

/// Where an opened document starts: `place` in a book, else `page`.
fn landing(doc: Option<&Document>, page: usize, place: Option<Anchor>) -> usize {
    let found = place.and_then(|a| doc?.locate(a));
    found.unwrap_or(page)
}
