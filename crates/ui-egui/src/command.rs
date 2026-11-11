//! What a view can ask for: the engine's intents, and chrome.
//!
//! `Intent` is what means something with no window on screen; `UiCommand`
//! only moves what is on it. A view pushes `Command` and the app splits it.

pub use mareader_engine::command::{Independence, Intent};
pub use mareader_engine::dict::Download;

use mareader_engine::documents::Placement;
use mareader_engine::toast::Toast;

use crate::sidebar::Panel;

/// What the window buttons do: chrome, and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    Minimize,
    ToggleMaximize,
    Close,
    StartDrag,
    ToggleFullscreen,
}

/// Everything a view can ask for: the engine's intents, and chrome.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Changes something beyond this window: routed to the engine.
    Intent(Intent),
    /// Moves something on screen only.
    Ui(UiCommand),
}

impl From<Intent> for Command {
    fn from(intent: Intent) -> Self {
        Self::Intent(intent)
    }
}

impl From<UiCommand> for Command {
    fn from(ui: UiCommand) -> Self {
        Self::Ui(ui)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiCommand {
    /// Distraction-free reading: every bar hidden until asked back.
    Chromeless,
    /// Closes every toast.
    ClearToasts,
    CloseSettings,
    CloseSidebar,
    /// Closes one toast, by its stack id.
    CloseToast(u64),
    /// F3 and Cmd+G: the next or previous match, opening the bar if shut.
    FindStep {
        forward: bool,
    },
    /// Shows a toast on top of the stack.
    Notify(Toast),
    /// Opens the rail on a panel, or on the last one shown.
    OpenSidebar(Option<Panel>),
    /// Opens the Appearance popover (the keymap's route to it).
    ShowAppearance,
    /// The Dictionary page, or back to where it was opened from.
    ShowDictionary,
    /// The find bar over the focused document; else the library.
    ShowFind,
    /// The library page in place of the panes, or back to them.
    ShowLibrary,
    /// The system open dialog, the pick placed as asked.
    ShowOpenDialog(Placement),
    /// Back to the pane reading aloud, turned to the words read.
    ShowReading,
    /// The Search buttons: the find bar on, or off if open.
    ShowSearch,
    /// Opens the settings window where it was left.
    ShowSettings,
    /// Opens the keyboard shortcut cheat sheet, or shuts it.
    ShowShortcuts,
    /// Opens the title bar's View & tools menu.
    ShowViewMenu,
    ToggleSidebar,
    Window(WindowCommand),
}


/// The frame's command queue, handed to every view.
pub type Commands = Vec<Command>;
