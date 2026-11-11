//! The app's state, and the one door that changes it.
//!
//! `Session` owns what outlives a frame and is not a drawing: the settings,
//! the pane tree and which pane is awake, the panes by last focus, and the
//! route. A front end draws that state and asks for changes by handing over
//! an [`Intent`]; nothing else may write it.
//!
//! This is the first half of the reducer. The rest of what the review lists
//! as the owner — the documents model, the library, the panes' looks and
//! texts, the staged selection, the last document — still lives in the app,
//! and moves here once it is toolkit-free enough to make the trip.
//!
//! # What the door returns
//!
//! An intent the window must finish (a split needs its area, F6 needs the
//! focus ring) is answered with a [`Focus`], because the engine may not name
//! those things. Everything else is done here, and the caller sees the change
//! in the state it borrowed.

use mareader_settings::Settings;
use mareader_workspace::{Bounds, PaneId, PaneTree, Side, SplitAxis};

use crate::command::Intent;
use crate::route::Route;

/// One of the things a person can do without a window.
pub struct Session {
    settings: Settings,
    tree: PaneTree,
    active: PaneId,
    /// Panes by last focus, latest first: blend's paper follows them.
    recent: Vec<PaneId>,
    route: Route,
    /// Where the Dictionary page was opened from.
    dict_from: Option<Route>,
}

/// What the engine could not do alone, put back to the front end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Focus {
    /// Focus the next or previous pane, in reading order.
    Cycle { forward: bool },
    /// Focus a pane the engine has just made or found.
    Pane(PaneId),
}

/// One door's answer: what the front end must still carry out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Done {
    /// A viewport change, if the intent asked for one.
    pub focus: Option<Focus>,
    /// True when the state changed and the window should redraw.
    pub dirty: bool,
}

impl Done {
    fn quiet() -> Self {
        Self::default()
    }

    fn changed() -> Self {
        Self {
            dirty: true,
            ..Self::default()
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
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The same settings, writable: a view's own dial, not a command.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// The settings window's edits, applied on the frame they arrive.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
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
        self.recent.retain(|p| *p != id);
        self.recent.insert(0, id);
    }

    /// The page the window shows; the library keeps the route it came from.
    pub fn show(&mut self, route: Route) {
        match route {
            Route::Dictionary => {
                self.dict_from = Some(self.route);
                self.route = route;
            }
            _ => {
                self.route = route;
                if route == Route::Reader {
                    self.dict_from = None;
                }
            }
        }
    }

    /// Splits `at` into `side` of `area` and focuses the new pane.
    ///
    /// The geometry is the window's, so it arrives as plain numbers: the
    /// engine keeps the tree and the focus, and nothing else.
    pub fn split(
        &mut self,
        at: PaneId,
        axis: SplitAxis,
        side: Side,
        area: Bounds,
    ) -> Option<PaneId> {
        let made = self.tree.split(at, axis, side, area).ok()?;
        self.focus(made);
        Some(made)
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

    /// The pane after or before `active` in reading order, for F6.
    pub fn neighbour(&self, forward: bool) -> Option<PaneId> {
        let panes = self.tree.panes();
        let at = panes.iter().position(|p| *p == self.active)?;
        let n = panes.len();
        if n == 0 {
            return None;
        }
        let next = if forward {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        };
        panes.get(next).copied()
    }

    /// Whether the rail floats or docks.
    pub fn sidebar_overlay(&self) -> bool {
        self.settings.chrome.sidebar_overlay
    }

    /// The one door: carry out `intent`, and say what the window must still do.
    pub fn execute(&mut self, intent: Intent) -> Done {
        match intent {
            Intent::FocusPane(id) => {
                self.focus(id);
                Done::changed()
            }
            Intent::CyclePane { forward } => {
                let next = self.neighbour(forward);
                match next {
                    Some(id) => {
                        self.focus(id);
                        Done {
                            focus: Some(Focus::Cycle { forward }),
                            dirty: true,
                        }
                    }
                    None => Done::quiet(),
                }
            }
            Intent::ClosePane(id) => match self.close(id) {
                true => Done::changed(),
                false => Done::quiet(),
            },
            Intent::SetSidebarOverlay(on) => {
                self.settings.chrome.sidebar_overlay = on;
                Done::changed()
            }
            Intent::SetTitlebarPinned(on) => {
                let bar = self.route.bar();
                self.settings.chrome.pin(bar, on);
                Done::changed()
            }
            Intent::SetSettings(settings) => {
                let mut settings = *settings;
                settings.sanitize();
                self.settings = settings;
                Done::changed()
            }
            Intent::Zoom(_) | Intent::Arrow(_) => Done::quiet(),
            _ => Done::quiet(),
        }
    }
}
