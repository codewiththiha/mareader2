//! The app's state, and the one door that changes it.
//!
//! `Session` owns what outlives a frame and is not a drawing: the settings,
//! the pane tree and which pane is awake, the panes by last focus, and the
//! route. A front end draws that state and asks for changes by handing an
//! [`Intent`] to `Session::execute` (`execute.rs`); what the session cannot
//! show itself leaves as an [`Event`], drained once a frame.
//!
//! Documents, the library and the study tools are still run by front-end
//! controllers, reached through [`crate::desk::Desk`] until they move here.

use std::sync::Arc;

use mareader_settings::Settings;
use mareader_workspace::{Bounds, Edge, MAX_PANES, PaneId, PaneTree, TreeError};

use crate::command::Intent;
use crate::event::Event;
use crate::route::Route;
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
}

/// What an intent the session carries out alone came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Done {
    /// True when the state changed and the window should redraw.
    pub dirty: bool,
    /// Why an intent was refused, if it was: a bad path, a bad value.
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
    pub fn new(settings: Settings) -> Self {
        let tree = PaneTree::new();
        Self {
            settings,
            active: tree.panes().first().copied().unwrap_or(PaneId(1)),
            tree,
            recent: Vec::new(),
            route: Route::Reader,
            dict_from: None,
            events: Vec::new(),
            waker: Arc::new(|| {}),
        }
    }

    /// Who to ask for a frame; until set, a wake does nothing.
    pub fn set_waker(&mut self, waker: Waker) {
        self.waker = waker;
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

    /// Carries out an intent that needs no controller: panes and dials.
    ///
    /// `execute` routes those here; the rest answer quiet, untouched.
    pub fn apply(&mut self, intent: Intent) -> Done {
        match intent {
            Intent::FocusPane(id) => {
                self.focus(id);
                Done::changed()
            }
            Intent::ClosePane(id) => match self.close(id) {
                true => Done::changed(),
                false => Done::quiet(),
            },
            Intent::SetRatio(path, ratio) => {
                self.tree.set_ratio(&path, ratio);
                Done::changed()
            }
            Intent::SetSettings(settings) => {
                let mut settings = *settings;
                settings.sanitize();
                self.settings = settings;
                Done::changed()
            }
            Intent::SetField { path, value } => self.put(&path, Ok(value)),
            other => self.dial(other),
        }
    }

    /// The dials an intent names outright, each written as its path is.
    fn dial(&mut self, intent: Intent) -> Done {
        match intent {
            Intent::SetSidebarOverlay(on) => {
                self.settings.chrome.sidebar_overlay = on;
                Done::changed()
            }
            Intent::SetTitlebarPinned(on) => {
                let bar = self.route.bar();
                self.settings.chrome.pin(bar, on);
                Done::changed()
            }
            Intent::SetInkContrast(ink) => {
                self.put("text.ink_contrast", serde_json::to_string(&ink))
            }
            Intent::SetQuickText(quick) => {
                let mut text = self.settings.text.clone();
                quick.apply(&mut text);
                self.put("text", serde_json::to_string(&text))
            }
            Intent::SetIndependence(own) => {
                let themes = serde_json::to_string(&own.themes);
                self.put("workspace.independent_themes", themes);
                let textures = serde_json::to_string(&own.textures);
                self.put("workspace.independent_textures", textures)
            }
            Intent::SavePreset { name, group, look } => {
                self.settings.save_preset(&name, &group, look);
                self.settings.sanitize();
                Done::changed()
            }
            Intent::DeletePreset(id) => {
                self.settings.delete_preset(&id);
                self.settings.sanitize();
                Done::changed()
            }
            Intent::SetDictionary(dictionary) => {
                self.put("dictionary", serde_json::to_string(&*dictionary))
            }
            Intent::SetLibraryView(view) => self.put("library", serde_json::to_string(&view)),
            _ => Done::quiet(),
        }
    }

    /// One setting by its dotted path; a bad path or value is refused.
    fn put(&mut self, path: &str, json: serde_json::Result<String>) -> Done {
        let set = json
            .map_err(|e| e.to_string())
            .and_then(|value| self.settings.set_path(path, &value));
        match set {
            Ok(()) => Done::changed(),
            Err(why) => Done {
                dirty: false,
                refused: Some(why),
            },
        }
    }
}
