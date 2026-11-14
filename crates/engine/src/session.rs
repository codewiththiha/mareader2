//! The app's state, and the one door that changes it.
//!
//! `Session` owns what outlives a frame and is not a drawing: the settings,
//! the pane tree and which pane is awake, the panes by last focus, and the
//! route. A front end draws that state and asks for changes by handing an
//! [`Command`] to `Session::execute` (`execute.rs`); what the session cannot
//! show itself leaves as an [`Event`], drained once a frame.
//!
//! Documents, the library and the study tools are still run by front-end
//! controllers, reached through [`crate::view::View`] until they move here.

use std::sync::Arc;

use mareader_settings::Settings;
use mareader_workspace::{Bounds, Edge, MAX_PANES, PaneId, PaneTree, TreeError};

use crate::command::Command;
use crate::event::Event;
use crate::host::Shared;
use crate::route::Route;
use crate::save::{self, LastDocument, Save};
use crate::toast::Toast;

/// Asks for a frame from any thread; the same alias `speech` exports.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

pub struct Session {
    settings: Settings,
    tree: PaneTree,
    active: PaneId,
    /// Panes by last focus, latest first: blend's paper follows them.
    recent: Vec<PaneId>,
    route: Route,
    /// Where the Dictionary page was opened from.
    dict_from: Option<Route>,
    /// Results not yet drained, oldest first.
    events: Vec<Event>,
    waker: Waker,
    /// The machine it runs on, held from the moment it is built.
    host: Shared,
    /// Its own two files; a session with no directory writes nothing.
    save: Save,
}

/// What an command the session carries out alone came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Done {
    /// True when the state changed and the window should redraw.
    pub dirty: bool,
    /// Why an command was refused, if it was: a bad path, a bad value.
    pub refused: Option<String>,
}

impl Done {
    fn quiet() -> Self {
        Self::default()
    }

    fn changed() -> Self {
        Self {
            dirty: true,
            refused: None,
        }
    }
}

impl Session {
    /// A session on `host`, woken by `waker`, holding what its own file says.
    ///
    /// The settings come from `settings.json` under [`Host::state_dir`]; a
    /// file that is not there, or cannot be read, leaves the defaults. A
    /// launch that found values elsewhere hands them to `Session::import`.
    ///
    /// [`Host::state_dir`]: crate::host::Host::state_dir
    pub fn new(host: Shared, waker: Waker) -> Self {
        let save = Save::new(host.state_dir());
        let loaded = save::load(save.dir());
        let mut settings = loaded.settings;
        settings.sanitize();
        let tree = PaneTree::new();
        Self {
            host,
            waker,
            save,
            settings,
            active: tree.panes().first().copied().unwrap_or(PaneId(1)),
            tree,
            recent: Vec::new(),
            route: Route::Reader,
            dict_from: None,
            events: Vec::new(),
        }
    }

    /// Takes the settings a launch found before the engine's own file.
    ///
    /// The one-time import: a browser store's values, or a blob an older build
    /// left there. A launch that found nothing keeps what `new` read.
    pub fn import(&mut self, mut settings: Settings) {
        settings.sanitize();
        self.settings = settings;
    }

    /// Whether it has somewhere to write, so a caller can build what to put.
    pub fn persists(&self) -> bool {
        self.save.dir().is_some()
    }

    /// Remembers what was read, then writes when a write is due.
    ///
    /// The answer is the wait left when it skipped, for a frame to book.
    pub fn keep(&mut self, now: f64, last: Option<LastDocument>) -> Option<f64> {
        self.save.note(last);
        self.save.write(now, &self.settings)
    }

    /// Writes what is owed at once: the window is going.
    pub fn settle(&mut self, last: Option<LastDocument>) {
        self.save.note(last);
        self.flush();
    }

    /// Both files at once, with the document last noted.
    ///
    /// What eframe's persist hook asks for, and what a test can: the cadence
    /// is skipped, and a session with nowhere to write writes nothing.
    pub fn flush(&mut self) {
        self.save.flush(&self.settings);
    }

    /// The two files' text, for a platform that has no files to write them to.
    pub fn blobs(&self) -> Result<save::Blobs, String> {
        save::blobs(&self.settings, self.save.last())
    }

    /// Something persisted changed outside a command.
    pub fn touch(&mut self) {
        self.save.touch();
    }

    /// Why a write was refused, once; a caller turns it into a toast.
    pub fn take_failure(&mut self) -> Option<String> {
        self.save.take_failure()
    }

    /// The machine it runs on, for a command that reaches the clipboard.
    pub fn host(&self) -> &Shared {
        &self.host
    }

    /// Says something a front end shows when it drains.
    pub fn report(&mut self, toast: Toast) {
        self.emit(Event::Toast(toast));
    }

    /// Asks the front end for a frame.
    pub fn wake(&self) {
        (self.waker)();
    }

