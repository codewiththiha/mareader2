//! The app state and its eframe loop.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use eframe::Storage;
use egui::{Context, Rect, ViewportCommand};
use mareader_appearance::{
    Appearance, BaseMode, FilmGrain, NoiseMode, PaneLooks, Rgb, Routing, TextureMode,
    builtin_presets,
};
use mareader_session::Anchor;
use mareader_settings::{
    BookDesign, ColumnAlign, DictionaryOpens, FontChoice, LibraryClick, MotionSettings, PaperArea,
    Settings, TextSettings,
};
use mareader_workspace::{
    Bounds, Edge, LiftTarget, MAX_PANES, PaneId, PaneTree, Side, SplitAxis, TreeError,
};

use crate::backdrop::Backdrop;
use crate::bookmarks;
use crate::command::{Command, Commands, Independence, WindowCommand};
use crate::crossfade::Crossfade;
use crate::documents::{Documents, OpenRequest, Placement};
use crate::files::Files;
use crate::find_bar::FindBar;
use crate::floating_label::{self, LabelChrome};
use crate::gpu::{Glass, Gpu, GrainCallback, Quad, crawl_offset};
use crate::layout::ViewMode;
use crate::library::{self, LibraryState};
use crate::library_panel::{self, PaneTab, PanelContext, PanelState};
use crate::machine_fonts::MachineFonts;
use crate::notes_panel;
use crate::outline_panel::{self, OutlineState};
use crate::page_bar::{self, BarLook, PageBar};
use crate::pane_texts::PaneTexts;
use crate::panes::{self, PaneStyle, PaneView};
use crate::pdf_view::{PageLook, ViewContext};
use crate::platform::{NativeLights, Platform};
use crate::route::Route;
use crate::search::{Found, Lit, Search};
use crate::settings_window::{Scope, SettingsWindow, Shown, Tab};
use crate::sidebar::{Panel, RailContext, Sidebar};
use crate::study::{self, Explained, Gloss, Highlighted, Study};
use crate::thumbnails::ThumbState;
use crate::titlebar::{self, BarContext, BarPage, DictionaryBar, LibraryBar, ReaderBar, TitleBar};
use crate::toast::{Toast, Toasts};
use crate::{keymap, motion, resize, theme};

pub const SETTINGS_KEY: &str = "settings";
/// Kept apart from settings: it says what was read, not how to read.
pub const LAST_DOCUMENT_KEY: &str = "last_document";

/// What reopens at launch: the file read at exit, on its page.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LastDocument {
    pub path: PathBuf,
    pub page: usize,
    /// A book's block and offset, which outlast a change of type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<(usize, usize)>,
    /// The library book it was read as, so a twin reopens as itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub book: Option<mareader_library::BookId>,
}

/// Settings reach disk this long after a change, at most.
pub const SAVE_EVERY: Duration = Duration::from_millis(500);

pub struct MareaderApp {
    settings: Settings,
    platform: Platform,
    titlebar: TitleBar,
    /// Distraction-free: bars stay hidden while reading.
    chromeless: bool,
    /// Shots: the newest mark's menu opens once it is drawn.
    menu_shot: bool,
    /// Shots and tests: a page AI result kept, or a run started (true).
    page_ai_seed: Option<(usize, mareader_library::AiTask, bool, usize)>,
    /// Shots: pages the word list window opens on, once shown.
    word_window_seed: Option<Vec<usize>>,
    tree: PaneTree,
    active: PaneId,
    /// Panes by last focus, latest first: blend's paper follows them.
    recent: Vec<PaneId>,
    sidebar: Sidebar,
    settings_window: SettingsWindow,
    find: FindBar,
    search: Search,
    toasts: Toasts,
    /// The selection bar's answers, marks and the word card.
    study: Study,
    page_ai: crate::page_ai::PageAi,
    /// Who answers Explain, and the AI settings' background work.
    ai: crate::ai::Ai,
    /// Language packs, the lookup card and the Dictionary panel.
    dict: crate::dict::Dict,
    vocab: crate::vocab::Vocab,
    /// A selection the options asked for, made once its page is laid out.
    staged: std::collections::VecDeque<Staged>,
    lights: NativeLights,
    documents: Documents,
    files: Files,
    library: LibraryState,
    /// On the Dictionary page: the route it was opened from.
    dict_from: Option<Route>,
    /// The sidebar panels' scroll-following, for the focused pane.
    outline: OutlineState,
    thumbs: ThumbState,
    /// Picked pages' text on its way to the clipboard.
    page_copy: crate::page_copy::PageCopy,
    word_lists: crate::word_list::WordLists,
    word_window: crate::word_list::window::WordWindow,
    shelf_tree: PanelState,
    /// Each pane's page bar.
    page_bars: std::collections::BTreeMap<PaneId, PageBar>,
    /// Last frame's pane rects, to aim a file drop.
    pane_rects: Vec<(PaneId, egui::Rect)>,
    /// A pane held up to be moved, till the press ends.
    lifting: Option<crate::lift::Lift>,
    /// The library page's rect while it is up, for the glass behind bars.
    library_rect: Option<egui::Rect>,
    /// What an empty pane offers, as of a library revision.
    reopen: (u64, Vec<(mareader_library::BookId, String)>),
    /// Split panes' own looks, and the routing they were last fitted to.
    looks: PaneLooks<PaneId>,
    routed: Routing,
    /// Split panes' own text, while text is independent.
    texts: PaneTexts,
    /// Picks new panes' hues and textures.
    random: u64,
    /// A theme change held for, or fading over, a snapshot.
    fade: Crossfade<Held>,
    styled: Option<(Appearance, MotionSettings, bool)>,
    /// The system asks for less motion; set off the UI thread at launch.
    reduced_motion: Arc<AtomicBool>,
    /// Read from storage; [`MareaderApp::resume`] opens it.
    last: Option<LastDocument>,
    fonts: FontState,
    /// Fills in letters no bundled face has from the machine's fonts.
    machine_fonts: MachineFonts,
    /// Commands from outside a view: tests, OS drops, later the keymap.
    queued: Commands,
    #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
    live: theme::live::LiveTokens,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FontState {
    Missing,
    Installed,
    Ready,
}

/// A selection from the options: `text` on `page`, maybe explained.
struct Staged {
    page: usize,
    text: String,
    explain: bool,
    look_up: bool,
    pen: Option<mareader_library::Pen>,
    /// A note's words, and whether it is saved or left as a draft.
    note: Option<(String, bool)>,
    /// Its page was asked for once; the view is on its way there.
    turned: bool,
    /// Bookmarks the page instead of selecting on it.
    bookmark: bool,
    /// Picks every mark and opens Style… instead.
    pick: bool,
}

impl Default for MareaderApp {
    fn default() -> Self {
        Self::with_settings(Settings::default())
    }
}

impl MareaderApp {
    pub fn with_settings(mut settings: Settings) -> Self {
        settings.sanitize();
        Self {
            titlebar: TitleBar::new(settings.chrome.titlebar_pinned),
            settings,
            platform: Platform::current(),
            chromeless: false,
            menu_shot: false,
            page_ai_seed: None,
            word_window_seed: None,
            tree: PaneTree::new(),
            active: PaneId(1),
            recent: Vec::new(),
            sidebar: Sidebar::default(),
            settings_window: SettingsWindow::default(),
            find: FindBar::default(),
            search: Search::default(),
            toasts: Toasts::default(),
            study: Study::default(),
            page_ai: crate::page_ai::PageAi::default(),
            ai: crate::ai::Ai::default(),
            dict: crate::dict::Dict::default(),
            vocab: crate::vocab::Vocab::default(),
            staged: Default::default(),
            lights: NativeLights::default(),
            documents: Documents::default(),
            files: Files::default(),
            library: LibraryState::default(),
            dict_from: None,
            outline: OutlineState::default(),
            thumbs: ThumbState::default(),
            page_copy: Default::default(),
            word_lists: Default::default(),
            word_window: Default::default(),
            shelf_tree: PanelState::default(),
            page_bars: std::collections::BTreeMap::new(),
            pane_rects: Vec::new(),
            lifting: None,
            library_rect: None,
            reopen: (u64::MAX, Vec::new()),
            looks: PaneLooks::default(),
            routed: Routing::default(),
            texts: PaneTexts::default(),
            random: 0x9e37_79b9_7f4a_7c15,
            fade: Crossfade::default(),
            styled: None,
            reduced_motion: Arc::new(AtomicBool::new(false)),
            last: None,
            fonts: FontState::Missing,
            machine_fonts: MachineFonts::default(),
            queued: Commands::new(),
            #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
            live: theme::live::LiveTokens::from_env(),
        }
    }

    pub fn from_creation(cc: &eframe::CreationContext<'_>) -> Self {
        Self::from_storage(cc.storage)
    }

    /// Unreadable settings repair section by section; the notes go to stderr.
    pub fn from_storage(storage: Option<&dyn Storage>) -> Self {
        let mut app = Self::settings_from(storage);
        let last = storage.and_then(|s| s.get_string(LAST_DOCUMENT_KEY));
        app.last = last.and_then(|json| serde_json::from_str(&json).ok());
        app
    }

    fn settings_from(storage: Option<&dyn Storage>) -> Self {
        let Some(json) = storage.and_then(|s| s.get_string(SETTINGS_KEY)) else {
            return Self::default();
        };
        let loaded = mareader_settings::load(&json);
        for note in &loaded.notes {
            eprintln!("mareader: {note}");
        }
        let repaired = !loaded.notes.is_empty();
        let mut app = Self::with_settings(loaded.settings);
        if repaired {
            let note = "Some settings couldn't be read and were reset to defaults.";
            app.toasts.push(Toast::error(note));
        }
        app
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Changes apply next frame, as the settings window's do.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Reopens what was read at exit, if its file is still there.
    pub fn resume(&mut self) -> bool {
        let Some(last) = self.last.take().filter(|l| l.path.is_file()) else {
            return false;
        };
        let place = last.place.map(|(block, offset)| Anchor { block, offset });
        let request = OpenRequest {
            page: last.page,
            place,
            book: last.book,
            ..OpenRequest::path(last.path)
        };
        self.queue(Command::Open(request));
        true
    }

    /// The file being read: the active pane's, else the first with one.
    pub fn last_document(&self) -> Option<LastDocument> {
        let panes = std::iter::once(self.active).chain(self.tree.panes());
        let (pane, (path, page, place)) =
            (panes.into_iter()).find_map(|p| Some((p, self.documents.place(p)?)))?;
        let place = place.map(|a| (a.block, a.offset));
        let book = self.documents.book(pane);
        Some(LastDocument {
            path,
            page,
            place,
            book,
        })
    }

    /// Keeps the library in `root`; without one it lives in memory.
    pub fn attach_library(&mut self, root: PathBuf) {
        self.library.attach(root);
    }

    /// Starts the AI service, its keys kept in `root`; `helper` as in `Ai`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_ai(&mut self, ctx: &Context, root: PathBuf, helper: Option<PathBuf>) {
        self.ai.attach(ctx, root, helper);
        // A run stopped before quitting waits on its card.
        self.library.set_paused(self.ai.paused());
    }

    /// Keeps language packs in `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_dictionaries(&mut self, ctx: &Context, dir: PathBuf) {
        let ctx = ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        if let Err(e) = self.dict.attach(dir, waker) {
            self.toasts
                .push(Toast::info(format!("Dictionaries are off: {e}")));
        }
    }

