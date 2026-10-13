//! User intents. Views push them; the app applies them after the UI pass.

use mareader_workspace::{PaneId, SplitAxis};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    ToggleSidebar,
    ShowLibrary,
    ShowSearch,
    ShowAppearance,
    ShowSettings,
    Split(SplitAxis),
    FocusPane(PaneId),
    ClosePane(PaneId),
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