    /// Everything reported since the last drain, oldest first.
    pub fn drain(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn emit(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The settings, writable, for a caller that holds the owner alone.
    ///
    /// Tests and shots set dials directly; the app itself goes through
    /// `execute`, so nothing outside has to know a field's name.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn tree(&self) -> &PaneTree {
        &self.tree
    }

    pub fn active(&self) -> PaneId {
        self.active
    }

    pub fn route(&self) -> Route {
        self.route
    }

    pub fn dict_from(&self) -> Option<Route> {
        self.dict_from
    }

    /// Panes by last focus, latest first.
    pub fn recent(&self) -> &[PaneId] {
        &self.recent
    }

    /// Makes `id` the focused pane, newest in the recency list.
    pub fn focus(&mut self, id: PaneId) {
        if !self.tree.panes().contains(&id) {
            return;
        }
        self.active = id;
        self.restage();
    }

    /// Puts the focused pane first in `recent`, dropping closed panes.
    fn restage(&mut self) {
        let placed = self.tree.panes();
        let active = self.active;
        self.recent.retain(|p| placed.contains(p) && *p != active);
        self.recent.insert(0, active);
        let unseen: Vec<PaneId> = placed
            .into_iter()
            .filter(|p| !self.recent.contains(p))
            .collect();
        self.recent.extend(unseen);
    }

    /// The page the window shows; only the Dictionary remembers where from.
    pub fn show(&mut self, route: Route) {
        let here = self.route;
        self.dict_from = match route {
            Route::Dictionary if here == Route::Dictionary => self.dict_from,
            Route::Dictionary => Some(here),
            _ => None,
        };
        self.route = route;
    }

    /// A new pane on `edge` of `pane`, in `area`; a refusal is a note.
    ///
    /// The geometry is the window's, so it arrives as plain numbers.
    pub fn split_at(&mut self, pane: PaneId, edge: Edge, area: Bounds) -> Option<PaneId> {
        let (axis, side) = edge.placement();
        let why = match self.tree.split(pane, axis, side, area) {
            Ok(id) => return Some(id),
            Err(TreeError::Full) => format!("A workspace holds {MAX_PANES} panes at most."),
            Err(_) => "This pane is too small to split.".to_owned(),
        };
        self.emit(Event::Toast(Toast::info(why)));
        None
    }

    /// Right of `pane`, else below it when the right has no room.
    pub fn beside(&self, pane: PaneId, area: Bounds) -> Edge {
        let mut probe = self.tree.clone();
        let (axis, side) = Edge::Right.placement();
        match probe.split(pane, axis, side, area) {
            Err(TreeError::NoRoom(_)) => Edge::Bottom,
            _ => Edge::Right,
        }
    }

    pub fn close(&mut self, id: PaneId) -> bool {
        if self.tree.close(id).is_err() {
            return false;
        }
        self.recent.retain(|p| *p != id);
        if self.active == id {
            let next = self.tree.panes().first().copied().unwrap_or(PaneId(1));
            self.active = next;
        }
        true
    }

    /// The tree, writable: the app's pane surgery goes through here so the
    /// engine keeps the one copy.
    pub fn tree_mut(&mut self) -> &mut PaneTree {
        &mut self.tree
    }

    /// Carries out an command that needs no controller: panes and dials.
    ///
    /// `execute` routes those here; the rest answer quiet, untouched.
    pub fn apply(&mut self, intent: Command) -> Done {
        match intent {
            Command::FocusPane(id) => {
                self.focus(id);
                Done::changed()
            }
            Command::ClosePane(id) => match self.close(id) {
                true => Done::changed(),
                false => Done::quiet(),
            },
            Command::SetRatio(path, ratio) => {
                self.tree.set_ratio(&path, ratio);
                Done::changed()
            }
            Command::SetSettings(settings) => {
                let mut settings = *settings;
                settings.sanitize();
                self.settings = settings;
                Done::changed()
            }
            Command::SetField { path, value } => self.put(&path, Ok(value)),
            other => self.dial(other),
        }
    }

    /// The dials an command names outright, each written as its path is.
    fn dial(&mut self, intent: Command) -> Done {
        match intent {
            Command::SetSidebarOverlay(on) => {
                self.settings.chrome.sidebar_overlay = on;
                Done::changed()
            }
            Command::SetTitlebarPinned(on) => {
                let bar = self.route.bar();
                self.settings.chrome.pin(bar, on);
                Done::changed()
            }
            Command::SetInkContrast(ink) => {
                self.put("text.ink_contrast", serde_json::to_string(&ink))
            }
            Command::SetQuickText(quick) => {
                let mut text = self.settings.text.clone();
                quick.apply(&mut text);
                self.put("text", serde_json::to_string(&text))
            }
            Command::SetIndependence(own) => {
                let themes = serde_json::to_string(&own.themes);
                self.put("workspace.independent_themes", themes);
                let textures = serde_json::to_string(&own.textures);
                self.put("workspace.independent_textures", textures)
            }
            Command::SavePreset { name, group, look } => {
                self.settings.save_preset(&name, &group, look);
                self.settings.sanitize();
                Done::changed()
            }
            Command::DeletePreset(id) => {
                self.settings.delete_preset(&id);
                self.settings.sanitize();
                Done::changed()
            }
            Command::SetDictionary(dictionary) => {
                self.put("dictionary", serde_json::to_string(&*dictionary))
            }
            Command::SetLibraryView(view) => self.put("library", serde_json::to_string(&view)),
            _ => Done::quiet(),
        }
    }

    /// One setting by its dotted path; a bad path or value is refused.
    fn put(&mut self, path: &str, json: serde_json::Result<String>) -> Done {
        let set = json
            .map_err(|e| e.to_string())
            .and_then(|value| self.settings.set_path(path, &value));
        match set {
            Ok(()) => {
                self.save.touch();
                Done::changed()
            }
            Err(why) => Done {
                dirty: false,
                refused: Some(why),
            },
        }
    }
}
