//! What the reducer still asks of its front end.
//!
//! The session owns the settings, the panes and the route. Documents, the
//! library, study marks, speech and the AI are still run by controllers in
//! the front end, so an command that lands on one is finished through this
//! trait. Every method names one controller call with its arguments already
//! decided here; none of them chooses what an command means.
//!
//! A controller that moves below the line takes its methods with it, and the
//! trait ends when the last one has gone. Tests implement it without a
//! window (`tests/replay.rs`).

use std::path::PathBuf;

use mareader_appearance::{Appearance, Rgb};
use mareader_library::{BookId, Pen};
use mareader_settings::{ReadAloudSettings, WordList};
use mareader_speech::{Item, Model, Spot};
use mareader_workspace::{Bounds, PaneId};

use crate::dict::{Action as DictAction, Download, Window};
use crate::documents::{OpenRequest, Placement};
use crate::export::Format;
use crate::library::Action as LibraryAction;
use crate::page_ai::Action as PageAiAction;
use crate::pane::{Arrow, ViewMode, ZoomCommand};
use crate::read_aloud::ReadFrom;
use crate::route::Route;
use crate::session::Session;
use crate::settings::Tab;
use crate::study::Action as StudyAction;
use crate::toast::Toast;

/// The Dictionary's tab parked on the window's edge, as F6 finds it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EdgeTab {
    /// The tab is on screen to be visited.
    pub shown: bool,
    /// The keys are on it now.
    pub focused: bool,
}

/// The front end's controllers, as the reducer calls them.
pub trait View {
    /// The state the reducer changes; lent for the length of one command.
    fn session(&mut self) -> &mut Session;

    /// The room the panes share, in points: a split measures against it.
    fn area(&self) -> Bounds;
    fn edge_tab(&self) -> EdgeTab;
    /// The route changed; overlays of the page left behind close.
    fn arrived(&mut self, route: Route);
    /// A pane left the tree: its document and its AI results go with it.
    fn close_pane(&mut self, pane: PaneId);

    /// The request as it should open now; none while the library loads.
    fn resume(&mut self, request: OpenRequest) -> Option<OpenRequest>;
    /// Opens the request in `pane`; a refusal comes back as a note.
    fn open(&mut self, pane: PaneId, request: OpenRequest) -> Option<Toast>;
    fn zoom(&mut self, pane: PaneId, zoom: ZoomCommand, now: f64);
    fn set_mode(&mut self, pane: PaneId, mode: ViewMode, now: f64);
    fn arrow(&mut self, pane: PaneId, arrow: Arrow, now: f64);
    fn page_key(&mut self, pane: PaneId, forward: bool);
    /// False when the pane's view does not scroll.
    fn toggle_drift(&mut self, pane: PaneId, now: f64) -> bool;
    fn go_to(&mut self, pane: PaneId, page: usize);
    fn turn(&mut self, pane: PaneId, forward: bool);
    fn copy_pages(&mut self, pane: PaneId, pages: Vec<usize>);
    /// Writes an export where the reader says.
    fn save(&mut self, name: String, format: Format, contents: String);
    fn book(&self, pane: PaneId) -> Option<BookId>;
    fn path(&self, pane: PaneId) -> Option<PathBuf>;
    /// Reopens each pane's document at its first page.
    fn restart_at_start(&mut self, panes: &[PaneId]) -> Vec<Toast>;

    /// The look of the focused pane, or of the window, as routing says.
    fn set_appearance(&mut self, look: Appearance);
    fn open_settings(&mut self, tab: Tab);

    fn word_list(&mut self, pane: PaneId, pages: Vec<usize>, list: WordList);
    fn word_window(&mut self, pane: PaneId, pages: Vec<usize>, list: WordList);
    fn study(&mut self, action: StudyAction);
    fn explain(&mut self, pane: PaneId);
    fn look_up(&mut self, pane: PaneId);
    fn highlight(&mut self, pane: PaneId, pen: Pen, color: Option<Rgb>);
    fn note(&mut self, pane: PaneId);
    fn bookmark(&mut self, pane: PaneId);
    fn select(&mut self, pane: PaneId, page: usize, text: &str);
    fn dict_window(&mut self, window: Window);
    fn pursue(&mut self, word: &str);
    fn dict(&mut self, action: DictAction);
    fn vocab(&mut self, action: Download);
    #[cfg(not(target_arch = "wasm32"))]
    fn attach_vocab(&mut self, root: PathBuf) -> Result<(), String>;
    fn pronunciation(&mut self, action: Download) -> Result<(), String>;

    fn page_ai(&mut self, action: PageAiAction);
    #[cfg(not(target_arch = "wasm32"))]
    fn ai(&mut self, job: mareader_settings::Job);

    fn place_read_marker(&mut self, pane: PaneId, spot: Spot, now: f64);
    fn dismiss_read_markers(&mut self, now: f64);
    /// Reads `pane`'s book aloud, if it holds one.
    fn read_aloud(&mut self, pane: PaneId, from: ReadFrom);
    /// Plays or pauses; false when nothing is being read.
    fn toggle_read_aloud(&mut self) -> bool;
    fn stop_read_aloud(&mut self);
    fn try_voice(&mut self);
    fn try_voice_of(&mut self, voice: &str);
    fn stop_voice_try(&mut self);
    fn say_word(&mut self, word: &str, voice: Option<&str>) -> Result<(), String>;
    fn stop_saying(&mut self);
    fn reveal_voice(&mut self, model: &'static Model, language: &'static str);
    /// A model's download or removal; it may rewrite the voice picked.
    fn voice_model(
        &mut self,
        item: Item,
        action: Download,
        settings: &mut ReadAloudSettings,
    ) -> Result<(), String>;

    fn open_book(&mut self, book: BookId, pane: Option<PaneId>, placement: Placement);
    fn library(&mut self, action: LibraryAction);
}
