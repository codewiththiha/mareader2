//! The engine's host, answered by the controllers this crate still holds.

use mareader_engine::documents::OpenRequest;
use std::path::PathBuf;

use egui::Context;
use mareader_appearance::{Appearance, Rgb};
use mareader_engine::dict::{Action as DictAction, Download, Window};
use mareader_engine::export::Format;
use mareader_engine::host::{EdgeTab, Host};
use mareader_engine::page_ai::Action as PageAiAction;
use mareader_engine::pane::{Arrow, ZoomCommand};
use mareader_engine::session::Session;
use mareader_engine::study::Action as StudyAction;
use mareader_library::{BookId, Pen};
use mareader_settings::{ReadAloudSettings, WordList};
use mareader_speech::{Item, Model, Spot};
use mareader_workspace::{Bounds, PaneId};

use super::super::*;

/// The Dictionary's tab parked on the window's edge.
pub(super) const EDGE_TAB: &str = "dict-edge-tab-button";

/// The app and the frame's context, lent to the engine for one command.
pub(super) struct Front<'a> {
    pub app: &'a mut MareaderApp,
    pub ctx: &'a Context,
}

impl Host for Front<'_> {
    fn session(&mut self) -> &mut Session {
        &mut self.app.session
    }

    fn area(&self) -> Bounds {
        MareaderApp::area(self.ctx)
    }

    fn edge_tab(&self) -> EdgeTab {
        EdgeTab {
            shown: self.app.ui.dict.edge_tab_active(),
            focused: self.ctx.memory(|m| m.has_focus(egui::Id::new(EDGE_TAB))),
        }
    }

    fn arrived(&mut self, route: Route) {
        self.app.arrived(route);
    }

    fn close_pane(&mut self, pane: PaneId) {
        self.app.ui.documents.close_pane(pane);
        self.app.ui.page_ai.close_pane(pane);
    }

    fn resume(&mut self, request: OpenRequest) -> Option<OpenRequest> {
        self.app.ui.library.resume(request)
    }

    fn open(&mut self, pane: PaneId, request: OpenRequest) -> Option<Toast> {
        self.app.ui.documents.open(self.ctx, pane, request)
    }

    fn zoom(&mut self, pane: PaneId, zoom: ZoomCommand, now: f64) {
        self.app.ui.documents.zoom(pane, zoom, now);
    }

    fn set_mode(&mut self, pane: PaneId, mode: ViewMode, now: f64) {
        self.app.ui.documents.set_mode(pane, mode, now);
    }

    fn arrow(&mut self, pane: PaneId, arrow: Arrow, now: f64) {
        self.app.ui.documents.arrow(pane, arrow, now);
    }

    fn page_key(&mut self, pane: PaneId, forward: bool) {
        self.app.ui.documents.page_key(pane, forward);
    }

    fn toggle_drift(&mut self, pane: PaneId, now: f64) -> bool {
        self.app.ui.documents.toggle_drift(pane, now)
    }

    fn go_to(&mut self, pane: PaneId, page: usize) {
        self.app.ui.documents.go_to(pane, page);
    }

    fn turn(&mut self, pane: PaneId, forward: bool) {
        self.app.ui.documents.turn(pane, forward);
    }

    fn copy_pages(&mut self, pane: PaneId, pages: Vec<usize>) {
        if let Some(reading) = self.app.ui.documents.reading(pane) {
            self.app.ui.page_copy.start(reading.document, pages);
        }
    }

    fn save(&mut self, name: String, format: Format, contents: String) {
        self.app.ui.files.save(self.ctx, name, format, contents);
    }

    fn book(&self, pane: PaneId) -> Option<BookId> {
        self.app.ui.documents.book(pane)
    }

    fn path(&self, pane: PaneId) -> Option<PathBuf> {
        self.app.ui.documents.path(pane).map(PathBuf::from)
    }

    fn restart_at_start(&mut self, panes: &[PaneId]) -> Vec<Toast> {
        self.app.ui.documents.restart_at_start(self.ctx, panes)
    }

    /// The shelf wears the window's look, the crowned pane's if split.
    fn set_appearance(&mut self, look: Appearance) {
        let app = &mut *self.app;
        let routing = app.routing();
        let mut global = app.settings().appearance;
        if app.route() == Route::Reader {
            app.ui.looks.edit(app.active(), &mut global, routing, look);
        } else {
            app.ui.looks.edit_window(&mut global, routing, look);
        }
        let _ = app.set_field("appearance", global);
    }

    fn open_settings(&mut self, tab: Tab) {
        self.app.open_settings(Some(tab));
    }

    fn word_list(&mut self, pane: PaneId, pages: Vec<usize>, list: WordList) {
        if let Some(ask) = self.app.word_ask(pane, pages, list) {
            let wake = crate::repaint::wake(self.ctx);
            self.app.ui.word_lists.start(wake, ask);
        }
    }

    fn word_window(&mut self, pane: PaneId, pages: Vec<usize>, list: WordList) {
        if let Some(ask) = self.app.word_ask(pane, pages, list) {
            let chapters = !ask.document.outline().is_empty();
            self.app.ui.word_window.open(self.ctx, pane, ask, chapters);
        }
    }

    fn study(&mut self, action: StudyAction) {
        let app = &mut *self.app;
        let mut out = Commands::new();
        let model = app.ui.library.model_mut();
        app.ui.study.act(self.ctx, model, action, &mut out);
        app.ui.queued.extend(out);
    }

    fn explain(&mut self, pane: PaneId) {
        self.app.explain(self.ctx, pane);
    }

    fn look_up(&mut self, pane: PaneId) {
        self.app.look_up(pane);
    }

    fn highlight(&mut self, pane: PaneId, pen: Pen, color: Option<Rgb>) {
        self.app.highlight(pane, pen, color);
    }

    fn note(&mut self, pane: PaneId) {
        self.app.note(pane);
    }

    fn bookmark(&mut self, pane: PaneId) {
        self.app.bookmark(pane);
    }

    fn select(&mut self, pane: PaneId, page: usize, text: &str) {
        self.app.ui.documents.select(pane, page, text);
    }

    fn dict_window(&mut self, window: Window) {
        self.app.dict_window(window);
    }

    fn pursue(&mut self, word: &str) {
        self.app.pursue(word);
    }

    fn dict(&mut self, action: DictAction) {
        self.app.ui.dict.act(action);
    }

    fn vocab(&mut self, action: Download) {
        self.app.ui.vocab.act(action);
    }

    fn pronunciation(&mut self, action: Download) -> Result<(), String> {
        self.app.ui.sounds.act(action)
    }

    fn page_ai(&mut self, action: PageAiAction) {
        self.app.page_ai_act(self.ctx, action);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ai(&mut self, job: mareader_settings::Job) {
        self.app.ui.ai.send(job);
    }

    fn place_read_marker(&mut self, pane: PaneId, spot: Spot, now: f64) {
        self.app.ui.documents.place_read_marker(pane, spot, now);
    }

    fn dismiss_read_markers(&mut self, now: f64) {
        self.app.ui.documents.dismiss_read_markers(now);
    }

    fn read_aloud(&mut self, pane: PaneId, from: ReadFrom) {
        let app = &mut *self.app;
        if let Some(reading) = app.ui.documents.reading(pane) {
            app.ui.clips.stop();
            let settings = app.settings().read_aloud.clone();
            app.ui.read_aloud.start(self.ctx, &reading, from, &settings);
        }
    }

    fn toggle_read_aloud(&mut self) -> bool {
        self.app.ui.read_aloud.toggle()
    }

    fn stop_read_aloud(&mut self) {
        self.app.ui.read_aloud.stop();
    }

    fn try_voice(&mut self) {
        self.app.hush();
        let settings = self.app.settings().read_aloud.clone();
        self.app.ui.read_aloud.booth.try_voice(&settings);
    }

    fn try_voice_of(&mut self, voice: &str) {
        self.app.hush();
        let settings = self.app.settings().read_aloud.clone();
        let (booth, shelf) = (
            &mut self.app.ui.read_aloud.booth,
            &self.app.ui.read_aloud.shelf,
        );
        booth.try_voice_of(&settings, shelf, voice);
    }

    fn stop_voice_try(&mut self) {
        self.app.ui.read_aloud.booth.stop_trying();
    }

    fn say_word(&mut self, word: &str, voice: Option<&str>) -> Result<(), String> {
        self.app.hush();
        let settings = self.app.settings().read_aloud.clone();
        (self.app.ui.read_aloud).say_word(self.ctx, &settings, (word, voice))
    }

    fn stop_saying(&mut self) {
        self.app.ui.read_aloud.stop_word();
    }

    fn reveal_voice(&mut self, model: &'static Model, language: &'static str) {
        self.app.open_settings(Some(Tab::Model(model)));
        let fold = crate::settings_window::voice_fold(model, language);
        crate::settings_window::reveal(self.ctx, fold);
    }

    fn voice_model(
        &mut self,
        item: Item,
        action: Download,
        settings: &mut ReadAloudSettings,
    ) -> Result<(), String> {
        self.app.ui.read_aloud.act((item, action), settings)
    }

    fn open_book(&mut self, book: BookId, pane: Option<PaneId>, placement: Placement) {
        self.app.open_book(self.ctx, book, pane, placement);
    }

    fn library(&mut self, action: mareader_engine::library::Action) {
        self.app.library_action(self.ctx, action);
    }
}
