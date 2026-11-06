//! Lucide icons by meaning, drawn white and tinted: egui-lucide caches
//! per colour - and, for the glyphs `pack` drew, drawn a frame at a time so a
//! glyph can move: see `anim`.

pub(crate) mod anim;
/// The generated table. Its numbers are measurements four decimals deep, and a
/// measurement that happens to begin `1.4142` is not a claim about the square
/// root of two: the lint that reads it as one is asked off. So is the one that
/// counts digits past what an `f32` would keep, since a frame is written to
/// three decimals and the table keeps one more than that in hand. Re-port the
/// pack with its numbers rounded this way and the lint asks of it nothing else,
/// and run the formatter over the file after: it is written once and read
/// rarely, but it is read.
#[allow(clippy::approx_constant, clippy::excessive_precision)]
mod pack;

use egui::{Color32, Image, Rect, Ui};
use egui_lucide::Lucide;

pub const STROKE: f32 = 2.0;
/// Lucide's grid: a glyph is drawn inside this square.
pub const GRID: f32 = 24.0;
/// How far past the grid a moving frame may paint, in grid units. The lab's
/// keyframes scale a glyph up and slide its parts out of the box it was drawn
/// in, and a frame is clipped to its own canvas, so the canvas keeps this much
/// slack on every side rather than losing the ink at the edge.
pub const PAD: f32 = 8.5;

/// Where a frame writes a part's attributes into a pack's markup.
pub(crate) const HOLE: char = '\u{1}';
/// The same, for the stroke width an element carries itself.
pub(crate) const WIDTH_HOLE: char = '\u{2}';

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

// The voices' own marks, one per engine in `crates/speech`, in
// `assets/engines`. Each is drawn from what the engine is - its name, its size,
// its physics - so a row of engines reads as six different voices, and each is
// drawn on Lucide's grid at stroke 2 with round caps so it tints, and sits
// beside a stock glyph, like one.

/// The engines' files, one per id `Icon::engine` knows.
const ENGINE_FILES: [(&str, &str); 6] = [
    ("kokoro", include_str!("../../assets/engines/kokoro.svg")),
    ("piper", include_str!("../../assets/engines/piper.svg")),
    ("kitten", include_str!("../../assets/engines/kitten.svg")),
    ("phonon", include_str!("../../assets/engines/phonon.svg")),
    ("system", include_str!("../../assets/engines/system.svg")),
    ("silent", include_str!("../../assets/engines/silent.svg")),
];

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
    /// The voices themselves, one mark per engine: `Icon::engine` picks them.
    Kokoro,
    Piper,
    Kitten,
    Phonon,
    SystemVoice,
    SilentEngine,
    SkipBack,
    SkipForward,
    /// Read-aloud's options: page numbers skipped, pages turned.
    PageNumber,
    Follow,
    LeaveTable,
    Table,
    Figure,
    /// A sidebar or boxed aside beside the main text.
    Aside,
}

/// An engine's own file, by the id `Icon::engine` maps from. The two tables
/// stay in step because `tests` reads every engine id through this.
fn engine_file(id: &str) -> &'static str {
    ENGINE_FILES
        .into_iter()
        .find(|entry| entry.0 == id)
        .map_or("", |entry| entry.1)
}

