//! Every command, named and described once, so a window and a terminal can
//! list the same set.
//!
//! The table is the registry `mareader-cli commands` prints and the parity
//! ledger cites by id.

use crate::command::Command;

/// One command: its stable id, and what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    /// Dotted and stable: `pane.split`, the id a ledger or a script names.
    pub id: &'static str,
    /// One line, in the imperative, saying what happens.
    pub summary: &'static str,
}

/// Every command there is, one row each.
pub const SPECS: &[CommandSpec] = &[
    #[cfg(not(target_arch = "wasm32"))]
    CommandSpec {
        id: "ai.job",
        summary: "Background work for the AI settings: keys, models, a trial.",
    },
    CommandSpec {
        id: "page.arrow",
        summary: "An arrow key over the active document: a turn, or the scroll's.",
    },
    #[cfg(not(target_arch = "wasm32"))]
    CommandSpec {
        id: "vocab.attach",
        summary: "The word levels' dataset, at the folder a shot or the settings name.",
    },
    CommandSpec {
        id: "page.autoscroll",
        summary: "Shift+A: the active pane's auto-scroll on or off.",
    },
    CommandSpec {
        id: "mark.bookmark",
        summary: "Bookmarks the pane's page, or takes its bookmark back; none is the focused pane.",
    },
    CommandSpec {
        id: "pane.close",
        summary: "Closes a pane, and its book with it.",
    },
    CommandSpec {
        id: "page.copy",
        summary: "Puts the text of the pane's `pages` on the clipboard.",
    },
    CommandSpec {
        id: "pane.cycle",
        summary: "F6: focus moves to the next or previous pane, in reading order.",
    },
    CommandSpec {
        id: "settings.preset-delete",
        summary: "Deletes one of the user's presets.",
    },
    CommandSpec {
        id: "dict.act",
        summary: "Packs to download or remove, and the lookup card.",
    },
    CommandSpec {
        id: "word.explain",
        summary: "Explains the word selected in the pane, marking it.",
    },
    CommandSpec {
        id: "export.write",
        summary: "Asks where to save an export, then writes it.",
    },
    CommandSpec {
        id: "page.first",
        summary: "Home: the active document's first page.",
    },
    CommandSpec {
        id: "pane.focus",
        summary: "Makes a pane the one the keys go to.",
    },
    CommandSpec {
        id: "library.forget",
        summary: "Resets live panes so an erased location cannot be saved back.",
    },
    CommandSpec {
        id: "page.go",
        summary: "Shows `page` of the pane's document, from its start.",
    },
    CommandSpec {
        id: "mark.highlight",
        summary: "Highlights the pane's selection; a colour if one was picked.",
    },
    CommandSpec {
        id: "page.last",
        summary: "End: the active document's last page.",
    },
    CommandSpec {
        id: "library.act",
        summary: "Folders, scopes and finding lost books.",
    },
    CommandSpec {
        id: "word.lookup",
        summary: "Looks the pane's selection up in the dictionaries.",
    },
    CommandSpec {
        id: "pane.move",
        summary: "Moves a lifted pane to another's edge, in the room the window has.",
    },
    CommandSpec {
        id: "mark.note",
        summary: "Highlights the pane's selection and opens a note on it.",
    },
    CommandSpec {
        id: "reader.open",
        summary: "Shows a file in a pane, replacing what it showed.",
    },
    CommandSpec {
        id: "reader.open-book",
        summary: "Opens a library book as Settings › Panes says, or focuses it.",
    },
    CommandSpec {
        id: "settings.open",
        summary: "Opens the settings window on a tab.",
    },
    CommandSpec {
        id: "page.step",
        summary: "A page key: nearly a screen along a strip, a turn when paged.",
    },
    CommandSpec {
        id: "ai.page",
        summary: "The page AI's menus: a task to run, a result to show.",
    },
    CommandSpec {
        id: "reader.place-book",
        summary: "A library book placed from the rail: clicked, or dropped on a pane.",
    },
    CommandSpec {
        id: "read.marker",
        summary: "Pins where reading aloud will start, to be moved or let rest.",
    },
    CommandSpec {
        id: "dict.pronunciation",
        summary: "The pronunciation download, beside the word levels.",
    },
    CommandSpec {
        id: "read.start",
        summary: "Reads the pane's book aloud from a place, else its page.",
    },
    CommandSpec {
        id: "voice.reveal",
        summary: "Settings at a model's voices in `language`, opened and lit.",
    },
    CommandSpec {
        id: "settings.preset-save",
        summary: "Keeps `look` as a preset, in a section when `group` names one.",
    },
    CommandSpec {
        id: "word.say",
        summary: "Says a card's word in the reading voice; stops it.",
    },
    CommandSpec {
        id: "word.say-with",
        summary: "A word in a voice other than the one picked, for its language.",
    },
    CommandSpec {
        id: "mark.select",
        summary: "Selects the first `text` on `page` in the pane, as a drag does.",
    },
    CommandSpec {
        id: "settings.appearance",
        summary: "Every tick of an appearance control; restyles on the same frame.",
    },
    CommandSpec {
        id: "dict.settings",
        summary: "The Dictionary panel's choices, saved with the settings.",
    },
    CommandSpec {
        id: "settings.independence",
        summary: "The split's per-pane modes, from the appearance popover.",
    },
    CommandSpec {
        id: "settings.ink",
        summary: "The ink dial, 10 to 100 %: how dark a book's text stays.",
    },
    CommandSpec {
        id: "library.view",
        summary: "The library page's layout and order.",
    },
    CommandSpec {
        id: "settings.quick-text",
        summary: "The popover's text dials for the active book.",
    },
    CommandSpec {
        id: "pane.ratio",
        summary: "A divider drag; sent every frame it moves.",
    },
    CommandSpec {
        id: "settings.write",
        summary: "Every edit in the settings window; applies on the same frame.",
    },
    CommandSpec {
        id: "settings.set",
        summary: "One setting, by its dotted path, to its JSON value.  The channel a dial uses when no variant names it: `text.size_pct` to `170`. Bad paths and bad values are refused, not applied.",
    },
    CommandSpec {
        id: "settings.sidebar-overlay",
        summary: "Floating over the panes, or docked beside them.",
    },
    CommandSpec {
        id: "settings.titlebar-pin",
        summary: "Keeps the title bar up, or lets it follow the pointer.",
    },
    CommandSpec {
        id: "pane.split",
        summary: "Splits the active pane in two along an axis.",
    },
    CommandSpec {
        id: "read.stop",
        summary: "Stops reading aloud.",
    },
    CommandSpec {
        id: "word.stop",
        summary: "Stops the card's word.",
    },
    CommandSpec {
        id: "voice.stop-try",
        summary: "Stops the voice settings' sample line.",
    },
    CommandSpec {
        id: "study.act",
        summary: "The word card's and the mark menu's asks.",
    },
    CommandSpec {
        id: "pane.swap",
        summary: "Trades two panes' places in the tree.",
    },
    CommandSpec {
        id: "read.toggle",
        summary: "Plays or pauses the reading; starts the focused book if none.",
    },
    CommandSpec {
        id: "voice.try",
        summary: "Says a line in the voice settings pick; stops that line.",
    },
    CommandSpec {
        id: "voice.try-of",
        summary: "Says a line in this voice of the voice tab's engine.",
    },
    CommandSpec {
        id: "page.turn",
        summary: "The previous or next page, or spread, of the pane's document.",
    },
    CommandSpec {
        id: "pane.view",
        summary: "The active pane's view mode, kept on the page read.",
    },
    CommandSpec {
        id: "vocab.download",
        summary: "The word-level dataset.",
    },
    CommandSpec {
        id: "voice.model",
        summary: "A voice model or voice to download, pause, cancel or remove.",
    },
    CommandSpec {
        id: "word.list",
        summary: "Copies a word list of the pane's `pages`, with the instant options.",
    },
    CommandSpec {
        id: "word.list-window",
        summary: "Opens the word list window on the pane's `pages`.",
    },
    CommandSpec {
        id: "pane.zoom",
        summary: "Zooms the active pane's document.",
    },
];

