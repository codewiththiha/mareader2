//! Lucide icons by meaning, drawn white and tinted: egui-lucide caches
//! per colour.

use egui::{Color32, Image, Rect, Ui};
use egui_lucide::Lucide;

pub const STROKE: f32 = 2.0;

/// "AI" in strokes inside a rounded frame, on Lucide's 24-unit grid.
const AI_MARK: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" "#,
    r#"viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" "#,
    r#"stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<rect x="2.5" y="4.5" width="19" height="15" rx="4"/>"#,
    r#"<path d="M6.5 16 9.25 8 12 16"/><path d="M7.6 13.2h3.3"/>"#,
    r#"<path d="M16.5 8v8"/><path d="M15 8h3"/><path d="M15 16h3"/>"#,
    "</svg>"
);

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
    Close,
    WindowMinimize,
    WindowMaximize,
    WindowRestore,
    Book,
    Open,
    Light,
    Dark,
    Dim,
    Black,
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
    Chromeless,
    Keyboard,
    Alert,
    Done,
    ChevronDown,
    ChevronUp,
    ChevronLeft,
    ChevronRight,
    Minus,
    Plus,
    Grid,
    List,
    Sort,
    ViewOptions,
    Folders,
    Folder,
    FolderPlus,
    Rescan,
    SearchFolder,
    ChooseFile,
    Offline,
    Remove,
    Shelf,
    Rename,
    Check,
    Import,
    Link,
    Restore,
    More,
    Copy,
    /// Anything the AI does: "AI" drawn as an icon, not a Lucide glyph.
    Ai,
    /// The AI's models: a processor chip, the AI mark's pair.
    Chip,
    /// A custom server's badge: it has no logo of its own.
    Server,
    /// Providers' own logos, from `assets/logos`.
    OpenAi,
    Gemini,
    OpenRouter,
    Ollama,
    LmStudio,
    Apple,
    /// Try a model once.
    Try,
    /// Say a word aloud.
    Speak,
    /// The full highlighter.
    Highlight,
    /// The half highlighter: a letter on a bar.
    HalfHighlight,
    Underline,
    MoveUp,
    MoveDown,
    /// An API key.
    Key,
    /// Sets a key aside.
    Ban,
    Swap,
    Download,
    Pause,
    Resume,
    LookUp,
    SelectSeveral,
    AutoScroll,
    /// A highlight's note, and the panel that gathers them.
    Note,
    Bookmark,
    /// The page AI's tasks.
    Summarize,
    Translate,
    Simplify,
    Stop,
    Show,
    /// Continuous mode: on as one reads.
    Repeat,
    /// Read-aloud: its bar, steps and the way out of a table.
    ReadAloud,
    /// A downloaded voice model's own settings tab.
    VoiceModel,
    SkipBack,
    SkipForward,
    /// Read-aloud's options: page numbers skipped, pages turned.
    PageNumber,
    Follow,
    LeaveTable,
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
            Self::Close => Lucide::X,
            Self::WindowMinimize => Lucide::Minus,
            Self::WindowMaximize => Lucide::Square,
            Self::WindowRestore => Lucide::Copy,
            Self::Book => Lucide::BookOpen,
            Self::Open => Lucide::FolderOpen,
            Self::Light => Lucide::Sun,
            Self::Dark => Lucide::Moon,
            Self::Dim => Lucide::SunMoon,
            Self::Black => Lucide::Eclipse,
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
            Self::Chromeless => Lucide::EyeOff,
            Self::Keyboard => Lucide::Keyboard,
            Self::Alert => Lucide::CircleAlert,
            Self::Done => Lucide::CircleCheck,
            Self::ChevronDown => Lucide::ChevronDown,
            Self::ChevronUp => Lucide::ChevronUp,
            Self::ChevronLeft => Lucide::ChevronLeft,
            Self::ChevronRight => Lucide::ChevronRight,
            Self::Grid => Lucide::LayoutGrid,
            Self::List => Lucide::List,
            Self::Sort => Lucide::ArrowDownUp,
            Self::ViewOptions => Lucide::SlidersHorizontal,
            Self::AutoScroll => Lucide::ArrowDown,
            Self::Folders => Lucide::Folders,
            Self::Folder => Lucide::Folder,
            Self::FolderPlus => Lucide::FolderPlus,
            Self::Rescan => Lucide::RefreshCw,
            Self::SearchFolder => Lucide::FolderSearch,
            Self::ChooseFile => Lucide::FileSearch,
            Self::Offline => Lucide::HardDrive,
            Self::Remove => Lucide::Trash2,
            Self::Shelf => Lucide::SquareLibrary,
            Self::Key => Lucide::KeyRound,
            Self::Ban => Lucide::Ban,
            Self::Swap => Lucide::ArrowRightLeft,
            Self::Download => Lucide::Download,
            Self::Pause => Lucide::Pause,
            Self::Resume => Lucide::Play,
            Self::LookUp => Lucide::BookSearch,
            Self::SelectSeveral => Lucide::ListChecks,
            Self::Rename => Lucide::Pencil,
            Self::Note => Lucide::StickyNote,
            Self::Bookmark => Lucide::Bookmark,
            Self::Summarize => Lucide::ScrollText,
            Self::Translate => Lucide::Languages,
            Self::Simplify => Lucide::Feather,
            Self::Stop => Lucide::CircleStop,
            Self::Show => Lucide::Eye,
            Self::Repeat => Lucide::Repeat,
            Self::ReadAloud => Lucide::Headphones,
            Self::VoiceModel => Lucide::AudioWaveform,
            Self::SkipBack => Lucide::SkipBack,
            Self::SkipForward => Lucide::SkipForward,
            Self::LeaveTable => Lucide::ArrowRightFromLine,
            Self::PageNumber => Lucide::Hash,
            Self::Follow => Lucide::BookOpenText,
            Self::Check => Lucide::Check,
            Self::Import => Lucide::BookPlus,
            Self::Link => Lucide::Link,
            Self::Restore => Lucide::Undo2,
            Self::More => Lucide::Ellipsis,
            Self::Copy => Lucide::Copy,
            // Not drawn from it: see `svg`.
            Self::Ai
            | Self::OpenAi
            | Self::Gemini
            | Self::OpenRouter
            | Self::Ollama
            | Self::LmStudio
            | Self::Apple => Lucide::Square,
            Self::Chip => Lucide::Cpu,
            Self::Server => Lucide::Server,
            Self::Try => Lucide::Play,
            Self::Speak => Lucide::Volume2,
            Self::Highlight => Lucide::Highlighter,
            Self::HalfHighlight => Lucide::Baseline,
            Self::Underline => Lucide::Underline,
            Self::MoveUp => Lucide::ArrowUp,
            Self::MoveDown => Lucide::ArrowDown,
        }
    }

    /// Ours, not Lucide's: its loader uri and white SVG file.
    fn svg(self) -> Option<(&'static str, &'static str)> {
        let logo = |name, file| Some((name, file));
        match self {
            Self::Ai => logo("bytes://mareader/ai-mark.svg", AI_MARK),
            Self::OpenAi => logo(
                "bytes://mareader/logo-openai.svg",
                include_str!("../assets/logos/openai.svg"),
            ),
            Self::Gemini => logo(
                "bytes://mareader/logo-gemini.svg",
                include_str!("../assets/logos/gemini.svg"),
            ),
            Self::OpenRouter => logo(
                "bytes://mareader/logo-openrouter.svg",
                include_str!("../assets/logos/openrouter.svg"),
            ),
            Self::Ollama => logo(
                "bytes://mareader/logo-ollama.svg",
                include_str!("../assets/logos/ollama.svg"),
            ),
            Self::LmStudio => logo(
                "bytes://mareader/logo-lm-studio.svg",
                include_str!("../assets/logos/lm-studio.svg"),
            ),
            Self::Apple => logo(
                "bytes://mareader/logo-apple.svg",
                include_str!("../assets/logos/apple.svg"),
            ),
            _ => None,
        }
    }

    /// For egui widgets; tint it, or let a `Button` tint it.
    pub fn image(self, size: f32) -> Image<'static> {
        if let Some((uri, file)) = self.svg() {
            let source = egui::ImageSource::Bytes {
                uri: uri.into(),
                bytes: egui::load::Bytes::Static(file.as_bytes()),
            };
            return Image::new(source)
                .fit_to_exact_size(egui::Vec2::splat(size))
                .show_loading_spinner(false);
        }
        self.lucide().size(size).stroke_width(STROKE).image()
    }

    /// For custom-painted widgets.
    pub fn paint(self, ui: &Ui, rect: Rect, color: Color32) {
        self.image(rect.width()).tint(color).paint_at(ui, rect);
    }
}
