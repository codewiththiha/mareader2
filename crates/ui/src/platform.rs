//! Which caption family to draw; overridable so shots cover all three.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Native traffic lights over a full-size content view.
    Mac,
    /// Frameless; 46 pt square caption buttons, close turns red.
    Windows,
    /// Frameless; 24 pt translucent circles (GNOME/Linux).
    Gnome,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Gnome
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "mac" | "macos" => Some(Self::Mac),
            "windows" => Some(Self::Windows),
            "gnome" | "linux" => Some(Self::Gnome),
            _ => None,
        }
    }

    /// The bar row's left padding: mareader's light gutter, or the rest one.
    pub fn leading_gutter(self) -> f32 {
        match self {
            Self::Mac => 88.0,
            Self::Windows | Self::Gnome => 12.0,
        }
    }

    /// True when the app draws caption buttons and resize edges itself.
    pub fn frameless(self) -> bool {
        !matches!(self, Self::Mac)
    }
}
