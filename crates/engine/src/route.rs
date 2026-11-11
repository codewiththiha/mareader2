//! Which page fills the window, and which intents each one takes.

use mareader_settings::Bar;

use crate::command::Intent;

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
    /// The title bar it shows, each with its own pin.
    pub fn bar(self) -> Bar {
        match self {
            Route::Reader => Bar::Reader,
            Route::Library => Bar::Library,
            Route::Dictionary => Bar::Dictionary,
        }
    }

    /// The rail serves reading; the library has none.
    pub fn has_rail(self) -> bool {
        self == Route::Reader
    }
}

impl Route {
    /// Whether `intent` means anything on this page. The rest are dropped.
    ///
    /// Exhaustive over `Intent`, so a variant added above has to say here
    /// which pages it belongs to.
    pub fn takes(self, intent: &Intent) -> bool {
        self == Route::Reader || !self.reads(intent)
    }

    /// Whether `intent` acts on what only the reader shows: panes, pages,
    /// a book being read, or the pixels of one.
    ///
    /// Exhaustive over `Intent`, so a variant added above has to say here
    /// which pages it belongs to.
    fn reads(self, intent: &Intent) -> bool {
        matches!(
            intent,
            Intent::Zoom(_)
                | Intent::View(_)
                | Intent::Arrow(_)
                | Intent::Page { .. }
                | Intent::FirstPage
                | Intent::LastPage
                | Intent::AutoScroll
                | Intent::GoTo { .. }
                | Intent::Turn { .. }
                | Intent::CopyPages { .. }
                | Intent::WordList { .. }
                | Intent::WordListWindow { .. }
                | Intent::Split(_)
                | Intent::FocusPane(_)
                | Intent::ClosePane(_)
                | Intent::SetRatio(..)
                | Intent::CyclePane { .. }
                | Intent::PlaceBook { .. }
                | Intent::SetIndependence(_)
                | Intent::SetInkContrast(_)
                | Intent::SetQuickText(_)
                | Intent::Explain(_)
                | Intent::Highlight { .. }
                | Intent::SelectText { .. }
                | Intent::LookUp(_)
                | Intent::Note(_)
                | Intent::Bookmark(_)
                | Intent::PageAi(_)
                | Intent::ReadAloud { .. }
                | Intent::ToggleReadAloud
                | Intent::StopReadAloud
                | Intent::PlaceReadMarker { .. }
        ) || is_ai(intent)
    }
}

/// Background AI work, which only a desktop build has: the variant itself is
/// cfg-gated, so it cannot be named in the match above without breaking wasm.
#[cfg(target_arch = "wasm32")]
fn is_ai(_: &Intent) -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn is_ai(intent: &Intent) -> bool {
    matches!(intent, Intent::Ai(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_drops_what_acts_on_panes() {
        assert!(!Route::Library.takes(&Intent::View(crate::pane::ViewMode::Single)));
        assert!(!Route::Library.takes(&Intent::FirstPage));
        assert!(Route::Library.takes(&Intent::OpenSettings(crate::settings::Tab::Panes)));
        assert!(Route::Reader.takes(&Intent::FirstPage));
    }

    #[test]
    fn the_dictionary_keeps_its_settings_and_drops_reading() {
        assert!(Route::Dictionary.takes(&Intent::Bookmark(None)) == false);
        assert!(Route::Dictionary.takes(&Intent::OpenBook(mareader_library::BookId(1))));
        assert!(!Route::Dictionary.has_rail());
    }
}
