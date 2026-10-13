//! User intents. Views push them; the app applies them after the UI pass.

use mareader_appearance::Appearance;
use mareader_workspace::{DividerPath, PaneId, SplitAxis};

use crate::app::Settings;
use crate::settings_window::Tab;
use crate::sidebar::Panel;

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
    /// Opens the settings window where it was left.
    ShowSettings,
    OpenSettings(Tab),
    CloseSettings,
    /// Every edit in the settings window; applies on the same frame.
    SetSettings(Box<Settings>),
    Split(SplitAxis),
    FocusPane(PaneId),
    ClosePane(PaneId),
    /// A divider drag; sent every frame it moves.
    SetRatio(DividerPath, f32),
    /// Every tick of an appearance control; restyles on the same frame.
    SetAppearance(Appearance),
    SetTitlebarPinned(bool),
    Window(WindowCommand),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    Minimize,
    ToggleMaximize,
    Close,
    StartDrag,
}

/// The frame's command queue, handed to every view.
pub type Commands = Vec<Command>;
