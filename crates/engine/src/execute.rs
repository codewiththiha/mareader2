//! The reducer: every command, carried out against the session and its view.
//!
//! `Session::execute` is the one place an command takes effect. What the
//! session owns it changes here; what a controller owns it asks of the
//! [`View`]. Time arrives as `now`, in seconds, from whoever keeps a clock.

mod library;
mod panes;
mod study;
mod voice;

use crate::command::Command;
use crate::event::Event;
use crate::session::Session;
use crate::toast::Toast;
use crate::view::View;

impl Session {
    /// Carries out `command`, unless the page in the window has no use for it.
    pub fn execute(view: &mut dyn View, intent: Command, now: f64) {
        if view.session().route().takes(&intent) {
            dispatch(view, intent, now);
        }
    }
}

/// Exhaustive, so a new command cannot build until it is given a home.
fn dispatch(view: &mut dyn View, intent: Command, now: f64) {
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
        | Command::Turn { .. } => panes::run(view, intent, now),
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
            view.session().apply(intent);
        }
        Command::SetAppearance(look) => view.set_appearance(look),
        Command::OpenSettings(tab) => view.open_settings(tab),
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
        | Command::Pronunciation(_) => study::run(view, intent),
        Command::PageAi(action) => view.page_ai(action),
        #[cfg(not(target_arch = "wasm32"))]
        Command::Ai(job) => view.ai(job),
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
        | Command::StopSaying => voice::run(view, intent, now),
        Command::OpenBook(_)
        | Command::PlaceBook { .. }
        | Command::Library(_)
        | Command::ForgetPositions { .. } => library::run(view, intent),
    }
}

fn note(view: &mut dyn View, toast: Toast) {
    view.session().emit(Event::Toast(toast));
}

/// One setting written by its path; a value that will not serialize is not.
fn set(view: &mut dyn View, path: &str, json: serde_json::Result<String>) {
    if let Ok(value) = json {
        view.session().apply(Command::SetField {
            path: path.into(),
            value,
        });
    }
}
