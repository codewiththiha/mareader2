//! User intents. Views push them; the app applies them after the UI pass.

use mareader_appearance::Appearance;
use mareader_workspace::{DividerPath, PaneId, SplitAxis};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    ToggleSidebar,
    ShowLibrary,
    ShowSearch,
    ShowAppearance,
    ShowSettings,
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
