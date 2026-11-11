//! Which commands each page takes: the chrome half, where the engine's
//! `Route::takes` answers for intents.

use crate::command::{Command, UiCommand};

pub use mareader_engine::route::Route;

/// Which commands each page takes: chrome's view of `Route`.
pub trait Allows {
    /// Whether `command` means anything here; the rest are dropped.
    fn allows(self, command: &Command) -> bool;
}

impl Allows for Route {
    fn allows(self, command: &Command) -> bool {
        match command {
            Command::Intent(intent) => self.takes(intent),
            Command::Ui(ui) => self == Route::Reader || !chrome_reads(ui),
        }
    }
}

fn chrome_reads(ui: &UiCommand) -> bool {
    use UiCommand as U;
    // Exhaustive, so each new UI command says where it belongs.
    match ui {
        // The rail and the bar's folding tools act on panes.
        U::ToggleSidebar
        | U::OpenSidebar(_)
        | U::CloseSidebar
        | U::ShowSearch
        | U::Chromeless
        | U::FindStep { .. } => true,
        // Settings, menus and pages open over any page.
        U::ShowSettings
        | U::ShowShortcuts
        | U::ShowAppearance
        | U::ShowViewMenu
        | U::ShowLibrary
        | U::ShowReading
        | U::ShowDictionary
        | U::ShowFind
        | U::ShowOpenDialog(_)
        | U::Window(_)
        | U::CloseSettings => false,
        // Notices show anywhere.
        U::Notify(_) | U::CloseToast(_) | U::ClearToasts => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Intent, WindowCommand};
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