    /// Keeps the word-level dataset in `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_vocabulary(&mut self, ctx: &Context, dir: PathBuf) {
        let ctx = ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        if let Err(e) = self.vocab.attach(dir, waker) {
            self.toasts
                .push(Toast::info(format!("Word levels are off: {e}")));
        }
    }

    /// Keeps covers and other rebuildable files in `dir`.
    pub fn attach_cache(&mut self, dir: PathBuf) {
        self.library.attach_cache(dir);
    }

    pub fn library(&self) -> &mareader_library::Library {
        self.library.model()
    }

    /// The page's own state: the open shelf, a name being edited.
    pub fn library_state(&self) -> &LibraryState {
        &self.library
    }

    /// For tests and tools that shape the library straight on.
    pub fn library_state_mut(&mut self) -> &mut LibraryState {
        &mut self.library
    }

    pub fn library_showing(&self) -> bool {
        self.route() == Route::Library
    }

    /// The page in the window; the library replaces the panes, not ends them.
    pub fn route(&self) -> Route {
        match (self.dict_from, self.library.showing) {
            (Some(_), _) => Route::Dictionary,
            (None, true) => Route::Library,
            (None, false) => Route::Reader,
        }
    }

    /// Something is open in a pane, so the library can go back to it.
    fn reading(&self) -> bool {
        let panes = self.tree.panes();
        panes.iter().any(|p| self.documents.reading(*p).is_some())
    }

    /// The one way between routes; leaving the reader ends its overlays.
    fn go(&mut self, route: Route) {
        if route == Route::Library {
            self.settings_window.close();
        }
        if route != Route::Reader {
            self.find.close();
        }
        let here = self.route();
        self.dict_from = match route {
            Route::Dictionary if here == Route::Dictionary => self.dict_from,
            Route::Dictionary => Some(here),
            _ => None,
        };
        if route == Route::Dictionary {
            self.dict.arrive();
        } else {
            self.library.showing = route == Route::Library;
        }
    }

    /// The floating Dictionary: its state lives in the settings.
    fn dict_window(&mut self, w: crate::dict::Window) {
        use crate::dict::Window as W;
        let win = &mut self.settings.dictionary.window;
        match w {
            W::Toggle => {
                let show = !win.open || win.minimized;
                (win.open, win.minimized) = (show, false);
            }
            W::Focus => (win.open, win.minimized) = (true, false),
            W::Close => win.open = false,
            W::Minimize(folded) => win.minimized = folded,
            W::Place(r) => win.rect = Some([r.min.x, r.min.y, r.width(), r.height()]),
            W::Expand => {
                win.open = false;
                self.go(Route::Dictionary);
                return;
            }
        }
        // Shown, its field takes the keys.
        if win.open && !win.minimized && !matches!(w, W::Place(_)) {
            self.dict.arrive();
        }
    }

    /// The card's word, searched where the Dictionary opens from here.
    fn pursue(&mut self, word: &str) {
        self.dict.escape();
        let floats = self.settings.dictionary.opens == DictionaryOpens::Window;
        if self.route() == Route::Reader && floats {
            self.dict_window(crate::dict::Window::Focus);
        } else {
            self.go(Route::Dictionary);
        }
        self.dict.search(word);
    }

    /// Where the Dictionary page's pressed button returns.
    fn dict_back(&self) -> Route {
        match self.dict_from {
            Some(Route::Reader) if !self.reading() => Route::Library,
            Some(Route::Reader) => Route::Reader,
            _ => Route::Library,
        }
    }

    /// Covers held as textures, for tests.
    /// Document searches kept, for meters.
    pub fn searches(&self) -> usize {
        self.search.len()
    }

    pub fn covers_shown(&self) -> usize {
        self.library.covers().shown()
    }

    pub fn toast(&self) -> Option<&Toast> {
        self.toasts.current()
    }

    /// Applied at the end of the next frame, like a view's commands.
    pub fn queue(&mut self, command: Command) {
        self.queued.push(command);
    }

    /// Restyles egui when the look changed; true if it did.
    fn restyle(&mut self, ctx: &Context) -> bool {
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        if self.live.poll(ctx) {
            self.styled = None;
        }
        let reduced = self.reduced_motion.load(Ordering::Relaxed);
        let want = (
            self.settings.appearance,
            self.settings.motion.clone(),
            reduced,
        );
        if self.styled.as_ref() == Some(&want) {
            return false;
        }
        #[allow(unused_mut)]
        let mut tokens = theme::Tokens::new(&self.settings.appearance);
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        self.live.overlay(&mut tokens);
        theme::apply(ctx, &tokens);
        motion::set(ctx, &self.settings.motion, reduced);
        self.styled = Some(want);
        true
    }

    /// Store true here, from any thread, when the system wants less motion.
    pub fn reduced_motion(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.reduced_motion)
    }

    pub fn find(&self) -> &FindBar {
        &self.find
    }

    /// What the find bar's query has found in its pane, if asked yet.
    pub fn found(&self) -> Option<&Found> {
        found_in(&self.search, &self.documents, &self.find)
    }

    /// Tabbing into a pane makes it the focused one, as a click there does.
    fn follow_focus(&self, ctx: &Context, out: &mut Commands) {
        let Some(at) = crate::focus::rect(ctx).map(|r| r.center()) else {
            return;
        };
        let modal =
            self.settings_window.is_open() || self.word_window.is_open() || self.library.sheet_up();
        let pane = self.pane_rects.iter().find(|(_, r)| r.contains(at));
        if let Some((id, _)) = pane.filter(|(id, _)| !modal && *id != self.active) {
            out.push(Command::FocusPane(*id));
        }
    }

    /// Asks for the bar's query once it settles, and walks to its hits.
    fn drive_search(&mut self, ctx: &Context) {
        let Some(pane) = self.find.pane() else {
            return;
        };
        let Some(reading) = self.documents.reading(pane) else {
            return;
        };
        let key = Arc::as_ptr(&reading.document).addr();
        if self.find.due(ctx.input(|i| i.time), key) {
            let query = self.find.query();
            self.search.ask(ctx, reading.doc, &reading.document, query);
            self.find.asked(key);
        }
        let found = self.search.found(reading.doc, &reading.document);
        let found = found.filter(|f| f.query == self.find.query());
        self.find.settle(found, reading.current);
        let Some(i) = self.find.seeking() else {
            return;
        };
        ctx.request_repaint();
        let Some(hit) = found.and_then(|f| f.hits.get(i)) else {
            self.find.sought();
            return;
        };
        if self.documents.reveal_hit(pane, hit.page, hit.chars.clone()) {
            self.find.sought();
        }
    }

    pub fn settings_window(&self) -> &SettingsWindow {
        &self.settings_window
    }

    /// Panes whose book sets its own design: their text, so overridden.
    fn book_designs(&self) -> Vec<(PaneId, TextSettings)> {
        let own = self.texts.own(&self.settings.text);
        let lib = self.library.model();
        let pane_text = |pane: PaneId| {
            let mine = own.iter().find(|(p, _)| *p == pane);
            mine.map_or(&self.settings.text, |(_, t)| *t)
        };
        let designed = |pane: PaneId| {
            let book = self.library.book_in(&self.documents, pane)?;
            let design = lib.book(book)?.design.value?;
            let book_design = match design {
                mareader_library::Design::Auto => BookDesign::Auto,
                mareader_library::Design::On => BookDesign::On,
                mareader_library::Design::Off => BookDesign::Off,
            };
            Some((
                pane,
                TextSettings {
                    book_design,
                    ..pane_text(pane).clone()
                },
            ))
        };
        self.tree.panes().into_iter().filter_map(designed).collect()
    }

    /// What the focused pane and the workspace show: which settings apply.
    pub fn settings_scope(&self) -> Scope {
        let docs = &self.documents;
        let reading = docs.reading(self.active);
        let focused = match &reading {
            None => Shown::Nothing,
            Some(r) if r.document.is_reflowable() => Shown::Book {
                stream: r.mode == ViewMode::Vertical,
            },
            Some(_) => Shown::Pdf,
        };
        // A PDF's AI pages are set in the pane's type, like a book.
        let ai = |pane: PaneId| {
            let book = self.library.book_in(docs, pane);
            let name = docs.title(pane).unwrap_or_default();
            let lib = self.library.model();
            self.page_ai.any(lib, pane, docs, (book, name))
        };
        let text = |p: PaneId| docs.reflowable(p) || ai(p);
        let book_design = match focused {
            Shown::Book { .. } => self.library.book_in(docs, self.active).and_then(|id| {
                let book = self.library.model().book(id)?;
                Some((id, book.design.value))
            }),
            _ => None,
        };
        Scope {
            book_design,
            focused,
            split: self.tree.len() > 1,
            any_text: self.tree.panes().into_iter().any(text),
            ai_pages: focused == Shown::Pdf && ai(self.active),
            sideways: reading.is_some_and(|r| r.mode == ViewMode::Horizontal),
        }
    }

    pub fn sidebar(&self) -> &Sidebar {
        &self.sidebar
    }

    /// Where the macOS traffic lights go; the desktop binary moves them.
    pub fn native_lights(&self) -> NativeLights {
        self.lights
    }

    pub fn titlebar(&self) -> &TitleBar {
        &self.titlebar
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.tree.panes()
    }

    /// Each pane's rect as last drawn.
    pub fn pane_rects(&self) -> &[(PaneId, egui::Rect)] {
        &self.pane_rects
    }

    /// Whether a theme change is fading in over the old look.
    pub fn fading(&self) -> bool {
        self.fade.is_fading()
    }

    pub fn active_pane(&self) -> PaneId {
        self.active
    }

    pub fn documents(&self) -> &Documents {
        &self.documents
    }

    /// The study tools: the card, its answer, session marks.
    pub fn dict(&self) -> &crate::dict::Dict {
        &self.dict
    }

    /// The word levels: the dataset and the shaded words.
    pub fn vocab(&self) -> &crate::vocab::Vocab {
        &self.vocab
    }

    pub fn study(&self) -> &Study {
        &self.study
    }

    pub fn page_ai(&self) -> &crate::page_ai::PageAi {
        &self.page_ai
    }

    /// The pages picked in the thumbnails, in reading order.
    pub fn thumb_picks(&self) -> Vec<usize> {
        self.thumbs.picked()
    }

    /// What a word list of `pane`'s `pages` is made from.
    fn word_ask(
        &self,
        pane: PaneId,
        pages: Vec<usize>,
        list: mareader_settings::WordList,
    ) -> Option<crate::word_list::Ask> {
        let reading = self.documents.reading(pane)?;
        Some(crate::word_list::Ask {
            document: reading.document,
            title: self.documents.title(pane).unwrap_or_default().to_owned(),
            pages,
            level: self.settings.study.cefr.level.band(),
            list,
        })
    }

    /// Tests: the word list window's Markdown, once made.
    pub fn word_window_markdown(&self) -> Option<&str> {
        self.word_window.markdown()
    }

    /// Moves a word list on; made, it is copied or saved.
    fn poll_word_list(&mut self, ctx: &egui::Context) {
        if !self.word_lists.busy() {
            return;
        }
        // Answers land by waker, but a list's own steps need frames.
        ctx.request_repaint();
        let toast = match self.word_lists.poll(&mut self.vocab, &mut self.dict) {
            None => return,
            Some(Err(toast)) => toast,
            Some(Ok((made, list))) => {
                let title = self
                    .documents
                    .title(self.active)
                    .unwrap_or_default()
                    .to_owned();
                match crate::word_list::deliver(ctx, &self.files, &title, &made, list.output) {
                    Some(toast) => toast,
                    None => return,
                }
            }
        };
        self.toasts.push(toast);
    }

    /// Tests: whether a word list is still being made.
    pub fn making_word_list(&self) -> bool {
        self.word_lists.busy()
    }

    /// Tests: what `pane`'s document keeps of the page AI, oldest first.
    pub fn kept_ai(
        &self,
        pane: PaneId,
    ) -> Vec<(mareader_library::AiTask, mareader_library::PageSpan)> {
        let book = self.library.book_in(&self.documents, pane);
        let name = self.documents.title(pane).unwrap_or_default();
        self.page_ai.kept(self.library.model(), (book, name))
    }

    /// The marks `pane` shows, wherever they are kept.
    pub fn marks(&self, pane: PaneId) -> Vec<crate::study::marks::Shown> {
        let book = self.library.book_in(&self.documents, pane);
        let name = self.documents.title(pane).unwrap_or_default();
        let look = &self.settings.study.highlights;
        self.study.marks_of(self.library.model(), book, name, look)
    }

    /// The bar's title: the active pane's document, else the app.
    pub fn title(&self) -> &str {
        self.documents.title(self.active).unwrap_or("Mareader")
    }

    /// Headless knobs for `examples/shot.rs` and tests.
    pub fn set_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.reconcile_texts();
        match key {
            "theme" => {
                let base = BaseMode::ALL
                    .into_iter()
                    .find(|b| b.label().eq_ignore_ascii_case(value));
                self.settings.appearance.base = base.ok_or("theme: light, dark or dim")?;
            }
            "preset" => {
                let p = builtin_presets().into_iter().find(|p| p.id == value);
                self.settings.appearance = p
                    .ok_or("preset: sepia, green, night, parchment, cinema")?
                    .appearance;
            }
            "texture" => {
                let mode = TextureMode::ALL.into_iter().find(|m| {
                    let id = format!("{m:?}");
                    id.eq_ignore_ascii_case(value)
                });
                let hint = "texture: none, paper, lined, grid, dotted, cross";
                self.settings.appearance.texture = mode.ok_or(hint)?;
            }
            "texture_scale" => {
                let scale = value.parse().map_err(|_| "texture_scale: 25 to 400")?;
                self.settings.appearance.texture_scale = scale;
                self.settings.appearance.sanitize();
            }
            "platform" => {
                self.platform = Platform::parse(value).ok_or("platform: mac, windows or gnome")?
            }
            "toast" => self.toasts.push(match value.strip_prefix("error:") {
                Some(message) => Toast::error(message),
                None => Toast::info(value),
            }),
            "pinned" => {
                let on = value == "true";
                self.settings.chrome.titlebar_pinned = on;
                self.settings.chrome.library_titlebar_pinned = on;
                self.titlebar.pinned = on;
            }
            #[cfg(not(target_arch = "wasm32"))]
            "dictionaries" => {
                let inline = self.study.inline();
                self.dict.attach(PathBuf::from(value), Arc::new(|| {}))?;
                self.dict.set_inline(inline);
            }
            #[cfg(not(target_arch = "wasm32"))]
            "cefr" => {
                let inline = self.study.inline();
                self.vocab.attach(PathBuf::from(value), Arc::new(|| {}))?;
                self.vocab.set_inline(inline);
                self.settings.study.cefr.enabled = true;
            }
            "workers" => {
                let n = value
                    .parse()
                    .map_err(|_| "workers: a count, 0 runs inline")?;
                self.documents.set_workers(n);
                self.library.set_inline(n == 0);
                self.study.set_inline(n == 0);
                self.page_ai.set_inline(n == 0);
                self.dict.set_inline(n == 0);
                self.search.set_inline(n == 0);
            }
            // The floating Dictionary, searching for any word given.
            "dict_window" => {
                self.dict_window(crate::dict::Window::Toggle);
                if value != "show" {
                    self.dict.search(value);
                }
            }
            // The floating Dictionary folded to its pill.
            "dict_fold" => self.dict_window(crate::dict::Window::Minimize(value == "true")),
            // The Dictionary page, searching for any word given.
            "dict_page" => {
                // After a queued open, which would otherwise leave it.
                match self.queued.iter().any(|c| matches!(c, Command::Open(_))) {
                    true => self.queue(Command::ShowDictionary),
                    false => self.go(Route::Dictionary),
                }
                if value != "show" {
                    self.dict.search(value);
                }
            }
            "library" => match value {
                // After a queued open, which would otherwise leave it.
                "show" if self.queued.iter().any(|c| matches!(c, Command::Open(_))) => {
                    self.queue(Command::ShowLibrary);
                }
                "show" => self.go(Route::Library),
                _ => self.go(Route::Reader),
            },
            // The first books by last read, picked.
            "select" => {
                let n: usize = value.parse().map_err(|_| "select: a count of books")?;
                let recent = self.library.model().recent().into_iter().take(n);
                let picks = recent.map(|b| library::Pick::Book(b.id)).collect();
                let pick = library::Action::Pick(library::PickAction::Only(picks));
                self.queue(Command::Library(pick));
            }
            // Asks to remove the first books by last read, or a shelf by name.
            "remove" => {
                let model = self.library.model();
                let (books, shelves) = match value.strip_prefix("shelf:") {
                    Some(name) => {
                        let found = model.shelves().find(|s| s.name.value == name);
                        let id = found.map(|s| s.id).ok_or("remove: no shelf by that name")?;
                        (Vec::new(), vec![id])
                    }
                    None => {
                        let n: usize =
                            value.parse().map_err(|_| "remove: a count or shelf:Name")?;
                        let recent = model.recent().into_iter().take(n);
                        (recent.map(|b| b.id).collect(), Vec::new())
                    }
                };
                let links = Vec::new();
                let ask = library::BookAction::AskRemove {
                    books,
                    shelves,
                    links,
                };
                self.queue(Command::Library(library::Action::Book(ask)));
            }
            // The import sheet for a folder, as Add ▸ Folder… asks.
            "import" => {
                let ask = library::Action::AskFolder(std::path::PathBuf::from(value));
                self.queue(Command::Library(ask));
            }
            // A folder read in place, as the sheet can add one.
            "folder" => {
                let root = std::path::PathBuf::from(value);
                // Watched, every file counted, so its books show as found.
                let opts = mareader_library::Options {
                    watch: true,
                    min_size: 0,
                    ..mareader_library::Options::default()
                };
                let import = library::Action::ImportFolder { root, opts };
                self.queue(Command::Library(import));
            }
            "layout" => {
                self.settings.library.layout = match value {
                    "list" => mareader_library::Layout::List,
                    _ => mareader_library::Layout::Grid,
                }
            }
            "columns" => {
                self.settings.library.columns = value.parse().ok();
                self.settings.library.sanitize();
            }
            "cover" => {
                self.settings.library.cover = match value {
                    "crop" => mareader_library::CoverFit::Crop,
                    _ => mareader_library::CoverFit::Fit,
                }
            }
            "sort" => {
                let (key, way) = value.split_once(':').unwrap_or((value, ""));
                let key = mareader_library::SortKey::ALL
                    .into_iter()
                    .find(|k| format!("{k:?}").eq_ignore_ascii_case(&key.replace('_', "")))
                    .ok_or("sort: arranged, last_read, title, author or added")?;
                self.settings.library.sort = key;
                self.settings.library.ascending = match way {
                    "asc" => true,
                    "desc" => false,
                    _ => key.natural_ascending(),
                };
            }
            "books" => self
                .library
                .seed(value.parse().map_err(|_| "books: a count")?),
            "shelves" => self.library.seed_shelves(),
            "copies" => self
                .library
                .seed_copies(value.parse().map_err(|_| "copies: a count")?),
            // A made-up AI run over the loose copies, or its plan.
            "organize" => match value {
                "running" => self.library.organize_demo(false),
                "review" => self.library.organize_demo(true),
                "paused" => self.library.organize_paused_demo(false),
                "ready" => self.library.organize_paused_demo(true),
                _ => return Err("organize: running, review, paused or ready".into()),
            },
            // A shelf by its names from the top, as "Fiction/Classics".
            "shelf" => {
                let found = self.library.open_shelf(value);
                found.then_some(()).ok_or("shelf: no shelf by that path")?;
            }
            // Typed into the library's search field.
            "search" => {
                let text = library::SearchAction::Text(value.to_owned());
                self.queue(Command::Library(library::Action::Search(text)));
            }
            "home" => {
                self.settings.library.home = match value {
                    "unshelved" => mareader_library::Home::Unshelved,
                    _ => mareader_library::Home::Everything,
                }
            }
            "pdf" | "epub" => {
                let (kind, pages) = value.split_once(':').unwrap_or((value, ""));
                let count = pages.parse().ok();
                let pages = count.unwrap_or(120);
                let request = match (key, kind) {
                    ("epub", "sample") => OpenRequest::bytes(
                        mareader_epub::sample::epub(count.unwrap_or(3), "A Mareader Book"),
                        "sample.epub",
                    ),
                    (_, "sample") => OpenRequest::bytes(
                        mareader_pdf::sample::pdf(pages, "A Mareader Sample"),
                        "sample.pdf",
                    ),
                    (_, "figures") => OpenRequest::bytes(
                        mareader_pdf::sample::figures(pages, "A Mareader Sample"),
                        "figures.pdf",
                    ),
                    (_, "scan") => OpenRequest::bytes(
                        mareader_pdf::sample::scan(pages, "A Mareader Scan"),
                        "scan.pdf",
                    ),
                    _ => OpenRequest::path(value.into()),
                };
                // Into the pane in front now, so `focus=` aims each file.
                let request = OpenRequest {
                    pane: Some(self.active),
                    ..request
                };
                self.queue(Command::Open(request));
            }
            "blend" => self.settings.paper.blend = value == "true",
            // Who explains words: a script for shots, or nobody.
            "explain" => match value {
                "canned" => {
                    self.study.set_explainer(Arc::new(mareader_study::Canned));
                    self.page_ai.set_canned(true);
                }
                "none" => self
                    .study
                    .set_explainer(Arc::new(mareader_study::Unavailable)),
                _ => return Err("explain: canned or none".into()),
            },
            // One saved model on it explains words, e.g. `openai`.
            "provider" => {
                let name = serde_json::Value::String(value.to_owned());
                let p =
                    serde_json::from_value(name).map_err(|_| "provider: e.g. openai or ollama")?;
                let ai = &mut self.settings.ai;
                ai.presets.clear();
                let id = ai.add_preset(p, "");
                ai.tasks.explain.preset = Some(id);
            }
            // The meaning's language by tag, e.g. `my`.
            "meaning" => {
                mareader_settings::language_name(value).ok_or("meaning: a tag, e.g. my")?;
                self.settings.ai.tasks.explain.language = Some(value.to_owned());
            }
            // The first such text on a page selected, as `page:words`.
            "selection" => {
                let hint = "selection: page:text, as 0:quick";
                let (page, text) = value.split_once(':').ok_or(hint)?;
                let page = page.parse().map_err(|_| hint)?;
                let text = text.replace('_', " ");
                self.staged.push_back(Staged {
                    page,
                    text,
                    explain: false,
                    look_up: false,
                    pen: None,
                    note: None,
                    turned: false,
                    bookmark: false,
                    pick: false,
                });
            }
            // Picks marks, as `pick=on pick=on pick=style`: Style… opens.
            "pick" => {
                self.staged.push_back(Staged {
                    page: 0,
                    text: String::new(),
                    explain: false,
                    look_up: false,
                    pen: None,
                    note: None,
                    turned: true,
                    bookmark: false,
                    pick: value == "style",
                });
            }
            // Bookmarks a page, as `bookmark=2`.
            "bookmark" => {
                let page = value.parse().map_err(|_| "bookmark: a page index")?;
                self.staged.push_back(Staged {
                    page,
                    text: String::new(),
                    explain: false,
                    look_up: false,
                    pen: None,
                    note: None,
                    turned: false,
                    bookmark: true,
                    pick: false,
                });
            }
            // A kept result, `3:simplify`; a canned run, `follow:simplify`.
            "page_ai" => {
                let (page, task) = value.split_once(':').ok_or("page_ai: page:task")?;
                let follow = page == "follow";
                // A range, as 2-6, asks those pages as picked ones.
                let (page, last) = page.split_once('-').unwrap_or((page, page));
                let number = |p: &str| p.parse::<usize>().map_err(|e| e.to_string());
                let (page, last) = match follow {
                    true => (0, 0),
                    false => (number(page)?, number(last)?),
                };
                let task = match task {
                    "summary" => mareader_library::AiTask::Summary,
                    "points" => mareader_library::AiTask::Points,
                    "simplify" => mareader_library::AiTask::Simplify,
                    tag => mareader_library::AiTask::Translate {
                        language: tag.to_owned(),
                    },
                };
                if follow || last > page {
                    self.page_ai.set_canned(true);
                }
                self.page_ai_seed = Some((page, task, follow, last));
            }
            // Page AI runs stay mid-answer: `page_ai_hold=true`.
            "page_ai_hold" => self.page_ai.set_held(value == "true"),
            // Opens the word list window on pages 0-based: `word_window=0-4`.
            "word_window" => {
                let hint = "word_window: first-last, 0-based";
                let (a, b) = value.split_once('-').unwrap_or((value, value));
                let a: usize = a.parse().map_err(|_| hint)?;
                let b: usize = b.parse().map_err(|_| hint)?;
                self.word_window_seed = Some((a..=b.max(a)).collect());
            }
            // Explains the staged selection: its card opens.
            "gloss" => match self.staged.back_mut() {
                Some(staged) => staged.explain = true,
                None => return Err("gloss: after selection=page:text".into()),
            },
            // Looks the staged selection up: the dictionary card opens.
            "lookup" => match self.staged.back_mut() {
                Some(staged) => staged.look_up = true,
                None => return Err("lookup: after selection=page:text".into()),
            },
            // Notes the staged selection: a draft, or saved with `noted`.
            "note" | "noted" => {
                let staged = self.staged.back_mut().ok_or("note: after selection")?;
                staged.note = Some((value.replace('_', " "), key == "noted"));
            }
            // Highlights the staged selection with a pen.
            "highlight" => {
                let hint = "highlight: full, half or underline, after selection";
                let pen = serde_json::from_value(serde_json::Value::String(value.into()))
                    .map_err(|_| hint)?;
                let staged = self.staged.back_mut().ok_or(hint)?;
                staged.pen = Some(pen);
            }
            "grain" => {
                self.settings.appearance.noise = match value {
                    "off" => NoiseMode::Off,
                    "static" => NoiseMode::Static,
                    "animated" => NoiseMode::Animated,
                    _ => return Err("grain: off, static or animated".into()),
                }
            }
            "save_preset" => {
                // `Name` or `Name/Section`, saving the look set so far.
                let (name, group) = value.split_once('/').unwrap_or((value, ""));
                let look = self.settings.appearance;
                self.settings.save_preset(name, group, look);
            }
            "grain_intensity" => {
                let v = value.parse().map_err(|_| "grain_intensity: 0 to 100")?;
                self.settings.appearance.noise_intensity = v;
                self.settings.appearance.sanitize();
            }
            "blend_area" => {
                self.settings.paper.area = match value {
                    "whole" => PaperArea::WholePage,
                    "edges" => PaperArea::Edges,
                    _ => return Err("blend_area: whole or edges".into()),
                }
            }
            "chromeless" => self.chromeless = value == "true",
            "menu" => self.menu_shot = value == "true",
            "animations" => self.settings.motion.enabled = value == "true",
            "float" => self.settings.chrome.sidebar_overlay = value == "true",
            "glass" => self.settings.chrome.glass = value == "true",
            "margin" => {
                self.settings.layout.page_margin = value.parse().map_err(|_| "margin: points")?
            }
            "gap" => self.settings.layout.no_gap = value == "false",
            "text_size" => {
                self.settings.text.font_size = value.parse().map_err(|_| "text_size: px")?;
                self.settings.text.sanitize();
            }
            "indent" => {
                self.settings.text.text_indent = value.parse().map_err(|_| "indent: ems")?;
                self.settings.text.sanitize();
            }
            "ink" => {
                self.settings.text.ink_contrast = value.parse().map_err(|_| "ink: %")?;
                self.settings.text.sanitize();
            }
            "justify" => self.settings.text.justify = value == "true",
            "theme_pictures" => self.settings.text.theme_pictures = value == "true",
            "theme_emojis" => self.settings.text.theme_emojis = value == "true",
            "column_width" => {
                let pct = value.parse().map_err(|_| "column_width: %")?;
                self.settings.text.column_width_pct = pct;
            }
            "hyphenate" => self.settings.text.hyphenation = value == "true",
            "book_layout" => self.settings.text.book_layout = value == "true",
            "design" => {
                self.settings.text.book_design = match value {
                    "auto" => BookDesign::Auto,
                    "on" => BookDesign::On,
                    "off" => BookDesign::Off,
                    _ => return Err("design: auto, on or off".into()),
                }
            }
            "column" => {
                self.settings.text.column_align = match value {
                    "left" => ColumnAlign::Left,
                    "center" => ColumnAlign::Center,
                    "right" => ColumnAlign::Right,
                    _ => return Err("column: left, center or right".into()),
                }
            }
            // `system:georgia`, `bundled:inter`: the body's face.
            "body_font" => self.settings.text.default_font = FontChoice::parse(value),
            "shadow" => self.settings.layout.page_shadow = value == "true",
            "corners" => {
                self.settings.workspace.look.corners = match value {
                    "rounded" => mareader_settings::Corners::Rounded,
                    _ => mareader_settings::Corners::Square,
                }
            }
            "pane_gap" => {
                let gap = value.parse().map_err(|_| "pane_gap: points")?;
                self.settings.workspace.look.gap = gap;
                self.settings.sanitize();
            }
            "label" => {
                let l = &mut self.settings.layout;
                l.floating_label = value != "false";
                l.floating_label_persist = value == "persist";
                if value == "chapter" {
                    l.floating_label_style = mareader_settings::FloatingLabelStyle::Chapter;
                }
            }
            "reduced" => self
                .reduced_motion
                .store(value == "true", Ordering::Relaxed),
            "sidebar" => match Panel::parse(value) {
                Some(p) => self.sidebar.open(Some(p)),
                None if value == "closed" => self.sidebar.close(),
                None => return Err("sidebar: thumbnails, outline, library or closed".into()),
            },
            // The bar's query, landing on its first hit once `open=find`.
            "find" => self.find.seed(value),
            "open" => match value {
                "appearance" => self.queue(Command::ShowAppearance),
                "settings" => self.queue(Command::ShowSettings),
                "shortcuts" => self.queue(Command::ShowShortcuts),
                "menu" => self.queue(Command::ShowViewMenu),
                "find" => self.queue(Command::ShowFind),
                "search" => {
                    let focus = library::SearchAction::Focus;
                    self.queue(Command::Library(library::Action::Search(focus)));
                }
                "add" => self.queue(Command::Library(library::Action::ShowAdd)),
                "add_folder" => {
                    let root = std::path::PathBuf::from("/home/reader/Books");
                    self.queue(Command::Library(library::Action::AskFolder(root)));
                }
                // The removal sheet for what `select` picked.
                "remove" => {
                    let books = self.queued.iter().find_map(|c| match c {
                        Command::Library(library::Action::Pick(library::PickAction::Only(p))) => {
                            Some(p.iter().filter_map(|p| match p {
                                library::Pick::Book(id) => Some(*id),
                                _ => None,
                            }))
                        }
                        _ => None,
                    });
                    let books: Vec<_> = books.ok_or("open=remove: select books first")?.collect();
                    let (shelves, links) = (Vec::new(), Vec::new());
                    let ask = library::BookAction::AskRemove {
                        books,
                        shelves,
                        links,
                    };
                    self.queue(Command::Library(library::Action::Book(ask)));
                }
                "rename" => {
                    let first = self.library.model().recent().first().map(|b| b.id);
                    let first = first.ok_or("open=rename: no book")?;
                    let ask = library::Action::AskRename(library::Named::Book(first));
                    self.queue(Command::Library(ask));
                }
                // The first missing book's find-again sheet.
                "find_again" => {
                    let lost = self.library.model().books().find(|b| b.missing());
                    let lost = lost.ok_or("open=find_again: no missing book")?.id;
                    self.queue(Command::Library(library::Action::FindAgain(lost)));
                }
                _ => {
                    return Err(
                        "open: appearance, settings, menu, find, folders, find_again, remove \
                         or rename"
                            .into(),
                    );
                }
            },
            "settings" => {
                let tab = Tab::parse(value).ok_or("settings: a tab name, e.g. theme")?;
                self.settings_window.open(Some(tab));
            }
            "independent" => self.settings.workspace.independent_themes = value == "true",
            "independent_text" => self.settings.workspace.independent_text = value == "true",
            "independent_textures" => {
                self.settings.workspace.independent_textures = value == "true";
            }
            "shared_base" => self.settings.workspace.shared_base_mode = value == "true",
            "library_click" => {
                self.settings.workspace.library_click = match value {
                    "replace" => LibraryClick::Replace,
                    "split" => LibraryClick::Split,
                    "drag_only" => LibraryClick::DragOnly,
                    _ => return Err("library_click: replace, split or drag_only".into()),
                };
            }
            // Two imports under way, their cards shown at once.
            "runs" => self.library.runs.demo(),
            "focus" => {
                let n: u32 = value.parse().map_err(|_| "focus: a pane number")?;
                let pane = self.tree.panes().get(n.saturating_sub(1) as usize).copied();
                self.active = pane.ok_or("focus: no such pane")?;
            }
            "panes" => {
                let n: usize = value.parse().map_err(|_| "panes: 1 to 4")?;
                let area = Bounds::new(0.0, 0.0, 1600.0, 1000.0);
                while self.tree.len() < n.min(mareader_workspace::MAX_PANES) {
                    let last = self.tree.panes().last().copied().unwrap_or(self.active);
                    let axis = if self.tree.len() % 2 == 1 {
                        SplitAxis::Horizontal
                    } else {
                        SplitAxis::Vertical
                    };
                    self.tree
                        .split(last, axis, Side::After, area)
                        .map_err(|e| e.to_string())?;
                }
            }
            _ => return Err(format!("unknown option {key}")),
        }
        Ok(())
    }

    /// The pages' news for the study tools, then the card over them.
    fn glosses(&mut self, ctx: &Context, out: &mut Commands) {
        let pane = self.active;
        let sheets = self.documents.sheets(pane);
        let shown = |page| sheets.iter().any(|s| s.page == page);
        match self.staged.pop_front() {
            // Its page is turned to first, then selected once laid out.
            Some(mut staged) if !sheets.is_empty() && !shown(staged.page) => {
                if !staged.turned {
                    out.push(Command::GoTo {
                        pane,
                        page: staged.page,
                    });
                }
                staged.turned = true;
                self.staged.push_front(staged);
                ctx.request_repaint();
            }
            Some(Staged {
                page,
                text,
                explain,
                look_up,
                pen,
                note,
                bookmark,
                pick,
                ..
            }) if !sheets.is_empty() => {
                let first = self.marks(pane).first().map(|m| m.id);
                match (bookmark, first) {
                    (true, _) => out.push(Command::Bookmark(Some(pane))),
                    // Picking starts, then a later `pick` takes all in.
                    (false, Some(id)) if text.is_empty() && !self.study.picking() => {
                        let start = crate::study::Action::SelectSeveral(pane, id);
                        out.push(Command::Study(start));
                    }
                    (false, Some(_)) if text.is_empty() => {
                        out.push(Command::Study(crate::study::Action::PickAll));
                        if pick {
                            egui::Popup::open_id(ctx, egui::Id::new(crate::study::PICK_STYLE));
                        }
                    }
                    _ => out.push(Command::SelectText { pane, page, text }),
                }
                if explain {
                    out.push(Command::Explain(pane));
                }
                if look_up {
                    out.push(Command::LookUp(pane));
                }
                if let Some(pen) = pen {
                    let color = None;
                    out.push(Command::Highlight { pane, pen, color });
                }
                if let Some((text, save)) = note {
                    out.push(Command::Note(pane));
                    out.push(Command::Study(crate::study::Action::TypeNote(text, save)));
                }
                // The next waits a frame, as each selection replaces the last.
                if !self.staged.is_empty() {
                    ctx.request_repaint();
                }
            }
            Some(staged) => self.staged.push_front(staged),
            None => {}
        }
        if let Some(pages) = self
            .word_window_seed
            .take_if(|p| p.first().is_some_and(|&p| shown(p)))
        {
            out.push(Command::WordListWindow { pane, pages });
        }
        if let Some((page, task, follow, last)) = self.page_ai_seed.clone()
            && shown(page)
        {
            let book = self.library.book_in(&self.documents, pane);
            let name = self.documents.title(pane).unwrap_or_default().to_owned();
            let asked = crate::page_ai::Asked {
                pane,
                page,
                book,
                name: &name,
                documents: &self.documents,
                ai: &self.ai,
                settings: &self.settings.ai,
            };
            let now = ctx.input(|i| i.time);
            let done = match follow {
                false if last > page => {
                    let pages: Vec<usize> = (page..=last).collect();
                    self.page_ai.run_pages(asked, &pages, task);
                    true
                }
                true if self.page_ai.can_read(&asked) => {
                    let model = self.library.model();
                    self.page_ai.follow(asked, model, task, true, now, out);
                    true
                }
                true => false,
                false => self
                    .page_ai
                    .seed(asked, self.library.model_mut(), task, now),
            };
            if done {
                self.page_ai_seed = None;
            }
            ctx.request_repaint();
        }
        for (pane, gloss) in self.documents.take_gloss() {
            match gloss {
                Gloss::Explain => out.push(Command::Explain(pane)),
                Gloss::LookUp => out.push(Command::LookUp(pane)),
                Gloss::Note => out.push(Command::Note(pane)),
                Gloss::Highlight(pen, color) => out.push(Command::Highlight { pane, pen, color }),
                Gloss::Copied => out.push(Command::Notify(Toast::info("Copied"))),
                Gloss::Open { mark, at } => {
                    self.study
                        .open_mark(ctx, self.library.model(), pane, mark, at);
                    self.ask_role();
                }
                Gloss::Shaded { page, chars, at } => {
                    self.explain_shaded(ctx, pane, page, chars, at)
                }
                Gloss::Rest { word, at } => self.rest(pane, word, at),
                Gloss::Menu { mark, at, words } => {
                    self.menu_shot = false;
                    let model = self.library.model();
                    self.study.open_menu(model, pane, mark, at, words);
                }
                Gloss::Anchor(at) => self.study.anchor(pane, at),
                Gloss::Pick(mark) => self.study.pick(pane, mark),
                Gloss::Toggle(mark) => self.study.toggle(pane, mark),
                Gloss::Seen { page, all } => self.study.seen(pane, page, all),
                Gloss::Lift(origin) if self.tree.len() > 1 => {
                    let home = self.pane_rects.iter().find(|(id, _)| *id == pane);
                    if let Some(&(_, home)) = home {
                        let since = ctx.input(|i| i.time);
                        let lift = crate::lift::Lift {
                            pane,
                            origin,
                            home,
                            since,
                        };
                        self.lifting = Some(lift);
                        self.active = pane;
                        ctx.request_repaint();
                    }
                }
                Gloss::Lift(_) => {}
                Gloss::PageMenu { page, at } => self.page_ai.open_menu(pane, page, at),
                Gloss::Bubble { page, index, at } => {
                    let now = ctx.input(|i| i.time);
                    self.page_ai.pick(pane, page, index, at, now);
                }
                Gloss::BubbleMenu { page, index, at } => {
                    self.page_ai.open_bubble_menu(pane, page, index, at);
                }
                Gloss::Pill(crate::page_ai::PillPress::Stop) => self.page_ai.stop_pill(pane),
                Gloss::Pill(crate::page_ai::PillPress::Continue) => {
                    let Some(reading) = self.documents.reading(pane) else {
                        continue;
                    };
                    let book = self.library.book_in(&self.documents, pane);
                    let name = self.documents.title(pane).unwrap_or_default().to_owned();
                    let asked = crate::page_ai::Asked {
                        pane,
                        page: reading.current,
                        book,
                        name: &name,
                        documents: &self.documents,
                        ai: &self.ai,
                        settings: &self.settings.ai,
                    };
                    let now = ctx.input(|i| i.time);
                    let model = self.library.model();
                    self.page_ai.resume(asked, model, now, out);
                }
            }
        }
        for (pane, _) in &self.pane_rects {
            if self.documents.sheets(*pane).is_empty() {
                self.study.hold(*pane);
            }
        }
        if let Some(explainer) = self.ai.follow(&self.settings.ai) {
            self.study.set_explainer(explainer);
        }
        self.take_organized();
        self.study.receive(self.library.model_mut());
        let (changed, role) = self.vocab.poll();
        if changed {
            // Answers came: the pages plan their shading again.
            ctx.request_repaint();
        }
        let word = self.study.card_words().map(|(w, _)| w.to_owned());
        if let (Some(role), Some(word)) = (role, word) {
            self.study.set_role(&word, role);
        }
        let looks = (&self.settings.study.gloss, &self.settings.study.highlights);
        let model = self.library.model();
        self.study.show(ctx, model, &self.pane_rects, looks, out);
        let gpu = self.documents.gpu();
        self.page_ai
            .receive(ctx, self.library.model_mut(), gpu, out);
        let around = crate::page_ai::Around {
            main: self.library.model(),
            documents: &self.documents,
            ai: &self.ai,
            settings: &self.settings.ai,
        };
        self.page_ai.tick(ctx, &around, out);
        // A book lays its pane's shown results into its pages.
        for pane in self.tree.panes() {
            let book = self.library.book_in(&self.documents, pane);
            let name = self.documents.title(pane).unwrap_or_default().to_owned();
            let model = self.library.model();
            let who = (book, name.as_str());
            if let Some(inserts) = self.page_ai.inserts(model, pane, &self.documents, who) {
                self.documents.set_inserts(pane, inserts);
            }
        }
        let model = self.library.model();
        self.page_ai
            .show(ctx, &self.settings.ai, model, &self.pane_rects, out);
        if let Some(toast) = self.page_copy.poll(ctx) {
            self.toasts.push(toast);
        }
        self.dict.poll();
        self.poll_word_list(ctx);
        let services = (&mut self.vocab, &mut self.dict, &self.files);
        self.word_window.show(ctx, &self.settings, services, out);
        let documents = &self.documents;
        let offset = |pane| documents.offset(pane);
        self.dict.show(ctx, &self.pane_rects, offset, out);
    }

    /// Asks the tagger for the open card's word's role.
    fn ask_role(&mut self) {
        if let Some((word, context)) = self.study.card_words() {
            self.vocab.ask_role(word, context);
        }
    }

    /// A click on a shaded word: explained like a selected one.
    fn explain_shaded(
        &mut self,
        ctx: &Context,
        pane: PaneId,
        page: usize,
        chars: std::ops::Range<usize>,
        rect: Rect,
    ) {
        let Some(asked) = self.documents.asked_at(pane, page, chars) else {
            return;
        };
        self.documents.clear_selection(pane);
        self.dict.escape();
        let book = self.library.book_in(&self.documents, pane);
        let name = self.documents.title(pane).unwrap_or_default().to_owned();
        let at = Explained {
            pane,
            book,
            name: &name,
            asked,
            rect,
        };
        let model = self.library.model_mut();
        self.study.explain(ctx, model, at, library::wall_ms());
        self.ask_role();
    }

    /// Resting on a shaded word looks it up, when a pack can answer.
    fn rest(&mut self, pane: PaneId, word: String, at: Rect) {
        if !self.settings.dictionary.hover || self.dict.ready().is_empty() || self.study.card_open()
        {
            return;
        }
        let offset = self.documents.offset(pane);
        let to = self.settings.dictionary.default_lang.clone();
        self.dict.rest(pane, word, at, offset, to);
    }

    /// Look up: the pane's selection, its card beside it.
    fn look_up(&mut self, pane: PaneId) {
        let Some((text, rect)) = self.documents.looked_up(pane) else {
            return;
        };
        let max = mareader_study::MAX_GLOSS_CHARS;
        let Some(word) = crate::dict::lookup_text(&text, max) else {
            let rule = format!("Look up takes a word or short phrase, up to {max} characters.");
            self.toasts.push(Toast::info(rule));
            return;
        };
        // An open window answers instead; it covers nothing, so the
        // selection stays.
        if self.settings.dictionary.window.open && self.route() == Route::Reader {
            self.settings.dictionary.window.minimized = false;
            self.dict.escape();
            self.dict.feed(&word);
            return;
        }
        self.documents.clear_selection(pane);
        // One card at a time.
        self.study.escape(self.library.model_mut());
        let offset = self.documents.offset(pane);
        let to = self.settings.dictionary.default_lang.clone();
        self.dict.look_up(pane, word, rect, offset, to);
    }

    /// What the page AI's menus asked for.
    fn page_ai_act(&mut self, ctx: &Context, action: crate::page_ai::Action) {
        use crate::page_ai::{Action, task_for};
        use mareader_settings::{PageStart, PageTask};
        let now = ctx.input(|i| i.time);
        let mut out = Commands::new();
        let task = match &action {
            Action::Run { task, .. } => task_for(*task, &self.settings.ai),
            Action::Translate { tag, .. } => {
                self.settings.ai.tasks.translate.pick(tag);
                Some(mareader_library::AiTask::Translate {
                    language: tag.clone(),
                })
            }
            _ => None,
        };
        match action {
            Action::Run {
                pane, page, how, ..
            }
            | Action::Translate {
                pane, page, how, ..
            } => {
                let Some(task) = task else { return };
                let kind = crate::page_ai::page_task(&task);
                if how.remember {
                    let start = match how.continuous {
                        true => PageStart::Continuous,
                        false => PageStart::Page,
                    };
                    let tasks = &mut self.settings.ai.tasks;
                    match kind {
                        PageTask::Translate => tasks.translate.start = start,
                        PageTask::Simplify => tasks.simplify.start = start,
                        PageTask::Summarize => {}
                    }
                }
                let book = self.library.book_in(&self.documents, pane);
                let name = self.documents.title(pane).unwrap_or_default().to_owned();
                self.documents.clear_selection(pane);
                let asked = crate::page_ai::Asked {
                    pane,
                    page,
                    book,
                    name: &name,
                    documents: &self.documents,
                    ai: &self.ai,
                    settings: &self.settings.ai,
                };
                match how.continuous && kind != PageTask::Summarize {
                    true => {
                        let model = self.library.model();
                        self.page_ai.follow(asked, model, task, true, now, &mut out);
                    }
                    false => self.page_ai.run(asked, task, now, &mut out),
                }
            }
            Action::StopFollow(pane) => self.page_ai.stop_follow(pane),
            Action::RunPages {
                pane,
                pages,
                task,
                tag,
            } => {
                let task = match (task, tag) {
                    (PageTask::Translate, Some(tag)) => {
                        self.settings.ai.tasks.translate.pick(&tag);
                        Some(mareader_library::AiTask::Translate { language: tag })
                    }
                    (task, _) => task_for(task, &self.settings.ai),
                };
                let Some(task) = task else { return };
                let book = self.library.book_in(&self.documents, pane);
                let name = self.documents.title(pane).unwrap_or_default().to_owned();
                let asked = crate::page_ai::Asked {
                    pane,
                    page: pages.first().copied().unwrap_or_default(),
                    book,
                    name: &name,
                    documents: &self.documents,
                    ai: &self.ai,
                    settings: &self.settings.ai,
                };
                self.page_ai.run_pages(asked, &pages, task);
            }
            Action::PagesMenu { pane, pages, at } => self.page_ai.open_pages_menu(pane, pages, at),
            Action::StopPages(pane) => self.page_ai.stop_groups(pane),
            Action::Redo { pane, page, slot } => {
                let book = self.library.book_in(&self.documents, pane);
                let name = self.documents.title(pane).unwrap_or_default().to_owned();
                let asked = crate::page_ai::Asked {
                    pane,
                    page,
                    book,
                    name: &name,
                    documents: &self.documents,
                    ai: &self.ai,
                    settings: &self.settings.ai,
                };
                let model = self.library.model();
                self.page_ai.again(asked, model, slot, now, &mut out);
            }
            Action::Show { pane, page, slot } => self.page_ai.show_result(pane, page, slot, now),
            Action::Copy(slot) => {
                if let Some(text) = self.page_ai.text_of(self.library.model(), slot) {
                    ctx.copy_text(text);
                    out.push(Command::Notify(Toast::info("Copied")));
                }
            }
            Action::Delete(slot) => self.page_ai.delete(self.library.model_mut(), slot),
        }
        self.queued.extend(out);
    }

    /// Explain: the pane's selected word, marked, its card open.
    fn explain(&mut self, ctx: &Context, pane: PaneId) {
        if self.documents.selection(pane).is_none() {
            return;
        }
        let Some((asked, rect)) = self.documents.asked(pane) else {
            let rule = format!(
                "Explain takes one word, up to {} characters.",
                mareader_study::MAX_GLOSS_CHARS
            );
            self.toasts.push(Toast::info(rule));
            return;
        };
        self.documents.clear_selection(pane);
        self.dict.escape();
        let book = self.library.book_in(&self.documents, pane);
        let name = self.documents.title(pane).unwrap_or_default().to_owned();
        let at = Explained {
            pane,
            book,
            name: &name,
            asked,
            rect,
        };
        let model = self.library.model_mut();
        self.study.explain(ctx, model, at, library::wall_ms());
        self.ask_role();
    }

    /// A highlighter: the pane's selection, marked in `pen`.
    fn highlight(&mut self, pane: PaneId, pen: mareader_library::Pen, color: Option<Rgb>) {
        let Some((spot, text)) = self.documents.highlighted(pane) else {
            return;
        };
        self.documents.clear_selection(pane);
        let book = self.library.book_in(&self.documents, pane);
        let name = self.documents.title(pane).unwrap_or_default().to_owned();
        let at = Highlighted {
            book,
            name: &name,
            text,
            spot,
            pen,
            color,
        };
        let model = self.library.model_mut();
        self.study.highlight(model, at, library::wall_ms());
    }

    /// Note: highlights the selection in the bar's first pen, then
    /// opens a draft on it.
    fn note(&mut self, pane: PaneId) {
        let Some((spot, text)) = self.documents.highlighted(pane) else {
            return;
        };
        let rect = self.documents.looked_up(pane).map(|(_, r)| r);
        self.documents.clear_selection(pane);
        let look = &self.settings.study.highlights;
        let pen = look.bar.iter().find_map(|i| match i {
            mareader_settings::BarItem::Highlight(pen) => Some(*pen),
            _ => None,
        });
        let book = self.library.book_in(&self.documents, pane);
        let name = self.documents.title(pane).unwrap_or_default().to_owned();
        let at = Highlighted {
            book,
            name: &name,
            text,
            spot,
            pen: pen.unwrap_or_default(),
            color: None,
        };
        let model = self.library.model_mut();
        let rect = rect.unwrap_or(Rect::NOTHING);
        self.study
            .write_note(model, pane, at, rect, library::wall_ms());
    }

    /// Bookmarks the pane's page, or takes back the bookmark on it.
    fn bookmark(&mut self, pane: PaneId) {
        let Some(reading) = self.documents.reading(pane) else {
            return;
        };
        let page = reading.current;
        let doc = &reading.document;
        let kept = bookmarks::on_pages(&self.study, &self.library, &self.documents, pane);
        // A result's later pages share its first page's bookmark.
        let here = crate::notes_panel::page_of(doc, bookmarks::spot(doc, page)).unwrap_or(page);
        let existing = kept.iter().find(|(_, p)| *p == here).map(|(id, _)| *id);
        let name = self.documents.title(pane).unwrap_or_default().to_owned();
        let at = study::Bookmarking {
            book: self.library.book_in(&self.documents, pane),
            name: &name,
            spot: bookmarks::spot(doc, page),
            label: bookmarks::label(doc, page),
            existing,
        };
        let mut out = Commands::new();
        let model = self.library.model_mut();
        self.study
            .toggle_bookmark(model, at, library::wall_ms(), &mut out);
        self.queued.extend(out);
    }

    fn apply(&mut self, ctx: &Context, command: Command) {
        if !self.route().allows(&command) {
            return;
        }
        // Text edits must reach the pane focused when they were made.
        self.reconcile_texts();
        match command {
            Command::ToggleSidebar => self.sidebar.toggle(),
            Command::OpenSidebar(panel) => self.sidebar.open(panel),
            Command::CloseSidebar => self.sidebar.close(),
            Command::SetSidebarOverlay(on) => self.settings.chrome.sidebar_overlay = on,
            Command::Split(axis) => {
                if let Some(id) = self.split(ctx, self.active, axis) {
                    self.active = id;
                }
            }
            Command::FocusPane(id) => self.active = id,
            Command::CyclePane { forward } => {
                let panes = self.tree.panes();
                let at = panes.iter().position(|p| *p == self.active).unwrap_or(0);
                let n = panes.len().max(1);
                let next = (if forward { at + 1 } else { at + n - 1 }) % n;
                if let Some(pane) = panes.get(next) {
                    self.active = *pane;
                }
                // Else focus in the last pane would pull it straight back.
                ctx.memory_mut(|m| m.focused().map(|id| m.surrender_focus(id)));
            }
            Command::ClosePane(id) => {
                if self.tree.close(id).is_ok() {
                    self.documents.close_pane(id);
                    self.page_ai.close_pane(id);
                    if self.active == id {
                        self.active = self.tree.panes().first().copied().unwrap_or(PaneId(1));
                    }
                }
            }
            Command::ShowOpenDialog(placement) => self.files.pick(ctx, placement),
            Command::Zoom(zoom) => {
                let now = ctx.input(|i| i.time);
                self.documents.zoom(self.active, zoom, now);
            }
            Command::View(mode) => {
                let now = ctx.input(|i| i.time);
                let fit = self.settings.layout.auto_scale;
                self.documents.set_mode(self.active, mode, fit, now);
            }
            Command::Arrow(arrow) => {
                let now = ctx.input(|i| i.time);
                self.documents.arrow(self.active, arrow, now);
            }
            Command::Page { forward } => self.documents.page_key(self.active, forward),
            Command::Chromeless => {
                self.chromeless = !self.chromeless;
                if self.chromeless {
                    self.sidebar.close();
                    let keys = keymap::hint(self.platform, &Command::Chromeless);
                    let keys = keys.map_or(String::new(), |k| format!(" or {k}"));
                    let how = format!("Distraction-free. Press Esc{keys} to leave.");
                    self.toasts.push(Toast::info(how));
                }
            }
            Command::AutoScroll => {
                let now = ctx.input(|i| i.time);
                if !self.documents.toggle_drift(self.active, now) {
                    let why = "Auto-scroll runs in the scrolling views";
                    self.toasts.push(Toast::info(why));
                }
            }
            Command::FirstPage => self.documents.go_to(self.active, 0),
            Command::LastPage => self.documents.go_to(self.active, usize::MAX),
            Command::GoTo { pane, page } => self.documents.go_to(pane, page),
            Command::CopyPages { pane, pages } => {
                if let Some(reading) = self.documents.reading(pane) {
                    self.page_copy.start(ctx, reading.document, pages);
                }
            }
            Command::WordList { pane, pages } => {
                let list = self.settings.study.words.instant().clone();
                if let Some(ask) = self.word_ask(pane, pages, list) {
                    self.word_lists.start(ctx, ask);
                }
            }
            Command::WordListWindow { pane, pages } => {
                let list = self.settings.study.words.last.clone();
                if let Some(ask) = self.word_ask(pane, pages, list) {
                    let chapters = !ask.document.outline().is_empty();
                    self.word_window.open(ctx, pane, ask, chapters);
                }
            }
            Command::Turn { pane, forward } => self.documents.turn(pane, forward),
            Command::Open(request) => {
                // Held while the library loads, to open where it was left.
                let Some(request) = self.library.resume(request) else {
                    return;
                };
                self.go(Route::Reader);
                let mut pane = request.pane.unwrap_or(self.active);
                let edge = match request.placement {
                    Placement::Into => None,
                    Placement::Beside => Some(self.beside(ctx, pane)),
                    Placement::Edge(edge) => Some(edge),
                };
                if let Some(edge) = edge {
                    match self.split_at(ctx, pane, edge) {
                        Some(new) => pane = new,
                        None => return,
                    }
                }
                if self.tree.panes().contains(&pane) {
                    self.active = pane;
                    if let Some(note) = self.documents.open(ctx, pane, request) {
                        self.toasts.push(note);
                    }
                }
            }
            Command::SetRatio(path, ratio) => {
                self.tree.set_ratio(&path, ratio);
            }
            // The shelf wears the window's look; no pane is in view.
            Command::SetAppearance(a) if self.route() != Route::Reader => {
                self.settings.appearance = a;
            }
            Command::SetAppearance(a) => {
                let routing = self.routing();
                let global = &mut self.settings.appearance;
                self.looks.edit(self.active, global, routing, a);
            }
            Command::SetInkContrast(ink) => {
                self.settings.text.ink_contrast = ink;
                self.settings.text.sanitize();
            }
            Command::SetIndependence(i) => {
                self.settings.workspace.independent_themes = i.themes;
                self.settings.workspace.independent_textures = i.textures;
            }
            Command::SavePreset { name, group, look } => {
                self.settings.save_preset(&name, &group, look);
            }
            Command::DeletePreset(id) => self.settings.delete_preset(&id),
            Command::SetTitlebarPinned(p) => {
                self.titlebar.pinned = p;
                *self.pin_setting() = p;
            }
            Command::Window(w) => ctx.send_viewport_cmd(match w {
                WindowCommand::Minimize => ViewportCommand::Minimized(true),
                WindowCommand::ToggleMaximize => {
                    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ViewportCommand::Maximized(!maximized)
                }
                WindowCommand::Close => ViewportCommand::Close,
                WindowCommand::StartDrag => ViewportCommand::StartDrag,
                WindowCommand::ToggleFullscreen => {
                    let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    ViewportCommand::Fullscreen(!full)
                }
            }),
            Command::Notify(toast) => self.toasts.push(toast),
            Command::Export {
                name,
                format,
                contents,
            } => self.files.save(ctx, name, format, contents),
            Command::DismissToast => self.toasts.dismiss(ctx.input(|i| i.time)),
            Command::Study(action) => {
                let mut out = Commands::new();
                let model = self.library.model_mut();
                self.study.act(ctx, model, action, &mut out);
                self.queued.extend(out);
            }
            Command::PageAi(action) => self.page_ai_act(ctx, action),
            Command::Explain(pane) => self.explain(ctx, pane),
            Command::LookUp(pane) => self.look_up(pane),
            Command::Dict(crate::dict::Action::Window(w)) => self.dict_window(w),
            Command::Dict(crate::dict::Action::Pursue(word)) => self.pursue(&word),
            Command::Dict(action) => self.dict.act(action),
            Command::Vocab(action) => self.vocab.act(action),
            Command::SetDictionary(d) => {
                self.settings.dictionary = *d;
                self.settings.dictionary.sanitize();
            }
            Command::Highlight { pane, pen, color } => self.highlight(pane, pen, color),
            Command::Note(pane) => self.note(pane),
            Command::Bookmark(pane) => self.bookmark(pane.unwrap_or(self.active)),
            Command::SelectText { pane, page, text } => {
                self.documents.select(pane, page, &text);
            }
            Command::ShowAppearance => {
                self.titlebar.reveal();
                egui::Popup::open_id(ctx, egui::Id::new(crate::titlebar::APPEARANCE_POPOVER));
            }
            Command::ShowViewMenu => {
                self.titlebar.reveal();
                let menu = match self.route() {
                    Route::Reader => Some(crate::view_menu::VIEW_MENU),
                    Route::Library => Some(crate::library::menu::LIBRARY_VIEW_MENU),
                    Route::Dictionary => None,
                };
                if let Some(menu) = menu {
                    egui::Popup::open_id(ctx, egui::Id::new(menu));
                }
            }
            Command::ShowSettings => self.settings_window.open(None),
            Command::ShowShortcuts => crate::shortcuts::toggle(ctx),
            // With nothing to search in, find searches the library.
            Command::FindStep { forward } if self.find.is_open() => {
                let found = found_in(&self.search, &self.documents, &self.find);
                self.find.key_step(found, forward, ctx.input(|i| i.time));
            }
            Command::FindStep { .. } => self.apply(ctx, Command::ShowFind),
            Command::ShowFind => match (self.route(), self.documents.reading(self.active)) {
                // The page's own field is its find.
                (Route::Dictionary, _) => self.dict.arrive(),
                (Route::Reader, Some(_)) => {
                    self.find.open(self.active);
                    self.titlebar.reveal();
                }
                _ => {
                    self.go(Route::Library);
                    let focus = library::SearchAction::Focus;
                    self.apply(ctx, Command::Library(library::Action::Search(focus)));
                }
            },
            Command::OpenSettings(tab) => self.settings_window.open(Some(tab)),
            Command::CloseSettings => self.settings_window.close(),
            #[cfg(not(target_arch = "wasm32"))]
            Command::Ai(job) => self.ai.send(job),
            Command::SetSettings(s) => {
                let mut s = *s;
                s.sanitize();
                self.titlebar.pinned = s.chrome.titlebar_pinned;
                self.settings = s;
            }
            Command::ShowSearch if self.find.is_open() => self.find.close(),
            Command::ShowSearch => self.apply(ctx, Command::ShowFind),
            Command::ShowLibrary => match self.route() {
                Route::Reader | Route::Dictionary => self.go(Route::Library),
                // An empty reader is no place to go back to.
                Route::Library if self.reading() => self.go(Route::Reader),
                Route::Library => {}
            },
            Command::ShowDictionary => match self.route() {
                Route::Dictionary => self.go(self.dict_back()),
                // Over the page, unless Settings ask for the page.
                Route::Reader if self.settings.dictionary.opens == DictionaryOpens::Window => {
                    self.dict_window(crate::dict::Window::Toggle);
                }
                _ => self.go(Route::Dictionary),
            },
            Command::SetLibraryView(view) => {
                self.settings.library = view;
                self.settings.library.sanitize();
            }
            // A lost book asks where it went instead of failing.
            Command::OpenBook(book) => {
                // Do nothing is for the rail; the page has nothing to drag to.
                let placement = match self.settings.workspace.library_click {
                    LibraryClick::Split => Placement::Beside,
                    LibraryClick::Replace | LibraryClick::DragOnly => Placement::Into,
                };
                self.open_book(ctx, book, None, placement);
            }
            Command::PlaceBook {
                book,
                pane,
                placement,
            } => self.open_book(ctx, book, pane, placement),
            Command::Library(action) => self.library_action(ctx, action),
        }
    }
}

