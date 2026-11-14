//! A host with no window: real documents, and marks kept in a list.
//!
//! It opens what it is asked to through `mareader-session`, reads the text
//! `mareader-pdf` finds, and writes exports to a folder. Controllers no test
//! here drives answer nothing.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use mareader_appearance::{Appearance, Rgb};
use mareader_engine::dict::{Action as DictAction, Download, Window};
use mareader_engine::documents::{OpenRequest, Placement};
use mareader_engine::export::{Entry, Format, file_name};
use mareader_engine::host::Fake;
use mareader_engine::library::Action as LibraryAction;
use mareader_engine::page_ai::Action as PageAiAction;
use mareader_engine::pane::{Arrow, ViewMode, ZoomCommand};
use mareader_engine::read_aloud::ReadFrom;
use mareader_engine::route::Route;
use mareader_engine::session::Session;
use mareader_engine::settings::Tab;
use mareader_engine::study::Action as StudyAction;
use mareader_engine::toast::Toast;
use mareader_engine::view::{EdgeTab, View};
use mareader_library::{BookId, Pen};
use mareader_session::{Budget, Document, Event as Opened};
use mareader_settings::{ReadAloudSettings, WordList};
use mareader_speech::{Item, Model, Spot};
use mareader_workspace::{Bounds, PaneId};

/// What one pane reads, and where in it.
pub struct Open {
    pub doc: Arc<Document>,
    pub page: usize,
    /// The page and the words picked on it.
    pub selection: Option<(usize, String)>,
}

pub struct Headless {
    pub session: Session,
    docs: mareader_session::Session,
    pub panes: BTreeMap<PaneId, Open>,
    pub marks: Vec<Entry>,
    /// Where exports land.
    pub out: PathBuf,
    pub area: Bounds,
    pub arrivals: Vec<Route>,
}

impl Headless {
    pub fn new(out: PathBuf) -> Self {
        Self {
            session: Session::new(Arc::new(Fake::default()), Arc::new(|| {})),
            docs: mareader_session::Session::new(0, || {}),
            panes: BTreeMap::new(),
            marks: Vec::new(),
            out,
            area: Bounds::new(0.0, 0.0, 1200.0, 800.0),
            arrivals: Vec::new(),
        }
    }

    /// The text of the page `pane` shows.
    pub fn page_text(&self, pane: PaneId) -> String {
        let Some(open) = self.panes.get(&pane) else {
            return String::new();
        };
        let layer = open.doc.text_layer(open.page);
        layer
            .map(|l| l.text.text(0..l.text.len()))
            .unwrap_or_default()
    }
}

impl View for Headless {
    fn session(&mut self) -> &mut Session {
        &mut self.session
    }

    fn area(&self) -> Bounds {
        self.area
    }

    fn edge_tab(&self) -> EdgeTab {
        EdgeTab::default()
    }

    fn arrived(&mut self, route: Route) {
        self.arrivals.push(route);
    }

    fn close_pane(&mut self, pane: PaneId) {
        self.panes.remove(&pane);
    }

    fn resume(&mut self, request: OpenRequest) -> Option<OpenRequest> {
        Some(request)
    }

    fn open(&mut self, pane: PaneId, request: OpenRequest) -> Option<Toast> {
        self.docs
            .open(request.source, request.name, Default::default());
        let budget = Budget {
            items: 1,
            bytes: usize::MAX,
        };
        let opened = self.docs.poll(budget).into_iter().find_map(|e| match e {
            Opened::Opened { result, .. } => Some(result),
            _ => None,
        });
        let entry = match opened {
            Some(Ok(id)) => self.docs.get(id),
            _ => None,
        };
        let Some(entry) = entry else {
            return Some(Toast::error("The document didn't open."));
        };
        let doc = Arc::clone(&entry.doc);
        let page = request.page.min(doc.page_count().saturating_sub(1));
        let selection = None;
        self.panes.insert(
            pane,
            Open {
                doc,
                page,
                selection,
            },
        );
        None
    }

    fn zoom(&mut self, _: PaneId, _: ZoomCommand, _: f64) {}

    fn set_mode(&mut self, _: PaneId, _: ViewMode, _: f64) {}

    fn arrow(&mut self, _: PaneId, _: Arrow, _: f64) {}

    fn page_key(&mut self, pane: PaneId, forward: bool) {
        self.turn(pane, forward);
    }

    fn toggle_drift(&mut self, _: PaneId, _: f64) -> bool {
        false
    }

