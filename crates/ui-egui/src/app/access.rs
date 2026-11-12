use super::*;
use crate::app::navigation::found_in;

impl MareaderApp {
    /// The owner of everything that outlives a frame.
    pub fn session(&self) -> &mareader_engine::session::Session {
        &self.session
    }

    pub fn settings(&self) -> &Settings {
        self.session.settings()
    }

    pub fn tree(&self) -> &mareader_workspace::PaneTree {
        self.session.tree()
    }

    /// The focused pane.
    pub fn active(&self) -> PaneId {
        self.session.active()
    }

    /// Makes `pane` the focused pane.
    pub(super) fn focus(&mut self, pane: PaneId) {
        self.send(Intent::FocusPane(pane));
    }

    /// The tree, writable, for the pane surgery `execute` cannot express:
    /// a lift and drop, a divider drag.
    pub(super) fn tree_mut(&mut self) -> &mut mareader_workspace::PaneTree {
        self.session.tree_mut()
    }

    /// The panes by last focus, latest first.
    pub fn recent(&self) -> &[PaneId] {
        self.session.recent()
    }

    /// Dials set directly, for shots and tests; the app uses `set_field`.
    pub fn settings_mut(&mut self) -> &mut Settings {
        self.session.settings_mut()
    }

    /// Takes a whole new settings, through the door.
    pub fn set_settings(&mut self, settings: Settings) {
        self.send(Intent::SetSettings(Box::new(settings)));
    }

    /// One dial, by its dotted path, to whatever JSON `value` makes.
    ///
    /// The option handlers' shared way in: a path names the setting, so a
    /// new dial needs no new command.
    pub(super) fn set_field<T: serde::Serialize>(
        &mut self,
        path: &str,
        value: T,
    ) -> Result<(), String> {
        let value = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        let path = path.into();
        let done = self.session.apply(Intent::SetField { path, value });
        done.refused.map_or(Ok(()), Err)
    }

    /// An intent the session carries out alone: a dial, the focus.
    pub(super) fn send(&mut self, intent: Intent) {
        self.session.apply(intent);
    }

    pub fn read_aloud(&self) -> &crate::read_aloud::ReadAloud {
        &self.ui.read_aloud
    }

    /// The file being read: the active pane's, else the first with one.
    pub fn last_document(&self) -> Option<LastDocument> {
        let panes = std::iter::once(self.active()).chain(self.tree().panes());
        let (pane, at) =
            (panes.into_iter()).find_map(|pane| Some((pane, self.ui.documents.place(pane)?)))?;
        let place = at.anchor.map(|anchor| (anchor.block, anchor.offset));
        let spine = at.spine.map(|place| mareader_library::SpinePosition {
            href: place.href,
            item: place.item,
            block: place.block,
            offset: place.offset,
        });
        let book = self.ui.documents.book(pane);
        Some(LastDocument {
            path: at.path,
            page: at.page,
            place,
            spine,
            pages: at.pages,
            book,
        })
    }

    pub fn library(&self) -> &mareader_library::Library {
        self.ui.library.model()
    }

    /// The page's own state: the open shelf, a name being edited.
    pub fn library_state(&self) -> &LibraryState {
        &self.ui.library
    }

    /// For tests and tools that shape the library straight on.
    pub fn library_state_mut(&mut self) -> &mut LibraryState {
        &mut self.ui.library
    }

    pub fn library_showing(&self) -> bool {
        self.route() == Route::Library
    }

    /// Covers held as textures, for tests.
    /// Document searches kept, for meters.
    pub fn searches(&self) -> usize {
        self.ui.search.len()
    }

    pub fn covers_shown(&self) -> usize {
        self.ui.library.covers().shown()
    }

    /// The newest toast still up.
    pub fn toast(&self) -> Option<&Toast> {
        self.ui.toasts.current()
    }

    /// Every toast still up, newest first, and whether the stack is open.
    pub fn toasts(&self) -> (Vec<&Toast>, bool) {
        (self.ui.toasts.shown().collect(), self.ui.toasts.is_open())
    }

    /// Applied at the end of the next frame, like a view's commands.
    pub fn queue(&mut self, command: Command) {
        self.ui.queued.push(command);
    }

