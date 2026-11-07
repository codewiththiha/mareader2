//! Semantic Lucide icons with optional generated animation.

pub(crate) mod anim;
mod diagnostics;
/// The UI-owned semantic `Icon` to generated pack-id adapter.
mod pack;
/// Generated one-frame resting glyphs.
mod rest;

use egui::{Color32, Image, Rect, Ui};

/// Lucide's grid: a glyph is drawn inside this square.
pub const GRID: f32 = mareader_animated_icons::GRID;
/// Authored animation headroom beyond each grid edge.
pub const PAD: f32 = mareader_animated_icons::PAD;

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

// Voice-engine marks use the Lucide grid and stroke.

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

/// The embedded mark for an engine id.
fn engine_file(id: &str) -> &'static str {
    ENGINE_FILES
        .into_iter()
        .find(|entry| entry.0 == id)
        .map_or("", |entry| entry.1)
}

impl Icon {
    /// Resolves an engine id to its mark.
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

    /// A custom mark: its loader URI and white SVG file.
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
    pub fn image(self, ui: &Ui, size: f32) -> Image<'static> {
        if let Some((uri, file)) = self.svg() {
            let source = egui::ImageSource::Bytes {
                uri: uri.into(),
                bytes: egui::load::Bytes::Static(file.as_bytes()),
            };
            return Image::new(source)
                .fit_to_exact_size(egui::Vec2::splat(size))
                .show_loading_spinner(false);
        }
        let Some(pack) = pack::of(self) else {
            return rest::blank(ui.ctx(), size);
        };
        rest::image(ui, pack, size)
    }

    /// For custom-painted widgets.
    pub fn paint(self, ui: &Ui, rect: Rect, color: Color32) {
        self.image(ui, rect.width()).tint(color).paint_at(ui, rect);
    }

    /// Stable glyph identity for transient animation keys.
    fn slot(self) -> u32 {
        self as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a tintable engine mark in the shared icon shell.
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