impl MareaderApp {
    /// The last three books opened, rebuilt only as the library changes.
    fn recent_books(&mut self) -> Vec<(mareader_library::BookId, String)> {
        let model = self.library.model();
        if self.reopen.0 != model.revision() {
            let books = model.recent().into_iter();
            let opened = books.filter(|b| b.opened.value > 0 && !b.missing());
            let three = opened.take(3).map(|b| (b.id, b.title.value.clone()));
            self.reopen = (model.revision(), three.collect());
        }
        self.reopen.1.clone()
    }

    /// Asks the AI to shelve loose books; a paused run asks first.
    fn organize(&mut self, picked: Option<Vec<mareader_library::BookId>>, anew: bool) {
        self.library.close_replace();
        if !anew && self.library.organizer().paused().is_some() {
            return self.library.ask_replace(picked);
        }
        match self.library.ask_organize(picked.as_deref()) {
            Ok(ask) => self.start_organizing(&ask, false),
            Err(why) => self.toasts.push(Toast::info(why)),
        }
    }

    /// Carries on the paused run; one with nothing left is let go.
    fn resume_organizing(&mut self) {
        self.library.close_replace();
        match self.library.ask_resume() {
            Ok(ask) => self.start_organizing(&ask, true),
            Err(why) => {
                if !self.library.organizer().busy() {
                    self.ai.forget();
                    self.library.set_paused(None);
                }
                self.toasts.push(Toast::info(why));
            }
        }
    }

