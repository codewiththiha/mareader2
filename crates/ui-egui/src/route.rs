//! Which page fills the window, and which commands each one takes.

use crate::command::Command;

/// The page in the window: the panes being read, the library, or the
/// dictionary.
pub use mareader_engine::route::Route;

/// Which commands each page takes: chrome's view of `Route`.
pub trait Allows {
    /// Whether `command` means anything here; the rest are dropped.
    fn allows(self, command: &Command) -> bool;
}

impl Allows for Route {
    fn allows(self, command: &Command) -> bool {
        self == Route::Reader || !reads(command)
    }
}

/// Acts on panes, pages or the rail, all hidden behind the library.
fn reads(command: &Command) -> bool {
    use crate::command::UiCommand as U;
    use mareader_engine::command::Intent as I;
    // Exhaustive, so each new command must say where it belongs.
    match command {
        Command::Ui(ui) => matches!(
            ui,
            // The rail and the bar's folding tools act on panes.
            U::ToggleSidebar | U::OpenSidebar(_) | U::CloseSidebar | U::ShowSearch
                | U::Chromeless | U::FindStep { .. }
                // Settings, menus and pages open over any page.
                | U::ShowSettings | U::ShowShortcuts | U::ShowAppearance | U::ShowViewMenu
                | U::ShowLibrary | U::ShowReading | U::ShowDictionary | U::ShowFind
                | U::ShowOpenDialog(_) | U::Window(_) | U::CloseSettings
                // Notices show anywhere.
                | U::Notify(_) | U::CloseToast(_) | U::ClearToasts
        ),
        Command::Intent(intent) => !matches!(
            intent,
            // The focused pane's view and place.
            I::Zoom(_) | I::View(_) | I::Arrow(_) | I::Page { .. } | I::FirstPage
                | I::LastPage | I::AutoScroll | I::GoTo { .. } | I::Turn { .. }
                // The thumbnails copy their picked pages.
                | I::CopyPages { .. } | I::WordList { .. } | I::WordListWindow { .. }
                // Panes and their dividers.
                | I::Split(_) | I::FocusPane(_) | I::ClosePane(_) | I::SetRatio(..)
                | I::CyclePane { .. } | I::PlaceBook { .. }
                // These read a pane's pixels or its text.
                | I::SetIndependence(_) | I::SetInkContrast(_) | I::SetQuickText(_)
                | I::Explain(_) | I::Highlight { .. } | I::SelectText { .. }
                | I::LookUp(_) | I::Note(_) | I::Bookmark(_) | I::PageAi(_)
                // Reading aloud follows a book on the page.
                | I::ReadAloud { .. } | I::ToggleReadAloud | I::StopReadAloud
                | I::PlaceReadMarker { .. }
                // Everything else keeps working wherever the window went.
                | I::OpenSettings(_) | I::TryVoice | I::TryVoiceOf(_)
                | I::StopVoiceTry | I::VoiceModel(..) | I::SayWord(_) | I::SayWordWith { .. }
                | I::StopSaying | I::RevealVoice(..) | I::Study(_) | I::Dict(_)
                | I::SetDictionary(_) | I::Vocab(_) | I::Pronunciation(_) | I::OpenBook(_)
                | I::SetLibraryView(_) | I::Library(_) | I::ForgetPositions { .. }
                | I::Open(_) | I::SetAppearance(_) | I::SavePreset { .. }
                | I::DeletePreset(_) | I::SetTitlebarPinned(_) | I::SetSidebarOverlay(_)
                | I::SetSettings(_) | I::Export { .. }
                | I::Ai(_)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Intent, UiCommand, WindowCommand};
    use crate::layout::ViewMode;

    #[test]
    fn the_library_drops_what_acts_on_panes() {
        let lib = Route::Library;
        assert!(!lib.allows(&Command::Ui(UiCommand::ToggleSidebar)));
        assert!(lib.allows(&Command::Ui(UiCommand::ShowSettings)));
        assert!(!lib.allows(&Command::Intent(Intent::View(ViewMode::Single))));
        assert!(!lib.allows(&Command::Intent(Intent::FirstPage)));
        assert!(lib.allows(&Command::Ui(UiCommand::ShowLibrary)));
        assert!(lib.allows(&Command::Ui(UiCommand::ShowViewMenu)));
        assert!(lib.allows(&Command::Ui(UiCommand::Window(WindowCommand::Close))));
        assert!(Route::Reader.allows(&Command::Ui(UiCommand::ToggleSidebar)));
    }

    #[test]
    fn the_dictionary_keeps_its_settings_and_drops_reading() {
        let page = Route::Dictionary;
        assert!(page.allows(&Command::Ui(UiCommand::ShowSettings)));
        assert!(page.allows(&Command::Ui(UiCommand::ShowDictionary)));
        assert!(page.allows(&Command::Ui(UiCommand::ShowLibrary)));
        assert!(!page.allows(&Command::Ui(UiCommand::ToggleSidebar)));
        assert!(!page.allows(&Command::Intent(Intent::Bookmark(None))));
        assert!(!page.has_rail());
    }
}
