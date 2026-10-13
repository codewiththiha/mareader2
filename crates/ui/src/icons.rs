//! Lucide icons by meaning, drawn white and tinted: egui-lucide caches
//! per colour.

use egui::{Color32, Image, Rect, Ui};
use egui_lucide::Lucide;

pub const STROKE: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Sidebar,
    SidebarClose,
    Thumbnails,
    Outline,
    Dictionary,
    Library,
    Search,
    Appearance,
    Settings,
    Pin,
    SplitRight,
    SplitDown,
    Close,
    WindowMinimize,
    WindowMaximize,
    WindowRestore,
    Book,
    Light,
    Dark,
    Dim,
}

impl Icon {
    pub fn lucide(self) -> Lucide {
        match self {
            Self::Sidebar => Lucide::PanelLeft,
            Self::SidebarClose => Lucide::PanelLeftClose,
            Self::Thumbnails => Lucide::LayoutGrid,
            Self::Outline => Lucide::ListTree,
            Self::Dictionary => Lucide::BookA,
            Self::Library => Lucide::LibraryBig,
            Self::Search => Lucide::Search,
            Self::Appearance => Lucide::Palette,
            Self::Settings => Lucide::Settings,
            Self::Pin => Lucide::Pin,
            Self::SplitRight => Lucide::Columns2,
            Self::SplitDown => Lucide::Rows2,
            Self::Close => Lucide::X,
            Self::WindowMinimize => Lucide::Minus,
            Self::WindowMaximize => Lucide::Square,
            Self::WindowRestore => Lucide::Copy,
            Self::Book => Lucide::BookOpen,
            Self::Light => Lucide::Sun,
            Self::Dark => Lucide::Moon,
            Self::Dim => Lucide::SunMoon,
        }
    }

    /// For egui widgets; tint it, or let a `Button` tint it.
    pub fn image(self, size: f32) -> Image<'static> {
        self.lucide().size(size).stroke_width(STROKE).image()
    }

    /// For custom-painted widgets.
    pub fn paint(self, ui: &Ui, rect: Rect, color: Color32) {
        self.image(rect.width()).tint(color).paint_at(ui, rect);
    }
}
