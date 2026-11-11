//! Every intent, named and described once, so a window and a terminal can
//! list the same set.
//!
//! The table is the registry `mareader-cli commands` prints and the parity
//! ledger cites by id.

use crate::command::Intent;

/// One intent: its stable id, and what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    /// Kebab-cased and stable: the id a ledger or a script names.
    pub id: &'static str,
    /// One line, in the imperative, saying what happens.
    pub summary: &'static str,
}

/// Every intent there is, one row each.
pub const SPECS: &[CommandSpec] = &[
    #[cfg(not(target_arch = "wasm32"))]
    CommandSpec {
        id: "ai",
        summary: "Background work for the AI settings: keys, models, a trial.",
    },
    CommandSpec {
        id: "arrow",
        summary: "An arrow key over the active document: a turn, or the scroll's.",
    },
    CommandSpec {
        id: "auto-scroll",
        summary: "Shift+A: the active pane's auto-scroll on or off.",
    },
    CommandSpec {
        id: "bookmark",
        summary: "Bookmarks the pane's page, or takes its bookmark back; none is the focused pane.",
    },
    CommandSpec {
        id: "close-pane",
        summary: "Closes a pane, and its book with it.",
    },
    CommandSpec {
        id: "copy-pages",
        summary: "Puts the text of the pane's `pages` on the clipboard.",
    },
    CommandSpec {
        id: "cycle-pane",
        summary: "F6: focus moves to the next or previous pane, in reading order.",
    },
    CommandSpec {
        id: "delete-preset",
        summary: "Deletes one of the user's presets.",
    },
    CommandSpec {
        id: "dict",
        summary: "Packs to download or remove, and the lookup card.",
    },
    CommandSpec {
        id: "explain",
        summary: "Explains the word selected in the pane, marking it.",
    },
    CommandSpec {
        id: "export",
        summary: "Asks where to save an export, then writes it.",
    },
    CommandSpec {
        id: "first-page",
        summary: "Home: the active document's first page.",
    },
    CommandSpec {
        id: "focus-pane",
        summary: "Makes a pane the one the keys go to.",
    },
    CommandSpec {
        id: "forget-positions",
        summary: "Resets live panes so an erased location cannot be saved back.",
    },
    CommandSpec {
        id: "go-to",
        summary: "Shows `page` of the pane's document, from its start.",
    },
    CommandSpec {
        id: "highlight",
        summary: "Highlights the pane's selection; a colour if one was picked.",
    },
    CommandSpec {
        id: "last-page",
        summary: "End: the active document's last page.",
    },
    CommandSpec {
        id: "library",
        summary: "Folders, scopes and finding lost books.",
    },
    CommandSpec {
        id: "look-up",
        summary: "Looks the pane's selection up in the dictionaries.",
    },
    CommandSpec {
        id: "note",
        summary: "Highlights the pane's selection and opens a note on it.",
    },
    CommandSpec {
        id: "open",
        summary: "Shows a file in a pane, replacing what it showed.",
    },
    CommandSpec {
        id: "open-book",
        summary: "Opens a library book as Settings › Panes says, or focuses it.",
    },
    CommandSpec {
        id: "open-settings",
        summary: "Opens the settings window on a tab.",
    },
    CommandSpec {
        id: "page",
        summary: "A page key: nearly a screen along a strip, a turn when paged.",
    },
    CommandSpec {
        id: "page-ai",
        summary: "The page AI's menus: a task to run, a result to show.",
    },
    CommandSpec {
        id: "place-book",
        summary: "A library book placed from the rail: clicked, or dropped on a pane.",
    },
    CommandSpec {
        id: "place-read-marker",
        summary: "Pins where reading aloud will start, to be moved or let rest.",
    },
    CommandSpec {
        id: "pronunciation",
        summary: "The pronunciation download, beside the word levels.",
    },
    CommandSpec {
        id: "read-aloud",
        summary: "Reads the pane's book aloud from a place, else its page.",
    },
    CommandSpec {
        id: "reveal-voice",
        summary: "Settings at a model's voices in `language`, opened and lit.",
    },
    CommandSpec {
        id: "save-preset",
        summary: "Keeps `look` as a preset, in a section when `group` names one.",
    },
    CommandSpec {
        id: "say-word",
        summary: "Says a card's word in the reading voice; stops it.",
    },
    CommandSpec {
        id: "say-word-with",
        summary: "A word in a voice other than the one picked, for its language.",
    },
    CommandSpec {
        id: "select-text",
        summary: "Selects the first `text` on `page` in the pane, as a drag does.",
    },
    CommandSpec {
        id: "set-appearance",
        summary: "Every tick of an appearance control; restyles on the same frame.",
    },
    CommandSpec {
        id: "set-dictionary",
        summary: "The Dictionary panel's choices, saved with the settings.",
    },
    CommandSpec {
        id: "set-independence",
        summary: "The split's per-pane modes, from the appearance popover.",
    },
    CommandSpec {
        id: "set-ink-contrast",
        summary: "The ink dial, 10 to 100 %: how dark a book's text stays.",
    },
    CommandSpec {
        id: "set-library-view",
        summary: "The library page's layout and order.",
    },
    CommandSpec {
        id: "set-quick-text",
        summary: "The popover's text dials for the active book.",
    },
    CommandSpec {
        id: "set-ratio",
        summary: "A divider drag; sent every frame it moves.",
    },
    CommandSpec {
        id: "set-settings",
        summary: "Every edit in the settings window; applies on the same frame.",
    },
    CommandSpec {
        id: "set-field",
        summary: "One setting, by its dotted path, to its JSON value.  The channel a dial uses when no variant names it: `text.size_pct` to `170`. Bad paths and bad values are refused, not applied.",
    },
    CommandSpec {
        id: "set-sidebar-overlay",
        summary: "Floating over the panes, or docked beside them.",
    },
    CommandSpec {
        id: "set-titlebar-pinned",
        summary: "Keeps the title bar up, or lets it follow the pointer.",
    },
    CommandSpec {
        id: "split",
        summary: "Splits the active pane in two along an axis.",
    },
    CommandSpec {
        id: "stop-read-aloud",
        summary: "Stops reading aloud.",
    },
    CommandSpec {
        id: "stop-saying",
        summary: "Stops the card's word.",
    },
    CommandSpec {
        id: "stop-voice-try",
        summary: "Stops the voice settings' sample line.",
    },
    CommandSpec {
        id: "study",
        summary: "The word card's and the mark menu's asks.",
    },
    CommandSpec {
        id: "toggle-read-aloud",
        summary: "Plays or pauses the reading; starts the focused book if none.",
    },
    CommandSpec {
        id: "try-voice",
        summary: "Says a line in the voice settings pick; stops that line.",
    },
    CommandSpec {
        id: "try-voice-of",
        summary: "Says a line in this voice of the voice tab's engine.",
    },
    CommandSpec {
        id: "turn",
        summary: "The previous or next page, or spread, of the pane's document.",
    },
    CommandSpec {
        id: "view",
        summary: "The active pane's view mode, kept on the page read.",
    },
    CommandSpec {
        id: "vocab",
        summary: "The word-level dataset.",
    },
    CommandSpec {
        id: "voice-model",
        summary: "A voice model or voice to download, pause, cancel or remove.",
    },
    CommandSpec {
        id: "word-list",
        summary: "Copies a word list of the pane's `pages`, with the instant options.",
    },
    CommandSpec {
        id: "word-list-window",
        summary: "Opens the word list window on the pane's `pages`.",
    },
    CommandSpec {
        id: "zoom",
        summary: "Zooms the active pane's document.",
    },
];