    /// Runs `ask`; a finished paused run needs no model to show its plan.
    fn start_organizing(&mut self, ask: &library::organizing::Ask, resume: bool) {
        let ai = &self.settings.ai;
        let ready = resume && self.library.organizer().paused().is_some_and(|p| p.ready);
        let model = ai
            .organize_preset()
            .map(mareader_settings::ModelPreset::title);
        let Some(model) = model.or_else(|| ready.then(|| "plan ready".to_owned())) else {
            let why = "Choose a model to organize with in Settings › AI.";
            return self.toasts.push(Toast::warning(why));
        };
        #[cfg(not(target_arch = "wasm32"))]
        if self.ai.organize(ask, ai.tasks.organize.batch, resume) {
            self.library
                .organizing(ask.books.iter().map(|b| b.id), &model);
            // A busy model says so at once.
            self.take_organized();
            return;
        }
        let _ = (ask, model, resume);
        self.toasts
            .push(Toast::warning("The AI isn't available here."));
    }

    /// The run's news, into the library; an ended run's steps are read back.
    fn take_organized(&mut self) {
        let heard = self.ai.organized();
        use library::organizing::Heard;
        let over = |h: &Heard| !matches!(h, Heard::Progress { .. });
        let empty = |h: &Heard| matches!(h, Heard::Plan(p) if p.placed.is_empty());
        if heard.iter().any(empty) {
            // A plan that moves nothing has nothing to review later.
            self.ai.forget();
            self.library.set_paused(None);
        } else if heard.iter().any(over) {
            self.library.set_paused(self.ai.paused());
        }
        let mut out = Commands::new();
        for heard in heard {
            self.library.heard(heard, &mut out);
        }
        for command in out {
            self.queue(command);
        }
    }

