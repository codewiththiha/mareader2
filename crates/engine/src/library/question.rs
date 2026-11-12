//! One question the library asks the reader, and what answering it needs.

use mareader_library::gate::Clash;
use mareader_library::screen::{FileAsk, Why};
use mareader_library::{Found, Library, MergeAsk, MoveAsk, Options, Placement};

/// One question, with what answering it needs.
#[derive(Clone, Debug, PartialEq)]
pub enum Question {
    /// A folder's name is taken at the top level, asked before the walk.
    Clash {
        clash: Clash,
        opts: Options,
    },
    File(FileAsk),
    Merge(MergeAsk),
    /// A moved book meets one of its name where it lands.
    Move(MoveAsk),
}

/// Questions alike enough to share "apply to all" and a count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Clash,
    Name,
    Held,
    Merge,
}

impl Question {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Clash { .. } => Kind::Clash,
            Self::File(ask) if ask.why == Why::Name => Kind::Name,
            Self::Move(_) => Kind::Name,
            Self::File(_) => Kind::Held,
            Self::Merge(_) => Kind::Merge,
        }
    }

    pub fn offers(&self) -> &'static [Placement] {
        match self {
            Self::Clash { clash, .. } => clash.offers(),
            Self::File(ask) => ask.offers(),
            Self::Merge(_) => Placement::FOLDER_MERGE,
            Self::Move(ask) => ask.offers(),
        }
    }

    /// The arriving file, for an answer that copies it first.
    pub fn found(&self) -> Option<&Found> {
        match self {
            Self::Clash { .. } | Self::Move(_) => None,
            Self::File(ask) => Some(&ask.found),
            Self::Merge(ask) => Some(&ask.found),
        }
    }

    /// Whether `answer` lands a store copy that must be made first.
    pub fn copies(&self, answer: Placement, model: &Library) -> bool {
        match self {
            Self::Clash { .. } | Self::Move(_) => false,
            Self::File(_) => answer == Placement::KeepBoth,
            Self::Merge(ask) => {
                let copying = model.folder(ask.folder).is_some_and(|f| f.mode().copies());
                copying && matches!(answer, Placement::KeepBoth | Placement::Replace)
            }
        }
    }
}
