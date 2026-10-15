//! Which page fills the window, and which commands each one takes.

use crate::command::Command;

/// The page in the window: the panes being read, or the library.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Route {
    #[default]
    Reader,
    Library,
}

impl Route {
    /// Whether `command` means anything here; the rest are dropped.
    pub fn allows(self, command: &Command) -> bool {
        self == Route::Reader || !reads(command)
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
        C::GoTo { .. } | C::Turn { .. } => true,
        C::FocusPane(_) | C::ClosePane(_) | C::SetRatio(..) => true,
        C::SetIndependence(_) | C::SetInkContrast(_) => true,
        // The library's own: its page, books and View menu.
        C::ShowLibrary | C::OpenBook(_) | C::SetLibraryView(_) => false,
        // Find searches the page in reading, the library on the shelf.
        C::ShowFind => false,
        C::Library(_) => false,
        C::Open(_) | C::ShowOpenDialog { .. } | C::ShowViewMenu => false,
        // The window's look, chrome and notices.
        C::ShowAppearance | C::SetAppearance(_) => false,
        C::SavePreset { .. } | C::DeletePreset(_) => false,
        C::SetTitlebarPinned(_) | C::Window(_) => false,
        C::Notify(_) | C::DismissToast => false,
        // Settings edits are inert while the window is shut.
        C::SetSidebarOverlay(_) | C::CloseSettings | C::SetSettings(_) => false,
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
}