    /// Library work; its dialogs need the app's files.
    fn library_action(&mut self, ctx: &Context, action: library::Action) {
        use library::Action as A;
        use library::SearchAction as S;
        match action {
            A::Search(S::Focus) => {
                self.titlebar.reveal();
                library::search::focus(ctx);
            }
            A::Search(S::Choose(_) | S::ChooseLit) => {
                library::search::blur(ctx);
                let mut out = Commands::new();
                self.library.act(action, ctx.input(|i| i.time), &mut out);
                for command in out {
                    self.apply(ctx, command);
                }
            }
            A::ShowAdd => egui::Popup::open_id(ctx, egui::Id::new(library::ADD_MENU)),
            A::PickFolder => {
                let ask = |root| Command::Library(A::AskFolder(root));
                self.files.pick_folder(ctx, "Import a folder", ask);
            }
            A::PickSearchFolder(book) => {
                let seek = move |folder| Command::Library(A::Seek { book, folder });
                self.files.pick_folder(ctx, "Search a folder", seek);
            }
            A::Book(library::BookAction::PickImport) => {
                let to = self.library.at();
                self.pick_import(ctx, None, to);
            }
            A::PickFrom(dir) => {
                let to = self.library.at();
                self.pick_import(ctx, Some(dir), to);
            }
            A::PickInto(to) => self.pick_import(ctx, None, to),
            A::Book(library::BookAction::Reveal(book)) => {
                let path = self.library.model().book(book).and_then(|b| b.path());
                let shown = path.is_some_and(crate::reveal::reveal);
                if !shown {
                    let note = "The file manager couldn't be opened.";
                    self.toasts.push(Toast::error(note));
                }
            }
            A::Shelf(library::ShelfAction::Reveal(shelf)) => {
                let dir = self.library.model().shelf_dir(shelf);
                let shown = dir.as_deref().is_some_and(crate::reveal::reveal);
                if !shown {
                    let note = "The file manager couldn't be opened.";
                    self.toasts.push(Toast::error(note));
                }
            }
            A::PickFile(book) => {
                let Some(format) = self.library.model().book(book).map(|b| b.format) else {
                    return;
                };
                let relink = move |path| Command::Library(A::Relink { book, path });
                self.files.pick_book(ctx, format.extensions(), relink);
            }
            A::Organize(picked) => self.organize(picked, false),
            A::OrganizeAnew(picked) => self.organize(picked, true),
            A::ResumeOrganizing => self.resume_organizing(),
            A::StopOrganizing => {
                self.ai.stop_organizing();
                // What was done is kept; its card offers to carry on.
                self.library.set_paused(self.ai.paused());
                self.library.organizer_end();
            }
            A::DiscardPaused => {
                let ready = self.library.organizer().paused().is_some_and(|p| p.ready);
                if self.ai.set_aside() {
                    let said = match ready {
                        true => "Discarded the shelf plan.",
                        false => "Discarded the paused run.",
                    };
                    let undo = Command::Library(A::RestorePaused);
                    self.toasts.push(Toast::info(said).undo(undo));
                }
                self.library.set_paused(None);
            }
            A::RestorePaused => {
                self.ai.bring_back();
                self.library.set_paused(self.ai.paused());
            }
            A::ApplyOrganized | A::DiscardOrganized => {
                // Applied, the run is done; a discarded plan can come back.
                let undoable = match action {
                    A::DiscardOrganized => self.ai.set_aside(),
                    _ => {
                        self.ai.forget();
                        false
                    }
                };
                self.library.set_paused(None);
                let mut out = Commands::new();
                self.library.act(action, ctx.input(|i| i.time), &mut out);
                if undoable {
                    let undo = Command::Library(A::RestorePaused);
                    let said = "Discarded the shelf plan.";
                    out.push(Command::Notify(Toast::info(said).undo(undo)));
                }
                for command in out {
                    self.apply(ctx, command);
                }
            }
            action => {
                let mut out = Commands::new();
                self.library.act(action, ctx.input(|i| i.time), &mut out);
                for command in out {
                    self.apply(ctx, command);
                }
            }
        }
    }

