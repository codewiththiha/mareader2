//! Which page fills the window, and which commands each one takes.

use crate::command::Command;

/// The page in the window: the panes being read, the library, or the
/// dictionary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Route {
    #[default]
    Reader,
    Library,
    Dictionary,
}

impl Route {
    /// Whether `command` means anything here; the rest are dropped.
    pub fn allows(self, command: &Command) -> bool {
        // The dictionary's settings are a click away from its page.
        let settings = matches!(command, Command::ShowSettings | Command::OpenSettings(_));
        self == Route::Reader || !reads(command) || (self == Route::Dictionary && settings)
    }

    /// The rail serves reading; the library has none.
    pub fn has_rail(self) -> bool {
        self == Route::Reader
    }
}

/// Acts on panes, pages or the rail, all hidden behind the library.
fn reads(command: &Command) -> bool {
    use Command as C;
    // Exhaustive, so each new command must say where it belongs.
    match command {
        // The rail and the bar's folding tools act on panes.
        C::ToggleSidebar | C::OpenSidebar(_) | C::CloseSidebar => true,
        C::ShowSearch | C::Split(_) => true,
        // Settings are about reading; their gear lives on the reader's bar.
        C::ShowSettings | C::OpenSettings(_) => true,
        // The focused pane's view and place.
        C::Zoom(_) | C::View(_) | C::Arrow(_) => true,
        C::Page { .. } | C::FirstPage | C::LastPage => true,
        C::AutoScroll | C::Chromeless => true,
        C::GoTo { .. } | C::Turn { .. } => true,
        // The thumbnails copy their picked pages.
        C::CopyPages { .. } => true,
        C::WordList { .. } => true,
        C::FocusPane(_) | C::ClosePane(_) | C::SetRatio(..) => true,
        C::CyclePane { .. } | C::FindStep { .. } => true,
        // The rail's Library panel places books among the panes.
        C::PlaceBook { .. } => true,
        C::SetIndependence(_) | C::SetInkContrast(_) => true,
        // Selecting and explaining act on a pane's text.
        C::Explain(_) | C::Highlight { .. } | C::SelectText { .. } => true,
        C::LookUp(_) | C::Note(_) | C::Bookmark(_) => true,
        // The page AI works on a page in reading.
        C::PageAi(_) => true,
        // Undo of a removal is offered by a toast, which shows anywhere.
        C::Study(_) => false,
        // Downloads go on behind the library; the card closes anywhere.
        C::Dict(_) | C::SetDictionary(_) | C::Vocab(_) => false,
        // The library's own: its page, books and View menu.
        C::ShowLibrary | C::OpenBook(_) | C::SetLibraryView(_) => false,
        // The dictionary comes up from every page.
        C::ShowDictionary => false,
        // Find searches the page in reading, the library on the shelf.
        C::ShowFind => false,
        C::Library(_) => false,
        C::Open(_) | C::ShowOpenDialog(_) | C::ShowViewMenu => false,
        C::ShowShortcuts => false,
        // The window's look, chrome and notices.
        C::ShowAppearance | C::SetAppearance(_) => false,
        C::SavePreset { .. } | C::DeletePreset(_) => false,
        C::SetTitlebarPinned(_) | C::Window(_) => false,
        C::Notify(_) | C::DismissToast => false,
        // A save dialog finishes wherever the window went.
        C::Export { .. } => false,
        // Settings edits are inert while the window is shut.
        C::SetSidebarOverlay(_) | C::CloseSettings | C::SetSettings(_) => false,
        // Key and model work finishes wherever the window went.
        #[cfg(not(target_arch = "wasm32"))]
        C::Ai(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::WindowCommand;
    use crate::layout::ViewMode;

    #[test]
    fn the_library_drops_what_acts_on_panes() {
        let lib = Route::Library;
        assert!(!lib.allows(&Command::ToggleSidebar));
        assert!(!lib.allows(&Command::ShowSettings));
        assert!(!lib.allows(&Command::View(ViewMode::Single)));
        assert!(!lib.allows(&Command::FirstPage));
        assert!(lib.allows(&Command::ShowLibrary));
        assert!(lib.allows(&Command::ShowViewMenu));
        assert!(lib.allows(&Command::Window(WindowCommand::Close)));
        assert!(Route::Reader.allows(&Command::ToggleSidebar));
    }

    #[test]
    fn the_dictionary_keeps_its_settings_and_drops_reading() {
        let page = Route::Dictionary;
        assert!(page.allows(&Command::ShowSettings));
        assert!(page.allows(&Command::ShowDictionary));
        assert!(page.allows(&Command::ShowLibrary));
        assert!(!page.allows(&Command::ToggleSidebar));
        assert!(!page.allows(&Command::Bookmark(None)));
        assert!(!page.has_rail());
    }
}
