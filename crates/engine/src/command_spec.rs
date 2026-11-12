//! Every intent, named and described once, so a window and a terminal can
//! list the same set.
//!
//! The table is the registry `mareader-cli commands` prints and the parity
//! ledger cites by id.

use crate::command::Intent;

/// One intent: its stable id, and what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    /// Dotted and stable: `pane.split`, the id a ledger or a script names.
    pub id: &'static str,
    /// One line, in the imperative, saying what happens.
    pub summary: &'static str,
}

/// Every intent there is, one row each.
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

impl Intent {
    /// This intent's stable id.
    pub fn id(&self) -> &'static str {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Intent::Ai(..) => "ai.job",
            Intent::Arrow(..) => "page.arrow",
            Intent::AutoScroll => "page.autoscroll",
            Intent::Bookmark(..) => "mark.bookmark",
            Intent::ClosePane(..) => "pane.close",
            Intent::CopyPages { .. } => "page.copy",
            Intent::CyclePane { .. } => "pane.cycle",
            Intent::DeletePreset(..) => "settings.preset-delete",
            Intent::Dict(..) => "dict.act",
            Intent::Explain(..) => "word.explain",
            Intent::Export { .. } => "export.write",
            Intent::FirstPage => "page.first",
            Intent::FocusPane(..) => "pane.focus",
            Intent::ForgetPositions { .. } => "library.forget",
            Intent::GoTo { .. } => "page.go",
            Intent::Highlight { .. } => "mark.highlight",
            Intent::LastPage => "page.last",
            Intent::Library(..) => "library.act",
            Intent::LookUp(..) => "word.lookup",
            Intent::Note(..) => "mark.note",
            Intent::Open(..) => "reader.open",
            Intent::OpenBook(..) => "reader.open-book",
            Intent::OpenSettings(..) => "settings.open",
            Intent::Page { .. } => "page.step",
            Intent::PageAi(..) => "ai.page",
            Intent::PlaceBook { .. } => "reader.place-book",
            Intent::PlaceReadMarker { .. } => "read.marker",
            Intent::Pronunciation(..) => "dict.pronunciation",
            Intent::ReadAloud { .. } => "read.start",
            Intent::RevealVoice(..) => "voice.reveal",
            Intent::SavePreset { .. } => "settings.preset-save",
            Intent::SayWord(..) => "word.say",
            Intent::SayWordWith { .. } => "word.say-with",
            Intent::SelectText { .. } => "mark.select",
            Intent::SetAppearance(..) => "settings.appearance",
            Intent::SetDictionary(..) => "dict.settings",
            Intent::SetIndependence(..) => "settings.independence",
            Intent::SetInkContrast(..) => "settings.ink",
            Intent::SetLibraryView(..) => "library.view",
            Intent::SetQuickText(..) => "settings.quick-text",
            Intent::SetRatio(..) => "pane.ratio",
            Intent::SetSettings(..) => "settings.write",
            Intent::SetField { .. } => "settings.set",
            Intent::SetSidebarOverlay(..) => "settings.sidebar-overlay",
            Intent::SetTitlebarPinned(..) => "settings.titlebar-pin",
            Intent::Split(..) => "pane.split",
            Intent::StopReadAloud => "read.stop",
            Intent::StopSaying => "word.stop",
            Intent::StopVoiceTry => "voice.stop-try",
            Intent::Study(..) => "study.act",
            Intent::ToggleReadAloud => "read.toggle",
            Intent::TryVoice => "voice.try",
            Intent::TryVoiceOf(..) => "voice.try-of",
            Intent::Turn { .. } => "page.turn",
            Intent::View(..) => "pane.view",
            Intent::Vocab(..) => "vocab.download",
            Intent::VoiceModel(..) => "voice.model",
            Intent::WordList { .. } => "word.list",
            Intent::WordListWindow { .. } => "word.list-window",
            Intent::Zoom(..) => "pane.zoom",
        }
    }

    /// This intent's row in [`SPECS`], when it has one.
    ///
    /// A `None` is a missing row, which `tests/specs.rs` fails on; the
    /// lookup does not pretend otherwise.
    pub fn spec(&self) -> Option<&'static CommandSpec> {
        let id = self.id();
        SPECS.iter().find(|s| s.id == id)
    }
}
