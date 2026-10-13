//! Icons by meaning, from Lucide. Drawn white and tinted, so a colour
//! change never re-rasterises.

use egui::{Color32, Rect, Ui};
use egui_lucide::Lucide;

/// Lucide's own default; a theme dial may move it later.
pub const STROKE: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Sidebar,
    Library,
    Open,
    Search,
    Appearance,
    Settings,
    Pin,
    Menu,
    SplitRight,
    SplitDown,
    Close,
    WindowMinimize,
    WindowMaximize,
    WindowRestore,
    Book,
}

impl Icon {
    pub fn lucide(self) -> Lucide {
        match self {
            Self::Sidebar => Lucide::PanelLeft,
            Self::Library => Lucide::LibraryBig,
            Self::Open => Lucide::FolderOpen,
            Self::Search => Lucide::Search,
            Self::Appearance => Lucide::Palette,
            Self::Settings => Lucide::Settings,
            Self::Pin => Lucide::Pin,
            Self::Menu => Lucide::Menu,
            Self::SplitRight => Lucide::Columns2,
            Self::SplitDown => Lucide::Rows2,
            Self::Close => Lucide::X,
            Self::WindowMinimize => Lucide::Minus,
            Self::WindowMaximize => Lucide::Square,
            Self::WindowRestore => Lucide::Copy,
            Self::Book => Lucide::BookOpen,
        }
    }

    /// Paints the icon filling `rect`, tinted `color`.
    pub fn paint(self, ui: &Ui, rect: Rect, color: Color32, stroke: f32) {
        let source = self
            .lucide()
            .color(Color32::WHITE)
            .stroke_width(stroke)
            .source();
        egui::Image::new(source)
            .tint(color)
            .show_loading_spinner(false)
            .paint_at(ui, rect);
    }
}
