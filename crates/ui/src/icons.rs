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
    Layout,
    Motion,
    Type,
    Levels,
    Clock,
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
    Open,
    Light,
    Dark,
    Dim,
    Menu,
    Fullscreen,
    ZoomIn,
    ZoomOut,
    FitWidth,
    FitPage,
    SinglePage,
    Spread,
    ScrollVertical,
    ScrollHorizontal,
    ExitFullscreen,
    Keyboard,
    Alert,
    Done,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    Minus,
    Plus,
    Grid,
    List,
    Sort,
    ViewOptions,
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
            Self::Layout => Lucide::LayoutTemplate,
            Self::Motion => Lucide::Wind,
            Self::Type => Lucide::Type,
            Self::Levels => Lucide::ChartNoAxesColumnIncreasing,
            Self::Clock => Lucide::Clock,
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
            Self::Open => Lucide::FolderOpen,
            Self::Light => Lucide::Sun,
            Self::Dark => Lucide::Moon,
            Self::Dim => Lucide::SunMoon,
            Self::Menu => Lucide::Menu,
            Self::Fullscreen => Lucide::Maximize,
            Self::Minus => Lucide::Minus,
            Self::Plus => Lucide::Plus,
            Self::ZoomIn => Lucide::ZoomIn,
            Self::ZoomOut => Lucide::ZoomOut,
            Self::FitWidth => Lucide::MoveHorizontal,
            Self::FitPage => Lucide::Shrink,
            Self::SinglePage => Lucide::RectangleVertical,
            Self::Spread => Lucide::BookOpen,
            Self::ScrollVertical => Lucide::GalleryVertical,
            Self::ScrollHorizontal => Lucide::GalleryHorizontal,
            Self::ExitFullscreen => Lucide::Minimize,
            Self::Keyboard => Lucide::Keyboard,
            Self::Alert => Lucide::CircleAlert,
            Self::Done => Lucide::CircleCheck,
            Self::ChevronDown => Lucide::ChevronDown,
            Self::ChevronLeft => Lucide::ChevronLeft,
            Self::ChevronRight => Lucide::ChevronRight,
            Self::Grid => Lucide::LayoutGrid,
            Self::List => Lucide::List,
            Self::Sort => Lucide::ArrowDownUp,
            Self::ViewOptions => Lucide::SlidersHorizontal,
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
