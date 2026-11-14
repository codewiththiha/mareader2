//! The reducer: every command, carried out against the session and its host.
//!
//! `Session::execute` is the one place an command takes effect. What the
//! session owns it changes here; what a controller owns it asks of the
//! [`Host`]. Time arrives as `now`, in seconds, from whoever keeps a clock.

mod library;
mod panes;
mod study;
mod voice;

use crate::command::Command;
use crate::event::Event;
use crate::host::Host;
use crate::session::Session;
use crate::toast::Toast;

impl Session {
    /// Carries out `command`, unless the page in the window has no use for it.
    pub fn execute(host: &mut dyn Host, intent: Command, now: f64) {
        if host.session().route().takes(&intent) {
            dispatch(host, intent, now);
        }
    }
}

/// Exhaustive, so a new command cannot build until it is given a home.
fn dispatch(host: &mut dyn Host, intent: Command, now: f64) {
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
        | Command::Turn { .. } => panes::run(host, intent, now),
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
            host.session().apply(intent);
        }
        Command::SetAppearance(look) => host.set_appearance(look),
        Command::OpenSettings(tab) => host.open_settings(tab),
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
        | Command::Pronunciation(_) => study::run(host, intent),
        Command::PageAi(action) => host.page_ai(action),
        #[cfg(not(target_arch = "wasm32"))]
        Command::Ai(job) => host.ai(job),
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
        | Command::StopSaying => voice::run(host, intent, now),
        Command::OpenBook(_)
        | Command::PlaceBook { .. }
        | Command::Library(_)
        | Command::ForgetPositions { .. } => library::run(host, intent),
    }
}

fn note(host: &mut dyn Host, toast: Toast) {
    host.session().emit(Event::Toast(toast));
}

/// One setting written by its path; a value that will not serialize is not.
fn set(host: &mut dyn Host, path: &str, json: serde_json::Result<String>) {
    if let Ok(value) = json {
        host.session().apply(Command::SetField {
            path: path.into(),
            value,
        });
    }
}
