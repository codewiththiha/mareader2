//! The reducer: every command, carried out against the session and its desk.
//!
//! `Session::execute` is the one place an command takes effect. What the
//! session owns it changes here; what a controller owns it asks of the
//! [`Desk`]. Time arrives as `now`, in seconds, from whoever keeps a clock.

mod library;
mod panes;
mod study;
mod voice;

use crate::command::Command;
use crate::desk::Desk;
use crate::event::Event;
use crate::session::Session;
use crate::toast::Toast;

impl Session {
    /// Carries out `command`, unless the page in the window has no use for it.
    pub fn execute(desk: &mut dyn Desk, intent: Command, now: f64) {
        if desk.session().route().takes(&intent) {
            dispatch(desk, intent, now);
        }
    }
}

/// Exhaustive, so a new command cannot build until it is given a home.
fn dispatch(desk: &mut dyn Desk, intent: Command, now: f64) {
    match intent {
        Command::Split(_)
        | Command::FocusPane(_)
        | Command::CyclePane { .. }
        | Command::ClosePane(_)
        | Command::SetRatio(..)
        | Command::Open(_)
        | Command::Export { .. }
        | Command::CopyPages { .. }
        | Command::Zoom(_)
        | Command::View(_)
        | Command::Arrow(_)
        | Command::Page { .. }
        | Command::AutoScroll
        | Command::FirstPage
        | Command::LastPage
        | Command::GoTo { .. }
        | Command::Turn { .. } => panes::run(desk, intent, now),
        Command::SetSidebarOverlay(_)
        | Command::SetTitlebarPinned(_)
        | Command::SetSettings(_)
        | Command::SetField { .. }
        | Command::SetInkContrast(_)
        | Command::SetQuickText(_)
        | Command::SetIndependence(_)
        | Command::SavePreset { .. }
        | Command::DeletePreset(_)
        | Command::SetDictionary(_)
        | Command::SetLibraryView(_) => {
            desk.session().apply(intent);
        }
        Command::SetAppearance(look) => desk.set_appearance(look),
        Command::OpenSettings(tab) => desk.open_settings(tab),
        Command::WordList { .. }
        | Command::WordListWindow { .. }
        | Command::Study(_)
        | Command::Explain(_)
        | Command::LookUp(_)
        | Command::Highlight { .. }
        | Command::Note(_)
        | Command::Bookmark(_)
        | Command::SelectText { .. }
        | Command::Dict(_)
        | Command::Vocab(_)
        | Command::Pronunciation(_) => study::run(desk, intent),
        Command::PageAi(action) => desk.page_ai(action),
        #[cfg(not(target_arch = "wasm32"))]
        Command::Ai(job) => desk.ai(job),
        Command::PlaceReadMarker { .. }
        | Command::ReadAloud { .. }
        | Command::ToggleReadAloud
        | Command::StopReadAloud
        | Command::RevealVoice(..)
        | Command::VoiceModel(..)
        | Command::TryVoice
        | Command::TryVoiceOf(_)
        | Command::StopVoiceTry
        | Command::SayWord(_)
        | Command::SayWordWith { .. }
        | Command::StopSaying => voice::run(desk, intent, now),
        Command::OpenBook(_)
        | Command::PlaceBook { .. }
        | Command::Library(_)
        | Command::ForgetPositions { .. } => library::run(desk, intent),
    }
}

fn note(desk: &mut dyn Desk, toast: Toast) {
    desk.session().emit(Event::Toast(toast));
}

/// One setting written by its path; a value that will not serialize is not.
fn set(desk: &mut dyn Desk, path: &str, json: serde_json::Result<String>) {
    if let Ok(value) = json {
        desk.session().apply(Command::SetField {
            path: path.into(),
            value,
        });
    }
}