    /// The file dialog for books, landing on `to` (none: the top).
    fn pick_import(
        &mut self,
        ctx: &Context,
        from: Option<PathBuf>,
        to: Option<mareader_library::ShelfId>,
    ) {
        let import = move |paths| {
            let action = library::BookAction::Import { paths, to };
            Command::Library(library::Action::Book(action))
        };
        self.files.pick_books(ctx, from, import);
    }

    /// Splits `pane`; a refusal is explained in a toast.
    fn split(&mut self, ctx: &Context, pane: PaneId, axis: SplitAxis) -> Option<PaneId> {
        let edge = match axis {
            SplitAxis::Horizontal => Edge::Right,
            SplitAxis::Vertical => Edge::Bottom,
        };
        self.split_at(ctx, pane, edge)
    }

    /// Every pane as the rail's Library panel lists it.
    fn tabs(&self) -> Vec<PaneTab> {
        // The library's name tells twins apart; the document's title doesn't.
        let name = |pane, book: Option<mareader_library::BookId>| {
            let named = book.and_then(|id| self.library.model().book(id));
            let title = named.map(|b| b.title.value.as_str());
            let title = title.or_else(|| self.documents.title(pane));
            title.unwrap_or("Empty pane").to_owned()
        };
        let panes = self.tree.panes().into_iter();
        let tab = |pane| {
            let book = self.library.book_in(&self.documents, pane);
            PaneTab {
                pane,
                name: name(pane, book),
                book,
            }
        };
        panes.map(tab).collect()
    }

    /// A book carried from the rail lands where it's let go.
    fn drops_from_rail(&self, ui: &egui::Ui, out: &mut Commands) {
        if self.route() != Route::Reader {
            return;
        }
        let floating = self.settings.chrome.sidebar_overlay;
        let rail = (self.sidebar.offset()).filter(|_| floating);
        let edge = rail.map(|o| o + crate::sidebar::WIDTH);
        // Docked, the rail ends where the panes begin.
        let left = self
            .pane_rects
            .iter()
            .map(|(_, r)| r.left())
            .reduce(f32::min);
        let docked = left.filter(|_| self.sidebar.is_open() && !floating);
        let landing = library_panel::Landing {
            panes: &self.pane_rects,
            rail: edge.or(docked),
            full: self.tree.len() >= MAX_PANES,
        };
        let empty = |pane| self.documents.title(pane).is_none();
        let name = |pane| self.documents.title(pane).unwrap_or("this pane").to_owned();
        library_panel::drop_overlay(ui, &landing, empty, name, out);
    }

    /// Opens a library book: shown already, a click focuses its pane.
    fn open_book(
        &mut self,
        ctx: &Context,
        id: mareader_library::BookId,
        pane: Option<PaneId>,
        placement: Placement,
    ) {
        // A lost book asks where it went instead of failing.
        if self.library.model().book(id).is_some_and(|b| b.missing()) {
            self.apply(ctx, Command::Library(library::Action::FindAgain(id)));
            return;
        }
        let dropped = pane.is_some();
        let panes = self.tree.panes();
        let showing = panes
            .into_iter()
            .find(|&p| self.library.book_in(&self.documents, p) == Some(id));
        if let (false, Some(showing)) = (dropped, showing) {
            self.active = showing;
            self.go(Route::Reader);
            return;
        }
        // A library book beside an empty pane goes in it, as in mareader.
        let empty = self.documents.title(pane.unwrap_or(self.active)).is_none();
        let placement = match placement {
            Placement::Beside if empty => Placement::Into,
            other => other,
        };
        match self.library.open(id) {
            Ok(request) => {
                let request = OpenRequest {
                    pane,
                    placement,
                    ..request
                };
                self.apply(ctx, Command::Open(request));
            }
            Err(toast) => self.toasts.push(toast),
        }
    }

    /// Right of `pane`, else below it when the right has no room.
    fn beside(&self, ctx: &Context, pane: PaneId) -> Edge {
        let mut probe = self.tree.clone();
        let area = Self::area(ctx);
        let (axis, side) = Edge::Right.placement();
        match probe.split(pane, axis, side, area) {
            Err(TreeError::NoRoom(_)) => Edge::Bottom,
            _ => Edge::Right,
        }
    }

    fn area(ctx: &Context) -> Bounds {
        let r = ctx.content_rect();
        Bounds::new(r.left(), r.top(), r.width(), r.height())
    }

    /// The lifted pane follows the pointer; letting go moves it there.
    fn lifted(&mut self, ctx: &Context) {
        let Some(lift) = self.lifting else {
            return;
        };
        let pane = lift.pane;
        if !self.tree.panes().contains(&pane) || self.tree.len() < 2 {
            self.lifting = None;
            return;
        }
        let title = self.documents.title(pane).unwrap_or("this pane").to_owned();
        let target = match crate::lift::show(ctx, &lift, &self.pane_rects, &title) {
            crate::lift::Lifted::Held => return,
            crate::lift::Lifted::Cancelled => None,
            crate::lift::Lifted::Dropped(target) => Some(target),
        };
        self.lifting = None;
        let moved = match target {
            None => return,
            Some(LiftTarget::Swap(other)) => self.tree.swap(pane, other),
            Some(LiftTarget::Dock(other, edge)) => {
                self.tree.move_to(pane, other, edge, Self::area(ctx))
            }
        };
        if moved.is_err() {
            let note = "There's no room for the pane there.";
            self.toasts.push(Toast::info(note));
        }
        ctx.request_repaint();
    }

    /// A new pane on `edge` of `pane`; a refusal is a note.
    fn split_at(&mut self, ctx: &Context, pane: PaneId, edge: Edge) -> Option<PaneId> {
        let area = Self::area(ctx);
        let (axis, side) = edge.placement();
        match self.tree.split(pane, axis, side, area) {
            Ok(id) => Some(id),
            Err(TreeError::Full) => {
                let note = format!("A workspace holds {MAX_PANES} panes at most.");
                self.toasts.push(Toast::info(note));
                None
            }
            Err(_) => {
                let note = "This pane is too small to split.";
                self.toasts.push(Toast::info(note));
                None
            }
        }
    }

    /// Drops open under the pointer, returning that pane; the library imports.
    fn drops(&self, ctx: &Context, out: &mut Commands) -> Option<PaneId> {
        let desktop = cfg!(not(target_arch = "wasm32"));
        if desktop && self.route() == Route::Library {
            let dropped = ctx.input(|i| i.raw.dropped_files.clone());
            let paths = dropped.iter().map(|f| f.path().to_path_buf());
            let paths: Vec<PathBuf> = paths.collect();
            // Folders ask how they're kept; files are copied in.
            if !paths.is_empty() {
                let to = self.library.file_target.or(self.library.at());
                let import = library::BookAction::Import { paths, to };
                out.push(Command::Library(library::Action::Book(import)));
            }
            return None;
        }
        let pointer = ctx.pointer_latest_pos();
        let under = pointer
            .and_then(|p| self.pane_rects.iter().find(|(_, r)| r.contains(p)))
            .map_or(self.active, |(id, _)| *id);
        let (hovering, dropped) = ctx.input(|i| {
            let first = i.raw.dropped_files.first().cloned();
            (!i.raw.hovered_files.is_empty(), first)
        });
        if let Some(file) = dropped {
            self.files.dropped(ctx, &file, under);
        }
        hovering.then_some(under)
    }