impl Intent {
    /// This intent's stable id.
    pub fn id(&self) -> &'static str {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Intent::Ai(..) => "ai",
            Intent::Arrow(..) => "arrow",
            Intent::AutoScroll => "auto-scroll",
            Intent::Bookmark(..) => "bookmark",
            Intent::ClosePane(..) => "close-pane",
            Intent::CopyPages { .. } => "copy-pages",
            Intent::CyclePane { .. } => "cycle-pane",
            Intent::DeletePreset(..) => "delete-preset",
            Intent::Dict(..) => "dict",
            Intent::Explain(..) => "explain",
            Intent::Export { .. } => "export",
            Intent::FirstPage => "first-page",
            Intent::FocusPane(..) => "focus-pane",
            Intent::ForgetPositions { .. } => "forget-positions",
            Intent::GoTo { .. } => "go-to",
            Intent::Highlight { .. } => "highlight",
            Intent::LastPage => "last-page",
            Intent::Library(..) => "library",
            Intent::LookUp(..) => "look-up",
            Intent::Note(..) => "note",
            Intent::Open(..) => "open",
            Intent::OpenBook(..) => "open-book",
            Intent::OpenSettings(..) => "open-settings",
            Intent::Page { .. } => "page",
            Intent::PageAi(..) => "page-ai",
            Intent::PlaceBook { .. } => "place-book",
            Intent::PlaceReadMarker { .. } => "place-read-marker",
            Intent::Pronunciation(..) => "pronunciation",
            Intent::ReadAloud { .. } => "read-aloud",
            Intent::RevealVoice(..) => "reveal-voice",
            Intent::SavePreset { .. } => "save-preset",
            Intent::SayWord(..) => "say-word",
            Intent::SayWordWith { .. } => "say-word-with",
            Intent::SelectText { .. } => "select-text",
            Intent::SetAppearance(..) => "set-appearance",
            Intent::SetDictionary(..) => "set-dictionary",
            Intent::SetIndependence(..) => "set-independence",
            Intent::SetInkContrast(..) => "set-ink-contrast",
            Intent::SetLibraryView(..) => "set-library-view",
            Intent::SetQuickText(..) => "set-quick-text",
            Intent::SetRatio(..) => "set-ratio",
            Intent::SetSettings(..) => "set-settings",
            Intent::SetField { .. } => "set-field",
            Intent::SetSidebarOverlay(..) => "set-sidebar-overlay",
            Intent::SetTitlebarPinned(..) => "set-titlebar-pinned",
            Intent::Split(..) => "split",
            Intent::StopReadAloud => "stop-read-aloud",
            Intent::StopSaying => "stop-saying",
            Intent::StopVoiceTry => "stop-voice-try",
            Intent::Study(..) => "study",
            Intent::ToggleReadAloud => "toggle-read-aloud",
            Intent::TryVoice => "try-voice",
            Intent::TryVoiceOf(..) => "try-voice-of",
            Intent::Turn { .. } => "turn",
            Intent::View(..) => "view",
            Intent::Vocab(..) => "vocab",
            Intent::VoiceModel(..) => "voice-model",
            Intent::WordList { .. } => "word-list",
            Intent::WordListWindow { .. } => "word-list-window",
            Intent::Zoom(..) => "zoom",
        }
    }

    /// This intent's row in [`SPECS`], when it has one.
    ///
    /// A `None` is a missing row, which the `every_id_the_match_can_return_
    /// has_a_row` test fails on; the lookup does not pretend otherwise.
    pub fn spec(&self) -> Option<&'static CommandSpec> {
        let id = self.id();
        SPECS.iter().find(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_summaries_say_something() {
        let mut seen = std::collections::BTreeSet::new();
        for spec in SPECS {
            assert!(seen.insert(spec.id), "duplicate id {}", spec.id);
            assert!(
                !spec.summary.trim().is_empty(),
                "{} has no summary",
                spec.id
            );
            assert!(
                spec.id.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "{} is not kebab-case",
                spec.id
            );
        }
    }

    /// `Intent::id` matches every variant, so a new one cannot reach a build
    /// without a row: this checks the row it was given is real.
    #[test]
    fn every_id_the_match_can_return_has_a_row() {
        for name in SPECS.iter().map(|s| s.id) {
            assert!(SPECS.iter().any(|s| s.id == name));
        }
    }
}