    fn go_to(&mut self, pane: PaneId, page: usize) {
        if let Some(open) = self.panes.get_mut(&pane) {
            open.page = page.min(open.doc.page_count().saturating_sub(1));
        }
    }

    fn turn(&mut self, pane: PaneId, forward: bool) {
        let Some(at) = self.panes.get(&pane).map(|open| open.page) else {
            return;
        };
        let to = if forward {
            at.saturating_add(1)
        } else {
            at.saturating_sub(1)
        };
        self.go_to(pane, to);
    }

    fn copy_pages(&mut self, _: PaneId, _: Vec<usize>) {}

    fn save(&mut self, name: String, _: Format, contents: String) {
        let _ = std::fs::write(self.out.join(name), contents);
    }

    fn book(&self, _: PaneId) -> Option<BookId> {
        None
    }

    fn path(&self, _: PaneId) -> Option<PathBuf> {
        None
    }

    fn restart_at_start(&mut self, panes: &[PaneId]) -> Vec<Toast> {
        for pane in panes {
            self.go_to(*pane, 0);
        }
        Vec::new()
    }

    fn set_appearance(&mut self, _: Appearance) {}

    fn open_settings(&mut self, _: Tab) {}

    fn word_list(&mut self, _: PaneId, _: Vec<usize>, _: WordList) {}

    fn word_window(&mut self, _: PaneId, _: Vec<usize>, _: WordList) {}

    fn study(&mut self, _: StudyAction) {}

    fn explain(&mut self, _: PaneId) {}

    fn look_up(&mut self, _: PaneId) {}

    fn highlight(&mut self, pane: PaneId, pen: Pen, color: Option<Rgb>) {
        let Some(open) = self.panes.get_mut(&pane) else {
            return;
        };
        let Some((page, quote)) = open.selection.take() else {
            return;
        };
        self.marks.push(Entry {
            quote,
            note: None,
            page: Some(page),
            chapter: None,
            pen,
            color: color.unwrap_or(Rgb(255, 221, 87)),
            added: 0,
        });
    }

    fn note(&mut self, _: PaneId) {}

    fn bookmark(&mut self, _: PaneId) {}

    fn select(&mut self, pane: PaneId, page: usize, text: &str) {
        let Some(open) = self.panes.get_mut(&pane) else {
            return;
        };
        let layer = open.doc.text_layer(page);
        let found = layer.is_ok_and(|l| l.text.text(0..l.text.len()).contains(text));
        open.selection = found.then(|| (page, text.to_owned()));
    }

    fn dict_window(&mut self, _: Window) {}

    fn pursue(&mut self, _: &str) {}

    fn dict(&mut self, _: DictAction) {}

    fn vocab(&mut self, _: Download) {}

    #[cfg(not(target_arch = "wasm32"))]
    fn attach_vocab(&mut self, _: PathBuf) -> Result<(), String> {
        Err("No word levels here.".into())
    }

    fn pronunciation(&mut self, _: Download) -> Result<(), String> {
        Err("No pronunciation pack here.".into())
    }

    fn page_ai(&mut self, _: PageAiAction) {}

    #[cfg(not(target_arch = "wasm32"))]
    fn ai(&mut self, _: mareader_settings::Job) {}

    fn place_read_marker(&mut self, _: PaneId, _: Spot, _: f64) {}

    fn dismiss_read_markers(&mut self, _: f64) {}

    fn read_aloud(&mut self, _: PaneId, _: ReadFrom) {}

    fn toggle_read_aloud(&mut self) -> bool {
        false
    }

    fn stop_read_aloud(&mut self) {}

    fn try_voice(&mut self) {}

    fn try_voice_of(&mut self, _: &str) {}

    fn stop_voice_try(&mut self) {}

    fn say_word(&mut self, _: &str, _: Option<&str>) -> Result<(), String> {
        Ok(())
    }

    fn stop_saying(&mut self) {}

    fn reveal_voice(&mut self, _: &'static Model, _: &'static str) {}

    fn voice_model(
        &mut self,
        _: Item,
        _: Download,
        _: &mut ReadAloudSettings,
    ) -> Result<(), String> {
        Ok(())
    }

    fn open_book(&mut self, _: BookId, _: Option<PaneId>, _: Placement) {}

    fn library(&mut self, _: LibraryAction) {}
}

/// The export's name for `title`, as the notes panel makes it.
pub fn export_name(title: &str, format: Format) -> String {
    file_name(title, format)
}
