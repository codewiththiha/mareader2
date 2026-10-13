//! What each pane shows, and the per-frame traffic with the session.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Context, Ui};
use mareader_session::{Budget, DocId, Event, Session, Source, Ticket};
use mareader_workspace::PaneId;

use crate::command::{Command, Commands};
use crate::pages::PageCache;
use crate::pdf_view::{self, Demand, PdfView, ViewContext};
use crate::theme::{self, Tokens};
use crate::toast::Toast;

/// Uploads per frame, so a burst of pages never stalls a scroll.
const UPLOADS: Budget = Budget {
    items: 8,
    bytes: 16 << 20,
};

/// A request to show a file in a pane (the active one by default).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRequest {
    pub source: Source,
    pub name: Option<String>,
    pub pane: Option<PaneId>,
}

impl OpenRequest {
    pub fn path(path: PathBuf) -> Self {
        Self {
            source: Source::Path(path),
            name: None,
            pane: None,
        }
    }

    pub fn bytes(bytes: Vec<u8>, name: impl Into<String>) -> Self {
        Self {
            source: Source::Bytes(Arc::new(bytes)),
            name: Some(name.into()),
            pane: None,
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

    fn is_epub(&self) -> bool {
        let path = match &self.source {
            Source::Path(p) => p.to_str(),
            Source::Bytes(_) => None,
        };
        self.name
            .as_deref()
            .or(path)
            .is_some_and(|n| n.to_lowercase().ends_with(".epub"))
    }
}

#[derive(Debug)]
enum Content {
    Opening { ticket: Ticket, name: String },
    Pdf(PdfView),
}

pub struct Documents {
    session: Option<Session>,
    workers: usize,
    pages: PageCache,
    panes: BTreeMap<PaneId, Content>,
    demand: Demand,
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
            panes: BTreeMap::new(),
            demand: Demand::new(),
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

    /// Starts opening into `pane`; a refusal comes back as a note.
    pub fn open(&mut self, ctx: &Context, pane: PaneId, request: OpenRequest) -> Option<Toast> {
        if request.is_epub() {
            let note = "EPUB books aren't supported yet; open a PDF for now.";
            return Some(Toast::info(note));
        }
        let name = request.label();
        let ticket = self.session(ctx).open(request.source, request.name);
        self.set(pane, Some(Content::Opening { ticket, name }));
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
        }
    }

    /// Takes finished work: documents into their panes, pages to the GPU.
    pub fn receive(&mut self, ctx: &Context, out: &mut Commands) {
        let Some(session) = &mut self.session else {
            return;
        };
        for event in session.poll(UPLOADS) {
            match event {
                Event::Rendered { key, result } => match result {
                    Ok(raster) => self.pages.insert(ctx, key, &raster),
                    Err(_) => self.pages.fail(key),
                },
                Event::Opened { ticket, result } => {
                    let waiting = self.panes.iter().find_map(|(id, c)| match c {
                        Content::Opening { ticket: t, name } if *t == ticket => {
                            Some((*id, name.clone()))
                        }
                        _ => None,
                    });
                    match (waiting, result) {
                        (Some((pane, _)), Ok(doc)) => {
                            self.panes.insert(pane, Content::Pdf(PdfView::new(doc)));
                        }
                        (Some((pane, name)), Err(e)) => {
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
    }

    /// Draws the pane's document; false leaves the pane to its empty state.
    pub fn show(&mut self, ui: &mut Ui, pane: PaneId, cx: &ViewContext) -> bool {
        let entry = match (self.panes.get(&pane), &self.session) {
            (Some(Content::Pdf(v)), Some(s)) => s.get(v.doc).cloned(),
            (Some(Content::Opening { name, .. }), _) => {
                opening(ui, name);
                return true;
            }
            _ => None,
        };
        let (Some(entry), Some(Content::Pdf(view))) = (entry, self.panes.get_mut(&pane)) else {
            return false;
        };
        pdf_view::show(ui, view, &entry.doc, &mut self.pages, &mut self.demand, cx);
        true
    }

    /// Publishes this frame's demand, most urgent first; trims the cache.
    pub fn end_frame(&mut self) {
        let mut demand = std::mem::take(&mut self.demand);
        demand.sort_by_key(|(urgency, _)| *urgency);
        if let Some(s) = &mut self.session {
            s.want(demand.into_iter().map(|(_, key)| key));
        }
        self.pages.end_frame();
    }

    /// The name of what `pane` shows, for the title bar.
    pub fn title(&self, pane: PaneId) -> Option<&str> {
        match self.panes.get(&pane)? {
            Content::Opening { name, .. } => Some(name),
            Content::Pdf(v) => self.session.as_ref()?.get(v.doc).map(|e| e.name.as_str()),
        }
    }

    /// Pages `pane` showed last frame.
    pub fn visible(&self, pane: PaneId) -> Option<std::ops::Range<usize>> {
        match self.panes.get(&pane)? {
            Content::Pdf(v) => Some(v.visible.clone()),
            Content::Opening { .. } => None,
        }
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

    /// Open documents and cached page textures, for tests and meters.
    pub fn stats(&self) -> (usize, usize) {
        (
            self.session.as_ref().map_or(0, Session::len),
            self.pages.len(),
        )
    }
}

fn opening(ui: &mut Ui, name: &str) {
    let t = Tokens::get(ui.ctx());
    let c = ui.max_rect().center();
    let spinner = egui::Rect::from_center_size(c - egui::vec2(0.0, 14.0), egui::vec2(20.0, 20.0));
    ui.put(spinner, egui::Spinner::new().size(20.0).color(t.muted));
    ui.painter().text(
        c + egui::vec2(0.0, 16.0),
        egui::Align2::CENTER_CENTER,
        format!("Opening {name}…"),
        theme::medium(13.0),
        t.muted,
    );
}
