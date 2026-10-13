//! Which desktop's window conventions to follow; tests can override it.

use crate::widgets::CaptionStyle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Native traffic lights over a full-size content view.
    Mac,
    /// Frameless, Windows-style captions.
    Windows,
    /// Frameless, GNOME-style captions.
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

    /// Room the title bar leaves at its left edge: macOS's traffic lights.
    pub fn leading_inset(self) -> f32 {
        match self {
            Self::Mac => 80.0,
            Self::Windows | Self::Gnome => 8.0,
        }
    }

    /// The caption buttons the app draws; macOS draws its own.
    pub fn captions(self) -> Option<CaptionStyle> {
        match self {
            Self::Mac => None,
            Self::Windows => Some(CaptionStyle::Windows),
            Self::Gnome => Some(CaptionStyle::Gnome),
        }
    }

    /// True when the app draws caption buttons and resize edges itself.
    pub fn frameless(self) -> bool {
        !matches!(self, Self::Mac)
    }
}