    /// Every document pane's counter, progress line and bottom bar.
    fn page_bars(&mut self, ctx: &Context, backdrop: &Backdrop, out: &mut Commands) {
        let rects = &self.pane_rects;
        self.page_bars
            .retain(|id, _| rects.iter().any(|(p, _)| p == id));
        let label = LabelChrome {
            bar: self
                .titlebar
                .visible()
                .then(|| self.titlebar.band_bottom(ctx)),
            sidebar_open: self.sidebar.offset().is_some(),
        };
        // The title bar's glass takes slot 0.
        for (i, (id, rect)) in rects.iter().enumerate() {
            if let Some(reading) = self.documents.reading(*id) {
                // Distraction-free keeps the label, which has its own setting.
                if self.chromeless {
                    floating_label::show(ctx, *rect, &reading, &self.settings.layout, label);
                    continue;
                }
                let look = BarLook {
                    layout: &self.settings.layout,
                    backdrop,
                    slot: i + 1,
                };
                let kept = bookmarks::on_pages(&self.study, &self.library, &self.documents, *id);
                let marked = kept.iter().any(|(_, p)| *p == reading.current);
                let bar = self.page_bars.entry(*id).or_default();
                page_bar::show(ctx, *rect, &reading, (&look, marked), bar, out);
                floating_label::show(ctx, *rect, &reading, &self.settings.layout, label);
            }
        }
    }

    /// What the bars show behind them: pane fills, then this frame's pages.
    fn backdrop(&mut self, ctx: &Context, view: &PaneView) -> Backdrop {
        let drawn = self.documents.take_drawn();
        if !self.settings.chrome.glass {
            return Backdrop::Solid;
        }
        let Some(gpu) = self.documents.gpu() else {
            return Backdrop::Tint;
        };
        let t = theme::Tokens::get(ctx);
        let fill = |id| view.style(id).map_or(t.paper, |s| s.fill);
        let fills = self
            .pane_rects
            .iter()
            .map(|(id, r)| Quad::fill(*r, fill(*id)));
        let mut quads: Vec<Quad> = fills.collect();
        // The library page's paper stands in for the panes.
        quads.extend(self.library_rect.map(|r| Quad::fill(r, t.paper)));
        quads.extend(drawn);
        Backdrop::Glass(Glass {
            pass: gpu.pass.clone(),
            canvas: t.surface,
            quads,
        })
    }

    /// Blend's raw paper: each pane's own, or else shared by every pane.
    pub fn blend_paper(&self, pane: PaneId) -> Option<Rgb> {
        let paper = &self.settings.paper;
        if !paper.blend {
            return None;
        }
        if self.routing().colour() {
            return self.documents.paper(pane, paper.area);
        }
        // The latest focused pane with a paper: a book's has none, as mareader.
        self.recent
            .iter()
            .find_map(|&p| self.documents.paper(p, paper.area))
    }

    /// The latest focused pane showing a PDF: shared blend reads its paper.
    fn paper_source(&self) -> Option<PaneId> {
        let d = &self.documents;
        self.recent
            .iter()
            .copied()
            .find(|&p| d.page_count(p).is_some() && !d.reflowable(p))
    }

    /// Puts the focused pane first in `recent`, dropping closed panes.
    fn note_focus(&mut self) {
        let placed = self.tree.panes();
        self.recent
            .retain(|p| placed.contains(p) && *p != self.active);
        self.recent.insert(0, self.active);
        let unseen: Vec<PaneId> = placed
            .into_iter()
            .filter(|p| !self.recent.contains(p))
            .collect();
        self.recent.extend(unseen);
    }

    /// The library keeps its own pin; the reader's is the other.
    fn pin_setting(&mut self) -> &mut bool {
        let route = self.route();
        let chrome = &mut self.settings.chrome;
        match route {
            Route::Library | Route::Dictionary => &mut chrome.library_titlebar_pinned,
            Route::Reader => &mut chrome.titlebar_pinned,
        }
    }

    /// Which families split panes own right now.
    fn routing(&self) -> Routing {
        routing(&self.settings, self.tree.len() > 1)
    }

    fn held(&self) -> Held {
        Held {
            settings: self.settings.clone(),
            looks: self.looks.clone(),
            routed: self.routed,
        }
    }

    fn restore(&mut self, held: Held) {
        self.titlebar.pinned = held.settings.chrome.titlebar_pinned;
        self.settings = held.settings;
        self.looks = held.looks;
        self.routed = held.routed;
    }

    /// Whether the window or a pane jumped to another look since `before`.
    fn leapt(&self, before: &Held) -> bool {
        let (old, new) = (&before.settings, &self.settings);
        let was = routing(old, self.tree.len() > 1);
        let jumped = |id: PaneId| {
            let from = before.looks.shown(id, &old.appearance, was);
            from.leaps_to(&self.look(id))
        };
        old.paper.blend != new.paper.blend
            || old.appearance.leaps_to(&new.appearance)
            || self.tree.panes().into_iter().any(jumped)
    }

    /// A fade needs motion, the GPU's snapshot, and no slider held.
    fn may_fade(&self, ctx: &Context) -> bool {
        let held = ctx.input(|i| i.pointer.any_down());
        motion::enabled(ctx) && self.documents.gpu().is_some() && !held
    }

    /// `pane`'s look as drawn: the window's, or its own while split.
    pub fn look(&self, pane: PaneId) -> Appearance {
        let global = &self.settings.appearance;
        self.looks.shown(pane, global, self.routing())
    }

    /// Fits the panes' own text to the panes and the focused one.
    fn reconcile_texts(&mut self) {
        let on = self.settings.workspace.independent_text;
        let placed = self.tree.panes();
        let window = &mut self.settings.text;
        self.texts.reconcile(on, (&placed, self.active), window);
    }

    /// `pane`'s text settings: the window's, or its own while independent.
    pub fn text(&self, pane: PaneId) -> &mareader_settings::TextSettings {
        self.texts.of(pane, &self.settings.text)
    }

    /// Fits the panes' looks to this frame's routing and panes.
    fn reconcile_looks(&mut self, ctx: &Context) {
        let now = self.routing();
        let placed = self.tree.panes();
        let mut state = self.random ^ ctx.input(|i| i.time).to_bits();
        let mut random = || {
            state = splitmix(state);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let global = &mut self.settings.appearance;
        let changes = (self.routed, now);
        self.looks
            .reconcile(changes, (&placed, self.active), global, &mut random);
        self.random = state;
        self.routed = now;
    }

    /// Each pane's theme this frame, and its blend paper.
    fn pane_styles(&self, window: &theme::Tokens) -> Vec<(PaneId, PaneStyle)> {
        let own = self.routing().any();
        let style = |id: PaneId| {
            let tokens = if own {
                theme::Tokens::new(&self.look(id))
            } else {
                *window
            };
            let paper = self.blend_paper(id);
            // A book's pane takes its page's paper, as mareader's text panes.
            let plain = match self.documents.reflowable(id) {
                true => theme::color(Rgb::from_unit(tokens.book.paper)),
                false => tokens.paper,
            };
            let fill = paper.map_or(plain, |raw| baked(&tokens, raw));
            let style = PaneStyle {
                tokens,
                paper,
                fill,
                own,
            };
            (id, style)
        };
        self.tree.panes().into_iter().map(style).collect()
    }

    /// The grain over every layer, menus and windows included, as mareader's.
    fn film_grain(&self, ctx: &Context) {
        let (Some(grain), Some(gpu)) = (
            FilmGrain::of(&self.settings.appearance),
            self.documents.gpu(),
        ) else {
            return;
        };
        let offset = crawl_offset(ctx, grain);
        let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("film-grain"));
        let shape = GrainCallback::window(gpu.grain.clone(), ctx.content_rect(), grain, offset);
        ctx.layer_painter(layer).add(shape);
    }

    /// The reader's half of the bar: rail toggle, tools and the menu's state.
    fn reader_bar(&self, overlay: bool) -> ReaderBar {
        let routing = self.routing();
        ReaderBar {
            sidebar_toggle: !overlay && !self.sidebar.is_open(),
            rail_docked: !overlay && self.sidebar.is_open(),
            independence: routing.split.then_some(Independence {
                themes: routing.themes,
                textures: routing.textures,
            }),
            zoom: self.documents.zoom_state(self.active),
            mode: self.documents.mode(self.active),
            auto_scroll: self.documents.drifting(self.active),
            ink: (self.documents.reflowable(self.active))
                .then_some(self.settings.text.ink_contrast),
            dictionary: {
                let w = &self.settings.dictionary.window;
                w.open && !w.minimized
            },
        }
    }

    /// The rail hosts the lights while drawn; then the bar, unless overlay.
    fn place_lights(&self, ctx: &Context, overlay: bool, route: Route) -> NativeLights {
        let rail = self.sidebar.offset().filter(|_| route.has_rail());
        let bar_hosts = rail.is_none() && (!overlay || !route.has_rail());
        // Handing over from a rail, they fade in rather than pop.
        let secs = motion::secs(ctx, titlebar::FADE);
        let handoff = ctx.animate_bool_with_time(egui::Id::new("lights-bar"), bar_hosts, secs);
        match rail {
            Some(shift) => NativeLights {
                shift,
                opacity: 1.0,
            },
            None if bar_hosts => NativeLights {
                shift: 0.0,
                opacity: self.titlebar.opacity().min(handoff),
            },
            None => NativeLights::default(),
        }
    }
}

/// `raw` paper themed as the pages are, so the backdrop matches them.
fn baked(t: &theme::Tokens, raw: Rgb) -> egui::Color32 {
    theme::color(Rgb::from_unit(t.page.apply(raw.to_unit())))
}

/// Three dots where AppKit would put its buttons, for checking off a Mac.
fn stand_in_lights(ctx: &Context, lights: NativeLights) {
    if lights.opacity <= 0.0 {
        return;
    }
    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("stand-in-lights"));
    let painter = ctx.layer_painter(layer);
    let top = ctx.content_rect().left_top();
    let colours = [(255, 95, 87), (254, 188, 46), (40, 200, 64)];
    for (i, (r, g, b)) in colours.into_iter().enumerate() {
        let x = 27.0 + lights.shift + 20.0 * i as f32;
        let fill = egui::Color32::from_rgb(r, g, b).gamma_multiply(lights.opacity);
        painter.circle_filled(top + egui::vec2(x, theme::TITLE_BAR_H / 2.0), 6.0, fill);
    }
}

