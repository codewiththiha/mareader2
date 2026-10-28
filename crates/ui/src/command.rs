//! User intents. Views push them; the app applies them after the UI pass.

use mareader_appearance::Appearance;
use mareader_library::BookId;
use mareader_workspace::{DividerPath, PaneId, SplitAxis};

use crate::documents::{OpenRequest, Placement};
use crate::layout::{Arrow, ViewMode};
use crate::settings_window::Tab;
use crate::sidebar::Panel;
use crate::toast::Toast;
use crate::zoom::ZoomCommand;
use mareader_settings::Settings;

/// Whether split panes take their own colour, texture, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Independence {
    pub themes: bool,
    pub textures: bool,
}

/// What a download's buttons ask of it; Start also resumes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Download {
    Start,
    Pause,
    Cancel,
    Remove,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    ToggleSidebar,
    /// Opens the rail on a panel, or on the last one shown.
    OpenSidebar(Option<Panel>),
    CloseSidebar,
    /// Floating over the panes, or docked beside them.
    SetSidebarOverlay(bool),
    /// The library page in place of the panes, or back to them.
    ShowLibrary,
    /// Back to the pane reading aloud, turned to the words read.
    ShowReading,
    /// The Dictionary page, or back to where it was opened from.
    ShowDictionary,
    /// Opens a library book as Settings › Workspace says, or focuses it.
    OpenBook(BookId),
    /// A library book placed from the rail: clicked, or dropped on a pane.
    PlaceBook {
        book: BookId,
        pane: Option<PaneId>,
        placement: Placement,
    },
    /// The library page's layout and order.
    SetLibraryView(mareader_library::View),
    /// Folders, scopes and finding lost books.
    Library(crate::library::Action),
    /// The Search buttons: the find bar on, or off if open.
    ShowSearch,
    /// Opens the Appearance popover (the keymap's route to it).
    ShowAppearance,
    /// Opens the title bar's View & tools menu.
    ShowViewMenu,
    /// Opens the settings window where it was left.
    ShowSettings,
    /// Opens the keyboard shortcut cheat sheet, or shuts it.
    ShowShortcuts,
    /// The find bar over the focused document; else the library.
    ShowFind,
    /// F3 and Cmd+G: the next or previous match, opening the bar if shut.
    FindStep {
        forward: bool,
    },
    OpenSettings(Tab),
    CloseSettings,
    /// Every edit in the settings window; applies on the same frame.
    SetSettings(Box<Settings>),
    /// The system open dialog, the pick placed as asked.
    ShowOpenDialog(Placement),
    /// Zooms the active pane's document.
    Zoom(ZoomCommand),
    /// The active pane's view mode, kept on the page read.
    View(ViewMode),
    /// An arrow key over the active document: a turn, or the scroll's.
    Arrow(Arrow),
    /// A page key: nearly a screen along a strip, a turn when paged.
    Page {
        forward: bool,
    },
    /// Shift+A: the active pane's auto-scroll on or off.
    AutoScroll,
    /// Distraction-free reading: every bar hidden until asked back.
    Chromeless,
    /// Home and End: the active document's first or last page.
    FirstPage,
    LastPage,
    /// Shows `page` of the pane's document, from its start.
    GoTo {
        pane: PaneId,
        page: usize,
    },
    /// Puts the text of the pane's `pages` on the clipboard.
    CopyPages {
        pane: PaneId,
        pages: Vec<usize>,
    },
    /// Copies a word list of the pane's `pages`, with the instant options.
    WordList {
        pane: PaneId,
        pages: Vec<usize>,
    },
    /// Opens the word list window on the pane's `pages`.
    WordListWindow {
        pane: PaneId,
        pages: Vec<usize>,
    },
    /// The previous or next page, or spread, of the pane's document.
    Turn {
        pane: PaneId,
        forward: bool,
    },
    /// Shows a file in a pane, replacing what it showed.
    Open(OpenRequest),
    Split(SplitAxis),
    FocusPane(PaneId),
    /// F6: focus moves to the next or previous pane, in reading order.
    CyclePane {
        forward: bool,
    },
    ClosePane(PaneId),
    /// A divider drag; sent every frame it moves.
    SetRatio(DividerPath, f32),
    /// Every tick of an appearance control; restyles on the same frame.
    SetAppearance(Appearance),
    /// The split's per-pane modes, from the appearance popover.
    SetIndependence(Independence),
    /// The ink dial, 10 to 100 %: how dark a book's text stays.
    SetInkContrast(f32),
    /// The popover's text dials for the active book.
    SetQuickText(crate::appearance_menu::QuickText),
    /// Keeps `look` as a preset, in a section when `group` names one.
    SavePreset {
        name: String,
        group: String,
        look: Appearance,
    },
    /// Deletes one of the user's presets.
    DeletePreset(String),
    SetTitlebarPinned(bool),
    Window(WindowCommand),
    /// The word card's and the mark menu's asks.
    Study(crate::study::Action),
    /// The page AI's menus: a task to run, a result to show.
    PageAi(crate::page_ai::Action),
    /// Reads the pane's book aloud from a place, else its page.
    ReadAloud {
        pane: PaneId,
        from: crate::read_aloud::ReadFrom,
    },
    /// Pins where reading aloud will start, to be moved or let rest.
    PlaceReadMarker {
        pane: PaneId,
        spot: mareader_speech::Spot,
    },
    /// Plays or pauses the reading; starts the focused book if none.
    ToggleReadAloud,
    StopReadAloud,
    /// Says a line in the voice settings pick; stops that line.
    TryVoice,
    /// Says a line in this voice of the voice tab's engine.
    TryVoiceOf(String),
    StopVoiceTry,
    /// Says a card's word in the reading voice; stops it.
    SayWord(String),
    /// A word in a voice other than the one picked, for its language.
    SayWordWith {
        word: String,
        voice: String,
    },
    StopSaying,
    /// Settings at a model's voices in `language`, opened and lit.
    RevealVoice(&'static mareader_speech::Model, &'static str),
    /// A voice model or voice to download, pause, cancel or remove.
    VoiceModel(mareader_speech::Item, Download),
    /// Explains the word selected in the pane, marking it.
    Explain(PaneId),
    /// Looks the pane's selection up in the dictionaries.
    LookUp(PaneId),
    /// Packs to download or remove, and the lookup card.
    Dict(crate::dict::Action),
    /// The Dictionary panel's choices, saved with the settings.
    SetDictionary(Box<mareader_settings::DictionarySettings>),
    /// The word-level dataset.
    Vocab(Download),
    /// The pronunciation download, beside the word levels.
    Pronunciation(Download),
    /// Highlights the pane's selection; a colour if one was picked.
    Highlight {
        pane: PaneId,
        pen: mareader_library::Pen,
        color: Option<mareader_appearance::Rgb>,
    },
    /// Highlights the pane's selection and opens a note on it.
    Note(PaneId),
    /// Bookmarks the pane's page, or takes its bookmark back; none is
    /// the focused pane.
    Bookmark(Option<PaneId>),
    /// Selects the first `text` on `page` in the pane, as a drag does.
    SelectText {
        pane: PaneId,
        page: usize,
        text: String,
    },
    /// Asks where to save an export, then writes it.
    Export {
        name: String,
        format: crate::export::Format,
        contents: String,
    },
    /// Shows a toast on top of the stack.
    Notify(Toast),
    /// Closes one toast, by its stack id.
    CloseToast(u64),
    /// Closes every toast.
    ClearToasts,
    /// Background work for the AI settings: keys, models, a trial.
    #[cfg(not(target_arch = "wasm32"))]
    Ai(crate::ai::Job),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    Minimize,
    ToggleMaximize,
    Close,
    StartDrag,
    ToggleFullscreen,
}

/// The frame's command queue, handed to every view.
pub type Commands = Vec<Command>;