impl Icon {
    /// The mark an engine owns, by its id in `crates/speech`. None for one with
    /// no mark of its own, where the stock waveform stands in.
    pub fn engine(id: &str) -> Option<Icon> {
        match id {
            "kokoro" => Some(Self::Kokoro),
            "piper" => Some(Self::Piper),
            "kitten" => Some(Self::Kitten),
            "phonon" => Some(Self::Phonon),
            mareader_speech::SYSTEM => Some(Self::SystemVoice),
            mareader_speech::SILENT => Some(Self::SilentEngine),
            _ => None,
        }
    }

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
            Self::Table => Lucide::Table,
            Self::Figure => Lucide::Image,
            Self::Aside => Lucide::PanelRight,
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
            | Self::Kokoro
            | Self::Piper
            | Self::Kitten
            | Self::Phonon
            | Self::SystemVoice
            | Self::SilentEngine
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
            Self::Kokoro => logo("bytes://mareader/engine-kokoro.svg", engine_file("kokoro")),
            Self::Piper => logo("bytes://mareader/engine-piper.svg", engine_file("piper")),
            Self::Kitten => logo("bytes://mareader/engine-kitten.svg", engine_file("kitten")),
            Self::Phonon => logo("bytes://mareader/engine-phonon.svg", engine_file("phonon")),
            Self::SystemVoice => logo("bytes://mareader/engine-system.svg", engine_file("system")),
            Self::SilentEngine => logo("bytes://mareader/engine-silent.svg", engine_file("silent")),
            Self::OpenAi => logo(
                "bytes://mareader/logo-openai.svg",
                include_str!("../../assets/logos/openai.svg"),
            ),
            Self::Gemini => logo(
                "bytes://mareader/logo-gemini.svg",
                include_str!("../../assets/logos/gemini.svg"),
            ),
            Self::OpenRouter => logo(
                "bytes://mareader/logo-openrouter.svg",
                include_str!("../../assets/logos/openrouter.svg"),
            ),
            Self::Ollama => logo(
                "bytes://mareader/logo-ollama.svg",
                include_str!("../../assets/logos/ollama.svg"),
            ),
            Self::LmStudio => logo(
                "bytes://mareader/logo-lm-studio.svg",
                include_str!("../../assets/logos/lm-studio.svg"),
            ),
            Self::Apple => logo(
                "bytes://mareader/logo-apple.svg",
                include_str!("../../assets/logos/apple.svg"),
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

    /// A small stable number for this glyph, for keys that must not change when
    /// the enum is reordered around it: `anim` keeps one run per icon per widget.
    fn slot(self) -> u32 {
        self as u32
    }
}

/* ------------------------------------------------------------------------- */
/* What a generated glyph is, and what a frame does to it                    */
/* ------------------------------------------------------------------------- */

/// One movable piece of a [Pack]'s glyph: what a part of it weighs, where it
/// hangs from, and how far its ink runs.
pub(crate) struct Part {
    /// The lab's name for it, as its markup tagged it. Read by nobody at
    /// runtime: it is what makes a row of the table and a line of the markup
    /// the same part, to anyone comparing the two.
    #[allow(dead_code)]
    pub name: &'static str,
    /// The pivot of any turn or stretch: the middle of the part's own ink, in
    /// the glyph's 24-unit box, which is what a browser pivots a part on.
    pub ox: f32,
    pub oy: f32,
    /// The length of its stroke, so a run that draws it on needs no measuring.
    pub len: f32,
    /// The width it is drawn at, which a run may thin and must give back.
    pub w: f32,
}

/// One mark in a pack's markup, saying which part is being written to and
/// whether it is the element's own stroke width rather than the whole part.
pub(crate) struct Hole {
    pub part: u8,
    pub width: bool,
}

/// What one segment of a timeline animates.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Prop {
    X,
    Y,
    Scale,
    ScaleX,
    ScaleY,
    Rotate,
    SkewX,
    /// No glyph the lab has drawn skews the other way yet; the port maps it
    /// here all the same, so a pack that grows one needs no new plumbing.
    #[allow(dead_code)]
    SkewY,
    Opacity,
    /// How much of a stroke is drawn, as a share of its length.
    PathLen,
    /// Where that share starts, as a share too.
    PathOff,
    StrokeW,
}

/// One property of one part: when it moves, how long it takes, where it goes.
pub(crate) struct Seg {
    pub part: u8,
    pub prop: Prop,
    /// Seconds from the run's start.
    pub t0: f32,
    /// Seconds the move takes. Zero is a snap, and holds either side of it.
    pub dur: f32,
    /// The values, one per `times` share. Two is a move from what came before.
    pub keys: &'static [f32],
    /// Where each key sits within the segment; empty spaces them evenly.
    pub times: &'static [f32],
    /// One easing per gap, into the pack's table; one entry serves them all.
    pub eases: &'static [u8],
}

/// A glyph that can move: its drawing at rest, and the timeline that bends it.
pub(crate) struct Pack {
    /// The lab's name for it, for the frame's own debugging.
    pub id: &'static str,
    /// Seconds the whole run takes. Every run ends where it began, so a glyph
    /// can be left alone once it has played without it showing the difference.
    pub dur: f32,
    pub parts: &'static [Part],
    pub segs: &'static [Seg],
    /// One per mark in `svg`, in the order they appear.
    pub holes: &'static [Hole],
    /// The glyph at rest, with a [`HOLE`] where each part's attributes go.
    pub svg: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every engine the app can speak with leads its row with a mark of its own,
    /// drawn in the same shell as the rest, and no two engines share one.
    /// The shell `AI_MARK` is drawn in: the whole point is that an engine's file
    /// opens with it, so a mark tints and weighs like a Lucide glyph.
    const SHELL: &str = concat!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" "#,
        r#"viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" "#,
        r#"stroke-linecap="round" stroke-linejoin="round">"#,
    );

    #[test]
    fn every_voice_engine_has_its_own_mark() {
        let ids: Vec<&str> = mareader_speech::MODELS
            .iter()
            .map(|m| m.id)
            .chain([mareader_speech::SYSTEM, mareader_speech::SILENT])
            .collect();
        let mut files: Vec<&str> = Vec::new();
        for id in ids {
            let file = Icon::engine(id).and_then(Icon::svg).map_or("", |(_, f)| f);
            assert!(!file.is_empty(), "{id}: no mark drawn");
            assert!(file.starts_with(SHELL), "{id}: not the shared shell");
            assert!(file.trim_end().ends_with("</svg>"), "{id}: svg left open");
            assert!(file.contains("<path"), "{id}: draws nothing");
            assert!(!files.contains(&file), "{id}: a shared mark");
            files.push(file);
        }
        assert_eq!(files.len(), 6, "six engines, six marks");
    }

    /// An engine this build does not know keeps the stock waveform, not a
    /// stranger's face.
    #[test]
    fn an_engine_without_a_mark_of_its_own_has_none() {
        assert!(Icon::engine("some-other-voice").is_none());
    }
}
