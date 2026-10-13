//! User intents. Views push them; the app applies them after the UI pass.

use mareader_appearance::Appearance;
use mareader_workspace::{DividerPath, PaneId, SplitAxis};

use crate::documents::OpenRequest;
use crate::layout::{Arrow, ViewMode};
use crate::settings_window::Tab;
use crate::sidebar::Panel;
use crate::toast::Toast;
use crate::zoom::ZoomCommand;
use mareader_settings::Settings;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    ToggleSidebar,
    /// Opens the rail on a panel, or on the last one shown.
    OpenSidebar(Option<Panel>),
    CloseSidebar,
    /// Floating over the panes, or docked beside them.
    SetSidebarOverlay(bool),
    ShowLibrary,
    ShowSearch,
    /// Opens the Appearance popover (the keymap's route to it).
    ShowAppearance,
    /// Opens the title bar's View & tools menu.
    ShowViewMenu,
    /// Opens the settings window where it was left.
    ShowSettings,
    OpenSettings(Tab),
    CloseSettings,
    /// Every edit in the settings window; applies on the same frame.
    SetSettings(Box<Settings>),
    /// The system open dialog; `beside` opens the pick in a new split.
    ShowOpenDialog {
        beside: bool,
    },
    /// Zooms the active pane's document.
    Zoom(ZoomCommand),
    /// The active pane's view mode, kept on the page read.
    View(ViewMode),
    /// An arrow key over the active document: a turn, or the scroll's.
    Arrow(Arrow),
    /// A page key: nearly a screen along a strip, a turn when paged.
    Page {
        forward: bool,
    },
    /// Home and End: the active document's first or last page.
    FirstPage,
    LastPage,
    /// Shows `page` of the pane's document, from its start.
    GoTo {
        pane: PaneId,
        page: usize,
    },
    /// The previous or next page, or spread, of the pane's document.
    Turn {
        pane: PaneId,
        forward: bool,
    },
    /// Shows a file in a pane, replacing what it showed.
    Open(OpenRequest),
    Split(SplitAxis),
    FocusPane(PaneId),
    ClosePane(PaneId),
    /// A divider drag; sent every frame it moves.
    SetRatio(DividerPath, f32),
    /// Every tick of an appearance control; restyles on the same frame.
    SetAppearance(Appearance),
    SetTitlebarPinned(bool),
    Window(WindowCommand),
    /// Shows a toast, replacing the one on screen.
    Notify(Toast),
    DismissToast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    Minimize,
    ToggleMaximize,
    Close,
    StartDrag,
    ToggleFullscreen,
}

/// The frame's command queue, handed to every view.
pub type Commands = Vec<Command>;