    /// Store true here, from any thread, when the system wants less motion.
    pub fn reduced_motion(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.ui.reduced_motion)
    }

    pub fn find(&self) -> &FindBar {
        &self.ui.find
    }

    /// What the find bar's query has found in its pane, if asked yet.
    pub fn found(&self) -> Option<&Found> {
        found_in(&self.ui.search, &self.ui.documents, &self.ui.find)
    }

    pub fn settings_window(&self) -> &SettingsWindow {
        &self.ui.settings_window
    }

    pub fn sidebar(&self) -> &Sidebar {
        &self.ui.sidebar
    }

    /// Where the macOS traffic lights go; the desktop binary moves them.
    pub fn native_lights(&self) -> NativeLights {
        self.ui.lights
    }

    /// Where the binary found the lights after moving them; none when hidden.
    pub fn set_light_spots(&mut self, spots: Option<LightSpots>) {
        self.ui.light_spots = spots;
    }

    pub fn titlebar(&self) -> &TitleBar {
        &self.ui.titlebar
    }

    /// Whether the current route's bar is pinned. The settings own it, so this
    /// is the only place to ask; there is no copy to fall out of step.
    pub fn pinned(&self) -> bool {
        self.settings().chrome.pinned(self.route().bar())
    }

    /// Whether the title bar is on screen: pinned, or hovered in.
    pub fn bar_shown(&self) -> bool {
        self.ui.titlebar.visible(self.pinned())
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.tree().panes()
    }

    /// Each pane's rect as last drawn.
    pub fn pane_rects(&self) -> &[(PaneId, egui::Rect)] {
        &self.ui.pane_rects
    }

    /// Whether a theme change is fading in over the old look.
    pub fn fading(&self) -> bool {
        self.ui.fade.is_fading()
    }

    pub fn active_pane(&self) -> PaneId {
        self.session.active()
    }

    pub fn documents(&self) -> &Documents {
        &self.ui.documents
    }

    /// The study tools: the card, its answer, session marks.
    pub fn dict(&self) -> &crate::dict::Dict {
        &self.ui.dict
    }

    pub fn dict_mut(&mut self) -> &mut crate::dict::Dict {
        &mut self.ui.dict
    }

    /// Whether the active pane's scripts have run and taken every click.
    pub fn scripts_settled(&self) -> bool {
        let pane = self.ui.documents.scripts_idle(self.active());
        self.ui.script_shot.is_empty() && pane && self.ui.scripts.idle()
    }

    /// The scripts' state, for a screenshot that waited too long.
    pub fn scripts_state(&self) -> String {
        let pane = self.ui.documents.scripts_state(self.active());
        format!("{}; {pane}", self.ui.scripts.describe())
    }

    /// The pronunciation pack, for tests and shots.
    pub fn sounds(&self) -> &crate::sounds::Sounds {
        &self.ui.sounds
    }

    /// The word levels: the dataset and the shaded words.
    pub fn vocab(&self) -> &crate::vocab::Vocab {
        &self.ui.vocab
    }

    pub fn study(&self) -> &Study {
        &self.ui.study
    }

    pub fn page_ai(&self) -> &crate::page_ai::PageAi {
        &self.ui.page_ai
    }

    /// The pages picked in the thumbnails, in reading order.
    pub fn thumb_picks(&self) -> Vec<usize> {
        self.ui.thumbs.picked()
    }

    /// Tests: the word list window's Markdown, once made.
    pub fn word_window_markdown(&self) -> Option<&str> {
        self.ui.word_window.markdown()
    }

    /// Tests: whether a word list is still being made.
    pub fn making_word_list(&self) -> bool {
        self.ui.word_lists.busy()
    }

    /// Tests: what `pane`'s document keeps of the page AI, oldest first.
    pub fn kept_ai(
        &self,
        pane: PaneId,
    ) -> Vec<(mareader_library::AiTask, mareader_library::PageSpan)> {
        let book = self.ui.library.book_in(&self.ui.documents, pane);
        let name = self.ui.documents.title(pane).unwrap_or_default();
        self.ui.page_ai.kept(self.ui.library.model(), (book, name))
    }

    /// The marks `pane` shows, wherever they are kept.
    pub fn marks(&self, pane: PaneId) -> Vec<crate::study::marks::Shown> {
        let book = self.ui.library.book_in(&self.ui.documents, pane);
        let name = self.ui.documents.title(pane).unwrap_or_default();
        let look = &self.settings().study.highlights;
        self.ui
            .study
            .marks_of(self.ui.library.model(), book, name, look)
            .to_vec()
    }

    /// The bar's title: the active pane's document, else the app.
    pub fn title(&self) -> &str {
        self.ui.documents.title(self.active()).unwrap_or("Mareader")
    }
}
