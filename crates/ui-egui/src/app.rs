//! The app state and its eframe loop.

mod access;
mod appearance;
mod apply;
mod frame;
mod glosses;
mod library;
mod navigation;
mod page_ai;
mod panes;
mod read_aloud;
mod setup;
mod study;
mod ui_state;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use egui::{Context, Rect};
use mareader_appearance::{Appearance, FilmGrain, PaneLooks, Rgb, Routing};
use mareader_session::Anchor;
use mareader_settings::{Bar, BookDesign, DictionaryOpens, MotionSettings, Settings, TextSettings};
use mareader_workspace::{Bounds, LiftTarget, MAX_PANES, PaneId, Side, SplitAxis};

use crate::backdrop::Backdrop;
use crate::bookmarks;
use crate::crossfade::Crossfade;
use crate::documents::Documents;
use crate::files::Files;
use crate::find_bar::FindBar;
use crate::floating_label::{self, LabelChrome};
use crate::gpu::{Glass, Gpu, GrainCallback, Quad, crawl_offset};
use crate::library::{self as ui_library, LibraryState};
use crate::library_panel::{self, PaneTab, PanelContext, PanelState};
use crate::machine_fonts::MachineFonts;
use crate::notes_panel;
use crate::outline_panel::{self, OutlineState};
use crate::page_bar::{self, BarLook, PageBar};
use crate::panes::{self as ui_panes, PaneStyle, PaneView};
use crate::pdf_view::{PageLook, ViewContext};
use crate::platform::{LightSpots, NativeLights, Platform};
use crate::search::{Found, Lit, Search};
use crate::settings_window::SettingsWindow;
use crate::sidebar::{Panel, RailContext, Sidebar};
use crate::study::{self as ui_study, Explained, Gloss, Highlighted, Study};
use crate::thumbnails::ThumbState;
use crate::titlebar::{self, BarContext, BarPage, DictionaryBar, LibraryBar, ReaderBar, TitleBar};
use crate::toast::Toasts;
use crate::ui_command::Allows;
use crate::ui_command::{Commands, Sent};
use crate::{keymap, motion, resize, theme};
use mareader_engine::command::{Command, Independence};
use mareader_engine::documents::{OpenRequest, Placement};
use mareader_engine::pane::ViewMode;
use mareader_engine::pane_texts::PaneTexts;
use mareader_engine::read_aloud::ReadFrom;
use mareader_engine::route::Route;
use mareader_engine::save::{LastDocument, Loaded};
use mareader_engine::scope::{Scope, Shown};
use mareader_engine::session::Session;
use mareader_engine::settings::Tab;
use mareader_engine::study::Action as StudyAction;
use mareader_engine::toast::Toast;
use ui_state::UiState;

/// Settings reach disk this long after a change, at most: the engine's own
/// `save::EVERY`, which a frame is booked just after.
pub const SAVE_EVERY: Duration = Duration::from_millis(500);

/// What a launch says when some choices came back as defaults.
const REPAIRED: &str = "Some settings couldn't be read and were reset to defaults.";

/// A generated sample for lookup screenshots, not a provider response.
struct AssistantSample;

impl mareader_study::Explainer for AssistantSample {
    fn by(&self) -> String {
        "Assistant-generated sample".into()
    }

    fn explain(
        &self,
        word: &str,
        _context: &str,
        partial: &(dyn Fn(mareader_study::WordInfo) + Sync),
    ) -> Result<mareader_study::WordInfo, mareader_study::Failure> {
        if !word.eq_ignore_ascii_case("acknowledged") {
            return Err(mareader_study::Why::BadAnswer.into());
        }
        let info = mareader_study::WordInfo {
            pos: "verb".into(),
            meaning: concat!(
                "Accepted as true or recognized. Here, Austen frames a belief about ",
                "wealthy bachelors and marriage as unquestioned, with a wry edge."
            )
            .into(),
            synonyms: vec![
                "recognized".into(),
                "accepted".into(),
                "assumed".into(),
                "understood".into(),
                "taken for granted".into(),
            ],
            usages: vec![
                "Her contribution was acknowledged in the report.".into(),
                "He acknowledged that the first plan had failed.".into(),
                "The report acknowledges the limits of the evidence.".into(),
            ],
        };
        partial(mareader_study::WordInfo {
            pos: info.pos.clone(),
            meaning: info.meaning.clone(),
            ..Default::default()
        });
        Ok(info.clamped())
    }
}

pub struct MareaderApp {
    /// The owner: settings, tree, route, and the door that changes them.
    session: Session,
    /// What dies with the window: `MareaderApp` is these two and nothing else.
    ui: UiState,
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
        let titlebar = TitleBar::default();
        Self {
            session: Session::new(settings),
            ui: UiState {
                titlebar,
                platform: Platform::current(),
                chromeless: false,
                menu_shot: false,
                read_aloud_shot: false,
                transcript_shot: Vec::new(),
                trigger_shot: Vec::new(),
                script_shot: Vec::new(),
                scripts: Default::default(),
                clips: mareader_engine::triggers::Clips::default(),
                page_ai_seed: None,
                word_window_seed: None,
                sidebar: Sidebar::default(),
                settings_window: SettingsWindow::default(),
                find: FindBar::default(),
                search: Search::default(),
                toasts: Toasts::default(),
                study: Study::default(),
                page_ai: crate::page_ai::PageAi::default(),
                read_aloud: crate::read_aloud::ReadAloud::default(),
                ai: crate::ai::Ai::default(),
                dict: crate::dict::Dict::default(),
                vocab: crate::vocab::Vocab::default(),
                sounds: crate::sounds::Sounds::default(),
                english: None,
                staged: Default::default(),
                lights: NativeLights::default(),
                light_spots: None,
                documents: Documents::default(),
                files: Files::default(),
                library: LibraryState::default(),
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
            },
        }
    }

    pub fn from_creation(cc: &eframe::CreationContext<'_>) -> Self {
        let dir = crate::import::state_dir();
        let loaded = crate::import::once(cc.storage, dir.clone());
        let mut app = Self::from_loaded(loaded);
        app.save_to(dir);
        app
    }

    /// An app from what a launch found: its choices, what to reopen, a repair.
    ///
    /// Unreadable settings arrive repaired section by section, and the notes
    /// that say so go to stderr and to one toast.
    pub fn from_loaded(loaded: Loaded) -> Self {
        for note in &loaded.notes {
            eprintln!("mareader: {note}");
        }
        let repaired = !loaded.notes.is_empty();
        let mut app = Self::with_settings(loaded.settings);
        app.ui.last = loaded.last;
        if repaired {
            app.ui.toasts.push(Toast::error(REPAIRED));
        }
        app
    }
}

/// What a cross-fade holds back: the settings and the panes' looks.
#[derive(Clone)]
struct Held {
    settings: Settings,
    looks: PaneLooks<PaneId>,
    routed: Routing,
}