impl eframe::App for MareaderApp {
    /// The remembered paper from the very first frame, never a dark flash.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let paper = theme::Tokens::new(&self.settings.appearance).paper;
        paper.to_normalized_gamma_f32()
    }

    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if self.fonts == FontState::Missing {
            theme::install_fonts(ctx);
            egui_extras::install_image_loaders(ctx);
            // Zoom keys zoom the document, not the interface.
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
            self.fonts = FontState::Installed;
        }
        self.restyle(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.note_focus();
        if self.documents.gpu().is_none()
            && let Some(render) = frame.wgpu_render_state()
        {
            self.documents.set_gpu(Gpu::new(render));
        }
        // Fonts from set_fonts arrive next frame; named families panic before.
        if self.fonts != FontState::Ready {
            self.fonts = FontState::Ready;
            // Still paper underneath, the frame the window first shows.
            let paper = theme::Tokens::get(&ctx).paper;
            ui.painter().rect_filled(ctx.content_rect(), 0.0, paper);
            ctx.request_repaint();
            return;
        }
        if let Some(after) = self.fade.poll(&ctx) {
            self.restore(after);
            self.restyle(&ctx);
            ctx.request_repaint_after(SAVE_EVERY + Duration::from_millis(50));
        }
        self.reconcile_looks(&ctx);
        self.reconcile_texts();
        self.titlebar.pinned = *self.pin_setting();
        resize::edges(&ctx, self.platform.frameless());
        // Tests, shots and the system change looks without a fade.
        let outside = !self.queued.is_empty();
        let mut commands = std::mem::take(&mut self.queued);
        let designed = self.book_designs();
        let mut own = self.texts.own(&self.settings.text);
        own.retain(|(pane, _)| designed.iter().all(|(p, _)| p != pane));
        own.extend(designed.iter().map(|(pane, text)| (*pane, text)));
        self.documents.set_text(&self.settings.text, &own);
        self.documents.receive(&ctx, &mut commands);
        self.files.collect(&mut commands);
        self.library.receive(&ctx, &mut commands);
        self.library.tick(&ctx);
        // A closed book's search would keep it, and its thread, alive.
        let documents = &self.documents;
        self.search.forget_closed(|doc| documents.document(doc));
        if self.search.poll() {
            ctx.request_repaint();
        }
        // Inline, the measure lands next frame; book it.
        if self.library.observe(&self.documents, &self.tree.panes()) {
            ctx.request_repaint();
        }
        if let Some(wait) = self.library.save(ctx.input(|i| i.time)) {
            ctx.request_repaint_after(Duration::from_secs_f64(wait));
        }
        let route = self.route();
        // The library keeps its bar: it has no other way out.
        self.titlebar.chromeless = self.chromeless && route == Route::Reader;
        // Tab brings hidden chrome up, unless a dialog would sit below it.
        let dialog = self.settings_window.is_open()
            || self.word_window.is_open()
            || self.library.sheet_up()
            || crate::shortcuts::is_open(&ctx);
        if crate::focus::begin(&ctx) && !dialog {
            self.titlebar.reveal();
            self.page_bars.entry(self.active).or_default().reveal();
        }
        let read = commands.len();
        keymap::read(&ctx, self.platform, route, &mut commands);
        // The shortcut gives an idle open window the keys before closing it.
        let win = &self.settings.dictionary.window;
        let floats = self.settings.dictionary.opens == DictionaryOpens::Window;
        if route == Route::Reader && floats && win.open && !win.minimized && !self.dict.typing() {
            let focus = Command::Dict(crate::dict::Action::Window(crate::dict::Window::Focus));
            for c in commands.iter_mut().skip(read) {
                if matches!(c, Command::ShowDictionary) {
                    *c = focus.clone();
                }
            }
        }
        let drop_target = self.drops(&ctx, &mut commands);
        // An open popup takes Escape first.
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        // Naming a shelf takes Escape to cancel.
        let modal = self.settings_window.is_open()
            || self.word_window.is_open()
            || self.library.sheet_up()
            || self.library.renaming().is_some()
            || (route == Route::Library && library::search::focused(&ctx))
            || crate::image_viewer::is_open(&ctx)
            || crate::shortcuts::is_open(&ctx)
            || self.lifting.is_some();
        let mut free = escape && !modal && !egui::Popup::is_any_open(&ctx);
        // The library page lets go of a selection, else goes back to reading.
        if free && route == Route::Library {
            if !self.library.escape() {
                commands.push(Command::ShowLibrary);
            }
            free = false;
        }
        // The dictionary clears its search, else goes back.
        if free && route == Route::Dictionary {
            if !self.dict.page_escape() {
                commands.push(Command::ShowDictionary);
            }
            free = false;
        }
        // Typing in the floating window: clear, then close.
        let window = self.settings.dictionary.window.open && route == Route::Reader;
        if free && window && self.dict.typing() {
            if !self.dict.page_escape() {
                commands.push(Command::Dict(crate::dict::Action::Window(
                    crate::dict::Window::Close,
                )));
            }
            free = false;
        }
        if free && (self.dict.escape() || self.study.escape(self.library.model_mut())) {
            free = false;
        }
        if free && self.documents.stop_drift(self.active) {
            free = false;
        }
        if free && self.find.is_open() {
            self.find.close();
            free = false;
        }
        // A selection, then picked pages, before the sidebar and chromeless.
        let selection = free && self.documents.clear_selection(self.active);
        let grid = self.sidebar.is_open() && self.sidebar.panel() == Panel::Thumbnails;
        if free && !selection && !(grid && self.thumbs.clear_picks()) {
            if self.sidebar.is_open() {
                commands.push(Command::CloseSidebar);
            } else if self.chromeless {
                commands.push(Command::Chromeless);
            }
        }
        let overlay = self.settings.chrome.sidebar_overlay;
        let rail = RailContext {
            overlay,
            slide: self.settings.motion.sidebar_slide,
            platform: self.platform,
        };
        let tabs = self.tabs();
        if let Some(made) = self.library.take_made() {
            self.shelf_tree.made(self.library.model(), made);
        }
        let organizer = self.library.organizer();
        let paused = organizer.paused().map(library::organizing::Paused::short);
        let shelves = PanelContext {
            model: self.library.model(),
            tabs: &tabs,
            active: self.active,
            click: self.settings.workspace.library_click,
            home: self.settings.library.home,
            held: organizer.held(),
            organizer: library::Organizer {
                busy: organizer.busy(),
                paused: paused.as_deref(),
            },
        };
        // Gathered only while the panel shows.
        let notes = match self.sidebar.is_open() && self.sidebar.panel() == Panel::Notes {
            true => {
                let book = self.library.book_in(&self.documents, self.active);
                let name = self.documents.title(self.active).unwrap_or_default();
                let look = &self.settings.study.highlights;
                self.study.notes_of(self.library.model(), book, name, look)
            }
            false => Vec::new(),
        };
        let shelf_tree = &mut self.shelf_tree;
        self.thumbs
            .set_offer(crate::thumbnails::Offer::of(&self.settings));
        let (documents, active) = (&mut self.documents, self.active);
        let (outline, thumbs) = (&mut self.outline, &mut self.thumbs);
        // The rail's panels serve the focused pane's document.
        let mut body =
            |ui: &mut egui::Ui, panel: Panel, recentre: bool, out: &mut Commands| match panel {
                Panel::Outline => documents.reading(active).is_some_and(|r| {
                    outline_panel::show(ui, &r, outline, recentre, out);
                    true
                }),
                Panel::Thumbnails => documents.thumbnails(ui, active, thumbs, recentre, out),
                Panel::Library => library_panel::show(ui, &shelves, shelf_tree, out),
                Panel::Notes => documents.reading(active).is_some_and(|r| {
                    notes_panel::show(ui, &r, &notes, out);
                    true
                }),
            };
        // The rail serves reading; the library keeps it, unseen, for return.
        let inset = match (route.has_rail(), overlay) {
            (false, _) => 0.0,
            (true, true) => self
                .sidebar
                .show_floating(&ctx, &rail, &mut body, &mut commands),
            (true, false) => self
                .sidebar
                .show_docked(ui, &rail, &mut body, &mut commands),
        };
        let tokens = theme::Tokens::get(&ctx);
        let view = PaneView {
            active: self.active,
            look: self.settings.workspace.look,
            chrome_bottom: self.titlebar.band_bottom(&ctx),
            drop_target,
            styles: self.pane_styles(&tokens),
            recent: self.recent_books(),
        };
        // Each pane reads its own paper while it has a colour of its own.
        let every = self.routing().colour();
        let source = self.paper_source();
        let corner = view.corner(self.tree.len() > 1, &tokens);
        let blend = self.settings.paper.blend;
        // A pinned bar covers the panes' top; documents start below it.
        let pinned_bottom = if self.titlebar.pinned {
            view.chrome_bottom
        } else {
            f32::NEG_INFINITY
        };
        // The bar's hits paint in the pane it searches.
        let lit = found_in(&self.search, &self.documents, &self.find).map(|f| Lit {
            hits: f.hits.clone(),
            active: self.find.active(),
        });
        let lit = self.find.pane().zip(lit);
        let documents = &mut self.documents;
        let (layout, texts, window) = (&self.settings.layout, &self.texts, &self.settings.text);
        let (library, library_view) = (&mut self.library, &self.settings.library);
        let study = &mut self.study;
        let page_ai = &mut self.page_ai;
        study.begin_frame();
        let highlights = &self.settings.study.highlights;
        let vocab = &mut self.vocab;
        let cefr = &self.settings.study.cefr;
        let shading = cefr.enabled.then_some(crate::vocab::Look {
            level: cefr.level.band(),
            click: cefr.click_explain,
            hover: self.settings.dictionary.hover,
            ink: highlights.cefr,
            opacity: highlights.opacity,
        });
        self.library_rect = None;
        let menu_shot = self.menu_shot;
        let lift = self.tree.len() > 1;
        let dictionary_page = (route == Route::Dictionary).then_some(&mut self.dict);
        let dictionary = &self.settings.dictionary;
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if let Some(dict) = dictionary_page {
                self.pane_rects.clear();
                let top = ui.max_rect().top() + crate::theme::TITLE_BAR_H;
                dict.page(ui, dictionary, top, &mut commands);
                return;
            }
            if library.showing {
                self.pane_rects.clear();
                self.library_rect = Some(ui.max_rect());
                library::view::show(ui, library, library_view, view.chrome_bottom, &mut commands);
                return;
            }
            let lifted = self.lifting.map(|l| l.pane);
            let tree = &self.tree;
            self.pane_rects = panes::show(ui, tree, lifted, &view, &mut commands, |ui, id| {
                let style = view.style(id);
                let cx = ViewContext {
                    active: id == view.active,
                    lift,
                    tokens: style.map_or(tokens, |s| s.tokens),
                    top_inset: (pinned_bottom - ui.max_rect().top()).max(0.0),
                    page: PageLook::new(layout, texts.of(id, window)),
                    paper: style.and_then(|s| s.paper),
                    corner,
                    sample: blend && (every || Some(id) == source),
                    marks: {
                        let book = library.book_in(documents, id);
                        let name = documents.title(id).unwrap_or_default();
                        study.marks_of(library.model(), book, name, highlights)
                    },
                    bookmarks: bookmarks::on_pages(study, library, documents, id)
                        .into_iter()
                        .map(|(_, page)| page)
                        .collect(),
                    highlights: highlights.clone(),
                    card: study.card_mark(id),
                    picked: study.picked(id),
                    shading,
                    menu_shot: menu_shot && id == view.active,
                    found: lit
                        .as_ref()
                        .filter(|(p, _)| *p == id)
                        .map(|(_, l)| l.clone()),
                    ai: {
                        let book = library.book_in(documents, id);
                        let name = documents.title(id).unwrap_or_default();
                        page_ai.overlays(&ctx, id, library.model(), documents, (book, name))
                    },
                };
                documents.show(ui, id, &cx, Some(&mut *vocab))
            });
        });
        self.library.end_frame(&ctx);
        self.glosses(&ctx, &mut commands);
        if route == Route::Reader {
            let mut stage = ctx.content_rect();
            stage.min.x += inset;
            stage.min.y += crate::theme::TITLE_BAR_H;
            // Above the page bar too, which would cover it.
            stage.max.y -= crate::page_bar::BAR_H;
            let dictionary = &self.settings.dictionary;
            self.dict.window(&ctx, dictionary, stage, &mut commands);
        }
        self.lifted(&ctx);
        self.drops_from_rail(ui, &mut commands);
        library::sheets::show(&ctx, &self.library, &mut commands);
        let backdrop = self.backdrop(&ctx, &view);
        self.page_bars(&ctx, &backdrop, &mut commands);
        let top = self.titlebar.band_bottom(&ctx);
        let found = found_in(&self.search, &self.documents, &self.find);
        self.find.show(&ctx, &self.pane_rects, top, found);
        self.drive_search(&ctx);
        // Typing, or filtered by what was typed, the field stays in view.
        let searching =
            route == Route::Library && (self.library.searching() || library::search::focused(&ctx));
        let band = ctx
            .content_rect()
            .with_max_y(self.titlebar.band_bottom(&ctx));
        let tabbing = crate::focus::within(&ctx, band);
        // The dictionary's bar is its only way out, so it stays.
        let page = route == Route::Dictionary;
        self.titlebar.hold = self.find.is_open() || searching || tabbing || page;
        self.follow_focus(&ctx, &mut commands);
        self.documents.end_frame();
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        let presets = self.settings.all_presets();
        let (title, look, page) = match route {
            Route::Reader => (
                self.documents.title(self.active).unwrap_or("Mareader"),
                self.look(self.active),
                BarPage::Reader(self.reader_bar(overlay)),
            ),
            Route::Library => {
                let width = self
                    .library_rect
                    .map_or(ctx.content_rect().width(), |r| r.width());
                let library = LibraryBar {
                    view: &self.settings.library,
                    auto_columns: library::view::page_columns(width),
                    can_return: self.reading(),
                    trail: self.library.trail(),
                    renaming: self.library.renaming(),
                    search: self.library.field(),
                    organizing: self.library.organizer().busy(),
                    paused: (self.library.organizer().paused())
                        .map(library::organizing::Paused::short),
                };
                // On a shelf, the breadcrumb names the page.
                let title = if library.trail.is_empty() {
                    "Library"
                } else {
                    ""
                };
                (title, self.settings.appearance, BarPage::Library(library))
            }
            Route::Dictionary => {
                let back = match self.dict_back() {
                    Route::Reader => "Back to reading",
                    _ => "Back to the library",
                };
                let page = BarPage::Dictionary(DictionaryBar { back });
                ("Dictionary", self.settings.appearance, page)
            }
        };
        let bar = BarContext {
            title,
            platform: self.platform,
            left_inset: inset,
            maximized: ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
            fullscreen,
            // With no rail beside it, the bar hosts the lights.
            gutter: (!overlay || !route.has_rail()) && inset <= 0.0 && !fullscreen,
            appearance: &look,
            presets: &presets,
            user_presets: &self.settings.user_presets,
            gpu: self.documents.gpu(),
            backdrop: &backdrop,
            page,
        };
        self.titlebar.show(&ctx, &bar, &mut commands);
        if route == Route::Library {
            library::search::suggestions(&ctx, &mut self.library, &mut commands);
        }
        self.lights = self.place_lights(&ctx, overlay, route);
        // Shots and the preview simulate macOS; draw where the lights go.
        if self.platform == Platform::Mac && !cfg!(target_os = "macos") {
            stand_in_lights(&ctx, self.lights);
        }
        let mut stage = ctx.content_rect();
        stage.min.x += inset;
        self.toasts.show(&ctx, stage, &mut commands);
        // A run outlives the page that started it, so cards show on both.
        let lift = match route {
            Route::Reader => crate::page_bar::BAR_H + 2.0 * crate::page_bar::EDGE,
            Route::Library | Route::Dictionary => 0.0,
        };
        // A paused run waits on the library page, off the pages read.
        let paused = route == Route::Library;
        let runs = &mut self.library.runs;
        library::runs::show(&ctx, runs, (stage, lift), paused, &mut commands);
        let window = self.settings_scope();
        self.settings_window.show(
            &ctx,
            &self.settings,
            &window,
            (&self.ai, &self.dict, &self.vocab),
            &mut commands,
        );
        crate::shortcuts::show(&ctx, self.platform);
        crate::focus::paint(&ctx);
        self.film_grain(&ctx);
        self.fade.paint(&ctx);
        // Applied after drawing, so the next frame must show the result.
        if !commands.is_empty() {
            ctx.request_repaint();
            // A change still waiting on its snapshot lands first.
            if let Some(after) = self.fade.take() {
                self.restore(after);
            }
            let before = self.held();
            for command in commands {
                self.apply(&ctx, command);
            }
            self.reconcile_looks(&ctx);
            // eframe autosaves only on a frame, so book one after the interval.
            if self.settings != before.settings {
                ctx.request_repaint_after(SAVE_EVERY + Duration::from_millis(50));
            }
            if !outside && self.may_fade(&ctx) && self.leapt(&before) {
                let after = self.held();
                self.restore(before);
                self.fade.hold(&ctx, after);
            }
        }
        // This frame drew with the old look; show the new one next frame.
        if self.restyle(&ctx) {
            ctx.request_repaint();
        }
        self.machine_fonts.after_frame(&ctx);
    }

    /// Writes happen on eframe's persist thread, and only when changed.
    fn auto_save_interval(&self) -> Duration {
        SAVE_EVERY
    }

    fn save(&mut self, storage: &mut dyn Storage) {
        self.library.flush();
        storage.set_string(SETTINGS_KEY, mareader_settings::to_json(&self.settings));
        // Nothing open saves `null`: the next launch starts empty.
        let last = serde_json::to_string(&self.last_document());
        storage.set_string(LAST_DOCUMENT_KEY, last.unwrap_or_else(|_| "null".into()));
    }
}

/// What a cross-fade holds back: the settings and the panes' looks.
#[derive(Clone)]
struct Held {
    settings: Settings,
    looks: PaneLooks<PaneId>,
    routed: Routing,
}

/// Which families split panes own under `settings`.
fn routing(settings: &Settings, split: bool) -> Routing {
    let w = &settings.workspace;
    Routing {
        themes: w.independent_themes,
        textures: w.independent_textures,
        shared_base: w.shared_base_mode,
        split,
    }
}

/// SplitMix64: a well-spread next state from any state.
fn splitmix(state: u64) -> u64 {
    let mut z = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// What the find bar's pane holds for its current query, if asked yet.
fn found_in<'a>(search: &'a Search, documents: &Documents, find: &FindBar) -> Option<&'a Found> {
    let reading = documents.reading(find.pane()?)?;
    let found = search.found(reading.doc, &reading.document)?;
    (found.query == find.query()).then_some(found)
}
