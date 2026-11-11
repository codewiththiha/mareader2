//! Which page fills the window, and which commands each one takes.

use mareader_settings::Bar;

use crate::Command;

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
