//! The app state and its eframe loop.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use eframe::Storage;
use egui::{Context, ViewportCommand};
use mareader_appearance::{
    Appearance, BaseMode, FilmGrain, NoiseMode, PaneLooks, Rgb, Routing, TextureMode,
    builtin_presets,
};
use mareader_session::Anchor;
use mareader_settings::{
    ColumnAlign, FontChoice, LibraryClick, MotionSettings, PaperArea, Settings,
};
use mareader_workspace::{Bounds, Edge, MAX_PANES, PaneId, PaneTree, Side, SplitAxis, TreeError};

use crate::backdrop::Backdrop;
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
use crate::outline_panel::{self, OutlineState};
use crate::page_bar::{self, BarLook, PageBar};
use crate::pane_texts::PaneTexts;
use crate::panes::{self, PaneStyle, PaneView};
use crate::pdf_view::{PageLook, ViewContext};
use crate::platform::{NativeLights, Platform};
use crate::route::Route;
use crate::settings_window::{Scope, SettingsWindow, Shown, Tab};
use crate::sidebar::{Panel, RailContext, Sidebar};
use crate::study::{Explained, Gloss, Study};
use crate::thumbnails::ThumbState;
use crate::titlebar::{self, BarContext, BarPage, LibraryBar, ReaderBar, TitleBar};
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
    tree: PaneTree,
    active: PaneId,
    sidebar: Sidebar,
    settings_window: SettingsWindow,
    find: FindBar,
    toasts: Toasts,
    /// The selection bar's answers, marks and the word card.
    study: Study,
    /// Who answers Explain, and the AI settings' background work.
    ai: crate::ai::Ai,
    /// A selection the options asked for, made once its page is laid out.
    staged: Option<Staged>,
    lights: NativeLights,
    documents: Documents,
    files: Files,
    library: LibraryState,
    /// The sidebar panels' scroll-following, for the focused pane.
    outline: OutlineState,
    thumbs: ThumbState,
    shelf_tree: PanelState,
    /// Each pane's page bar.
    page_bars: std::collections::BTreeMap<PaneId, PageBar>,
    /// Last frame's pane rects, to aim a file drop.
    pane_rects: Vec<(PaneId, egui::Rect)>,
    /// The library page's rect while it is up, for the glass behind bars.
    library_rect: Option<egui::Rect>,
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
    /// Its page was asked for once; the view is on its way there.
    turned: bool,
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
            tree: PaneTree::new(),
            active: PaneId(1),
            sidebar: Sidebar::default(),
            settings_window: SettingsWindow::default(),
            find: FindBar::default(),
            toasts: Toasts::default(),
            study: Study::default(),
            ai: crate::ai::Ai::default(),
            staged: None,
            lights: NativeLights::default(),
            documents: Documents::default(),
            files: Files::default(),
            library: LibraryState::default(),
            outline: OutlineState::default(),
            thumbs: ThumbState::default(),
            shelf_tree: PanelState::default(),
            page_bars: std::collections::BTreeMap::new(),
            pane_rects: Vec::new(),
            library_rect: None,
            looks: PaneLooks::default(),
            routed: Routing::default(),
            texts: PaneTexts::default(),
            random: 0x9e37_79b9_7f4a_7c15,
            fade: Crossfade::default(),
            styled: None,
            reduced_motion: Arc::new(AtomicBool::new(false)),
            last: None,
            fonts: FontState::Missing,
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

    pub fn library_showing(&self) -> bool {
        self.route() == Route::Library
    }

    /// The page in the window; the library replaces the panes, not ends them.
    pub fn route(&self) -> Route {
        match self.library.showing {
            true => Route::Library,
            false => Route::Reader,
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
            self.find.close();
            self.settings_window.close();
        }
        self.library.showing = route == Route::Library;
    }

    /// Covers held as textures, for tests.
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

    pub fn settings_window(&self) -> &SettingsWindow {
        &self.settings_window
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
        Scope {
            focused,
            split: self.tree.len() > 1,
            any_book: self.tree.panes().into_iter().any(|p| docs.reflowable(p)),
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
    pub fn study(&self) -> &Study {
        &self.study
    }

    /// The marks `pane` shows, wherever they are kept.
    pub fn marks(&self, pane: PaneId) -> Vec<(mareader_library::MarkId, mareader_library::Spot)> {
        let book = self.library.book_in(&self.documents, pane);
        let name = self.documents.title(pane).unwrap_or_default();
        self.study.marks_of(self.library.model(), book, name)
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
            "workers" => {
                let n = value
                    .parse()
                    .map_err(|_| "workers: a count, 0 runs inline")?;
                self.documents.set_workers(n);
                self.library.set_inline(n == 0);
                self.study.set_inline(n == 0);
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
            // A folder read in place, as the sheet can add one.
            "folder" => {
                let roots = vec![std::path::PathBuf::from(value)];
                // Watched and unshelved, so its books show as found.
                let plan = library::FolderPlan {
                    keep: library::FolderKeep::Watch,
                    walk: library::Walk::default(),
                };
                self.queue(Command::Library(library::Action::AddFolders {
                    roots,
                    plan,
                }));
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
                "canned" => self.study.set_explainer(Arc::new(mareader_study::Canned)),
                "none" => self
                    .study
                    .set_explainer(Arc::new(mareader_study::Unavailable)),
                _ => return Err("explain: canned or none".into()),
            },
            // The AI tab's choice, by its saved name, e.g. `openai`.
            "provider" => {
                let name = serde_json::Value::String(value.to_owned());
                self.settings.ai.provider =
                    serde_json::from_value(name).map_err(|_| "provider: e.g. openai or ollama")?;
            }
            // The first such text on a page selected, as `page:words`.
            "selection" => {
                let hint = "selection: page:text, as 0:quick";
                let (page, text) = value.split_once(':').ok_or(hint)?;
                let page = page.parse().map_err(|_| hint)?;
                let text = text.replace('_', " ");
                let explain = self.staged.as_ref().is_some_and(|s| s.explain);
                self.staged = Some(Staged {
                    page,
                    text,
                    explain,
                    turned: false,
                });
            }
            // Explains the staged selection: its card opens.
            "gloss" => match &mut self.staged {
                Some(staged) => staged.explain = true,
                None => return Err("gloss: after selection=page:text".into()),
            },
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
            "chromeless" => self.titlebar.chromeless = value == "true",
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
            "column_width" => {
                let pct = value.parse().map_err(|_| "column_width: %")?;
                self.settings.text.column_width_pct = pct;
            }
            "hyphenate" => self.settings.text.hyphenation = value == "true",
            "book_layout" => self.settings.text.book_layout = value == "true",
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
            "open" => match value {
                "appearance" => self.queue(Command::ShowAppearance),
                "settings" => self.queue(Command::ShowSettings),
                "menu" => self.queue(Command::ShowViewMenu),
                "find" => self.queue(Command::ShowFind),
                "search" => {
                    let focus = library::SearchAction::Focus;
                    self.queue(Command::Library(library::Action::Search(focus)));
                }
                "folders" => self.queue(Command::Library(library::Action::ShowFolders)),
                "add" => self.queue(Command::Library(library::Action::ShowAdd)),
                "add_folder" => {
                    let roots = vec![std::path::PathBuf::from("/home/reader/Books")];
                    self.queue(Command::Library(library::Action::AskFolders(roots)));
                }
                // The removal sheet for what `select` picked.
                "remove" => {
                    let books = self.queued.iter().find_map(|c| match c {
                        Command::Library(library::Action::Pick(library::PickAction::Only(p))) => {
                            Some(p.iter().filter_map(|p| match p {
                                library::Pick::Book(id) => Some(*id),
                                library::Pick::Shelf(_) => None,
                            }))
                        }
                        _ => None,
                    });
                    let books: Vec<_> = books.ok_or("open=remove: select books first")?.collect();
                    let shelves = Vec::new();
                    let ask = library::BookAction::AskRemove { books, shelves };
                    self.queue(Command::Library(library::Action::Book(ask)));
                }
                "rename" => {
                    let first = self.library.model().recent().first().map(|b| b.id);
                    let first = first.ok_or("open=rename: no book")?;
                    let ask = library::BookAction::AskRename(first);
                    self.queue(Command::Library(library::Action::Book(ask)));
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
        match self.staged.take() {
            // Its page is turned to first, then selected once laid out.
            Some(mut staged) if !sheets.is_empty() && !shown(staged.page) => {
                if !staged.turned {
                    out.push(Command::GoTo {
                        pane,
                        page: staged.page,
                    });
                }
                staged.turned = true;
                self.staged = Some(staged);
                ctx.request_repaint();
            }
            Some(Staged {
                page,
                text,
                explain,
                ..
            }) if !sheets.is_empty() => {
                out.push(Command::SelectText { pane, page, text });
                if explain {
                    out.push(Command::Explain(pane));
                }
            }
            staged => self.staged = staged,
        }
        for (pane, gloss) in self.documents.take_gloss() {
            match gloss {
                Gloss::Explain => out.push(Command::Explain(pane)),
                Gloss::Copied => out.push(Command::Notify(Toast::info("Copied"))),
                Gloss::Open { mark, at } => {
                    self.study
                        .open_mark(ctx, self.library.model(), pane, mark, at);
                }
                Gloss::Menu { mark, at } => self.study.open_menu(mark, at),
                Gloss::Anchor(at) => self.study.anchor(pane, at),
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
        self.study.receive();
        let look = &self.settings.study.gloss;
        self.study.show(ctx, &self.pane_rects, look, out);
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
            Command::ClosePane(id) => {
                if self.tree.close(id).is_ok() {
                    self.documents.close_pane(id);
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
            Command::FirstPage => self.documents.go_to(self.active, 0),
            Command::LastPage => self.documents.go_to(self.active, usize::MAX),
            Command::GoTo { pane, page } => self.documents.go_to(pane, page),
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
            Command::SetAppearance(a) if self.route() == Route::Library => {
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
            Command::DismissToast => self.toasts.dismiss(ctx.input(|i| i.time)),
            Command::Study(action) => {
                let mut out = Commands::new();
                let model = self.library.model_mut();
                self.study.act(ctx, model, action, &mut out);
                self.queued.extend(out);
            }
            Command::Explain(pane) => self.explain(ctx, pane),
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
                    Route::Reader => crate::view_menu::VIEW_MENU,
                    Route::Library => crate::library::menu::LIBRARY_VIEW_MENU,
                };
                egui::Popup::open_id(ctx, egui::Id::new(menu));
            }
            Command::ShowSettings => self.settings_window.open(None),
            // With nothing to search in, find searches the library.
            Command::ShowFind => match (self.route(), self.documents.reading(self.active)) {
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
                Route::Reader => self.go(Route::Library),
                // An empty reader is no place to go back to.
                Route::Library if self.reading() => self.go(Route::Reader),
                Route::Library => {}
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
            A::ShowFolders => {
                let model = self.library.model();
                let first = (model.sources())
                    .min_by_key(|s| s.name().to_lowercase())
                    .map(|s| s.id);
                if let Some(id) = first {
                    let scope = A::Scope(library::Scope::Folder(id));
                    self.library
                        .act(scope, ctx.input(|i| i.time), &mut Commands::new());
                    egui::Popup::open_id(ctx, library::folders::menu_id(id));
                }
            }
            A::ShowAdd => egui::Popup::open_id(ctx, egui::Id::new(library::ADD_MENU)),
            A::PickFolder => {
                let ask = |root| Command::Library(A::AskFolders(vec![root]));
                self.files.pick_folder(ctx, "Add a folder of books", ask);
            }
            A::PickSearchFolder(book) => {
                let seek = move |folder| Command::Library(A::Seek { book, folder });
                self.files.pick_folder(ctx, "Search a folder", seek);
            }
            A::Book(library::BookAction::PickImport) => {
                let to = self.library.at();
                let import = move |paths| {
                    let walk = library::Walk::default();
                    let action = library::BookAction::Import { paths, to, walk };
                    Command::Library(A::Book(action))
                };
                self.files.pick_books(ctx, import);
            }
            A::Book(library::BookAction::Reveal(book)) => {
                let path = self.library.model().book(book).and_then(|b| b.path());
                let shown = path.is_some_and(crate::reveal::reveal);
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
            action => {
                let mut out = Commands::new();
                self.library.act(action, ctx.input(|i| i.time), &mut out);
                for command in out {
                    self.apply(ctx, command);
                }
            }
        }
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
        if self.library.showing {
            return;
        }
        let floating = self.settings.chrome.sidebar_overlay;
        let rail = (self.sidebar.offset()).filter(|_| floating);
        let edge = rail.map(|o| o + crate::sidebar::WIDTH);
        let empty = |pane| self.documents.title(pane).is_none();
        let name = |pane| self.documents.title(pane).unwrap_or("this pane").to_owned();
        library_panel::drop_overlay(ui, &self.pane_rects, edge, empty, name, out);
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
            let (folders, paths): (Vec<PathBuf>, Vec<PathBuf>) = paths.partition(|p| p.is_dir());
            // Folders ask how they're kept; files are copied in.
            if !folders.is_empty() {
                out.push(Command::Library(library::Action::AskFolders(folders)));
            }
            if !paths.is_empty() {
                let to = self.library.file_target.or(self.library.at());
                let walk = library::Walk::default();
                let import = library::BookAction::Import { paths, to, walk };
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
                let look = BarLook {
                    layout: &self.settings.layout,
                    backdrop,
                    slot: i + 1,
                };
                let bar = self.page_bars.entry(*id).or_default();
                page_bar::show(ctx, *rect, &reading, &look, bar, out);
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

    /// Blend's raw paper: the focused PDF's in every pane, or each its own.
    fn blend_paper(&self, pane: PaneId) -> Option<Rgb> {
        let paper = &self.settings.paper;
        let from = if self.routing().colour() {
            pane
        } else {
            self.active
        };
        paper
            .blend
            .then(|| self.documents.paper(from, paper.area))
            .flatten()
    }

    /// The library keeps its own pin; the reader's is the other.
    fn pin_setting(&mut self) -> &mut bool {
        let route = self.route();
        let chrome = &mut self.settings.chrome;
        match route {
            Route::Library => &mut chrome.library_titlebar_pinned,
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
            let fill = paper.map_or(tokens.paper, |raw| baked(&tokens, raw));
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
            independence: routing.split.then_some(Independence {
                themes: routing.themes,
                textures: routing.textures,
            }),
            zoom: self.documents.zoom_state(self.active),
            mode: self.documents.mode(self.active),
            ink: (self.documents.reflowable(self.active))
                .then_some(self.settings.text.ink_contrast),
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
        if self.documents.gpu().is_none()
            && let Some(render) = frame.wgpu_render_state()
        {
            self.documents.set_gpu(Gpu::new(render));
        }
        // Fonts from set_fonts arrive next frame; named families panic before.
        if self.fonts != FontState::Ready {
            self.fonts = FontState::Ready;
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
        let own = self.texts.own(&self.settings.text);
        self.documents.set_text(&self.settings.text, &own);
        self.documents.receive(&ctx, &mut commands);
        self.files.collect(&mut commands);
        self.library.receive(&ctx, &mut commands);
        self.library.tick(&ctx);
        // Inline, the measure lands next frame; book it.
        if self.library.observe(&self.documents, &self.tree.panes()) {
            ctx.request_repaint();
        }
        if let Some(wait) = self.library.save(ctx.input(|i| i.time)) {
            ctx.request_repaint_after(Duration::from_secs_f64(wait));
        }
        let route = self.route();
        keymap::read(&ctx, self.platform, route, &mut commands);
        let drop_target = self.drops(&ctx, &mut commands);
        // An open popup takes Escape first.
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        // Naming a shelf takes Escape to cancel.
        let modal = self.settings_window.is_open()
            || self.library.sheet().is_some()
            || self.library.renaming().is_some()
            || (route == Route::Library && library::search::focused(&ctx));
        let mut free = escape && !modal && !egui::Popup::is_any_open(&ctx);
        // The library page lets go of a selection, else goes back to reading.
        if free && route == Route::Library {
            if !self.library.escape() {
                commands.push(Command::ShowLibrary);
            }
            free = false;
        }
        if free && self.study.escape() {
            free = false;
        }
        if free && self.find.is_open() {
            self.find.close();
            free = false;
        }
        // A selection takes Escape before the sidebar does.
        if free && !self.documents.clear_selection(self.active) && self.sidebar.is_open() {
            commands.push(Command::CloseSidebar);
        }
        let overlay = self.settings.chrome.sidebar_overlay;
        let rail = RailContext {
            overlay,
            slide: self.settings.motion.sidebar_slide,
            platform: self.platform,
        };
        let tabs = self.tabs();
        let shelves = PanelContext {
            model: self.library.model(),
            tabs: &tabs,
            active: self.active,
            click: self.settings.workspace.library_click,
            home: self.settings.library.home,
        };
        let shelf_tree = &mut self.shelf_tree;
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
                Panel::Dictionary => false,
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
        };
        // Each pane reads its own paper while it has a colour of its own.
        let every = self.routing().colour();
        let corner = view.corner(self.tree.len() > 1, &tokens);
        let blend = self.settings.paper.blend;
        // A pinned bar covers the panes' top; documents start below it.
        let pinned_bottom = if self.titlebar.pinned {
            view.chrome_bottom
        } else {
            f32::NEG_INFINITY
        };
        let documents = &mut self.documents;
        let (layout, texts, window) = (&self.settings.layout, &self.texts, &self.settings.text);
        let (library, library_view) = (&mut self.library, &self.settings.library);
        let study = &mut self.study;
        study.begin_frame();
        let gloss = crate::study::tint(&self.settings.study.gloss, &tokens);
        self.library_rect = None;
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if library.showing {
                self.pane_rects.clear();
                self.library_rect = Some(ui.max_rect());
                library::view::show(ui, library, library_view, view.chrome_bottom, &mut commands);
                return;
            }
            self.pane_rects = panes::show(ui, &self.tree, &view, &mut commands, |ui, id| {
                let style = view.style(id);
                let cx = ViewContext {
                    active: id == view.active,
                    tokens: style.map_or(tokens, |s| s.tokens),
                    top_inset: (pinned_bottom - ui.max_rect().top()).max(0.0),
                    page: PageLook::new(layout, texts.of(id, window)),
                    paper: style.and_then(|s| s.paper),
                    corner,
                    sample: blend && (every || id == view.active),
                    marks: {
                        let book = library.book_in(documents, id);
                        let name = documents.title(id).unwrap_or_default();
                        study.marks_of(library.model(), book, name)
                    },
                    gloss,
                    card: study.card_mark(id),
                };
                documents.show(ui, id, &cx)
            });
        });
        self.library.end_frame(&ctx);
        self.glosses(&ctx, &mut commands);
        self.drops_from_rail(ui, &mut commands);
        library::sheets::show(&ctx, &self.library, &mut commands);
        let backdrop = self.backdrop(&ctx, &view);
        self.page_bars(&ctx, &backdrop, &mut commands);
        let top = self.titlebar.band_bottom(&ctx);
        self.find.show(&ctx, &self.pane_rects, top);
        // Typing, or filtered by what was typed, the field stays in view.
        let searching =
            route == Route::Library && (self.library.searching() || library::search::focused(&ctx));
        self.titlebar.hold = self.find.is_open() || searching;
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
                };
                // On a shelf, the breadcrumb names the page.
                let title = if library.trail.is_empty() {
                    "Library"
                } else {
                    ""
                };
                (title, self.settings.appearance, BarPage::Library(library))
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
            Route::Library => 0.0,
        };
        library::runs::show(&ctx, &mut self.library.runs, stage, lift);
        let window = self.settings_scope();
        self.settings_window
            .show(&ctx, &self.settings, &window, &self.ai, &mut commands);
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