impl Command {
    /// This command's stable id.
    pub fn id(&self) -> &'static str {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Command::Ai(..) => "ai.job",
            Command::Arrow(..) => "page.arrow",
            #[cfg(not(target_arch = "wasm32"))]
            Command::AttachVocab(..) => "vocab.attach",
            Command::AutoScroll => "page.autoscroll",
            Command::Bookmark(..) => "mark.bookmark",
            Command::ClosePane(..) => "pane.close",
            Command::CopyPages { .. } => "page.copy",
            Command::CyclePane { .. } => "pane.cycle",
            Command::DeletePreset(..) => "settings.preset-delete",
            Command::Dict(..) => "dict.act",
            Command::Explain(..) => "word.explain",
            Command::Export { .. } => "export.write",
            Command::FirstPage => "page.first",
            Command::FocusPane(..) => "pane.focus",
            Command::ForgetPositions { .. } => "library.forget",
            Command::GoTo { .. } => "page.go",
            Command::Highlight { .. } => "mark.highlight",
            Command::LastPage => "page.last",
            Command::Library(..) => "library.act",
            Command::LookUp(..) => "word.lookup",
            Command::MovePane { .. } => "pane.move",
            Command::Note(..) => "mark.note",
            Command::Open(..) => "reader.open",
            Command::OpenBook(..) => "reader.open-book",
            Command::OpenSettings(..) => "settings.open",
            Command::Page { .. } => "page.step",
            Command::PageAi(..) => "ai.page",
            Command::PlaceBook { .. } => "reader.place-book",
            Command::PlaceReadMarker { .. } => "read.marker",
            Command::Pronunciation(..) => "dict.pronunciation",
            Command::ReadAloud { .. } => "read.start",
            Command::RevealVoice(..) => "voice.reveal",
            Command::SavePreset { .. } => "settings.preset-save",
            Command::SayWord(..) => "word.say",
            Command::SayWordWith { .. } => "word.say-with",
            Command::SelectText { .. } => "mark.select",
            Command::SetAppearance(..) => "settings.appearance",
            Command::SetDictionary(..) => "dict.settings",
            Command::SetIndependence(..) => "settings.independence",
            Command::SetInkContrast(..) => "settings.ink",
            Command::SetLibraryView(..) => "library.view",
            Command::SetQuickText(..) => "settings.quick-text",
            Command::SetRatio(..) => "pane.ratio",
            Command::SetSettings(..) => "settings.write",
            Command::SetField { .. } => "settings.set",
            Command::SetSidebarOverlay(..) => "settings.sidebar-overlay",
            Command::SetTitlebarPinned(..) => "settings.titlebar-pin",
            Command::Split(..) => "pane.split",
            Command::StopReadAloud => "read.stop",
            Command::StopSaying => "word.stop",
            Command::StopVoiceTry => "voice.stop-try",
            Command::Study(..) => "study.act",
            Command::SwapPanes { .. } => "pane.swap",
            Command::ToggleReadAloud => "read.toggle",
            Command::TryVoice => "voice.try",
            Command::TryVoiceOf(..) => "voice.try-of",
            Command::Turn { .. } => "page.turn",
            Command::View(..) => "pane.view",
            Command::Vocab(..) => "vocab.download",
            Command::VoiceModel(..) => "voice.model",
            Command::WordList { .. } => "word.list",
            Command::WordListWindow { .. } => "word.list-window",
            Command::Zoom(..) => "pane.zoom",
        }
    }

    /// This command's row in [`SPECS`], when it has one.
    ///
    /// A `None` is a missing row, which `tests/specs.rs` fails on; the
    /// lookup does not pretend otherwise.
    pub fn spec(&self) -> Option<&'static CommandSpec> {
        let id = self.id();
        SPECS.iter().find(|s| s.id == id)
    }
}
