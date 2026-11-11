//! The reducer: every intent, carried out against the session and its desk.
//!
//! `Session::execute` is the one place an intent takes effect. What the
//! session owns it changes here; what a controller owns it asks of the
//! [`Desk`]. Time arrives as `now`, in seconds, from whoever keeps a clock.

mod library;
mod panes;
mod study;
mod voice;

use crate::command::Intent;
use crate::desk::Desk;
use crate::event::Event;
use crate::session::Session;
use crate::toast::Toast;

impl Session {
    /// Carries out `intent`, unless the page in the window has no use for it.
    pub fn execute(desk: &mut dyn Desk, intent: Intent, now: f64) {
        if desk.session().route().takes(&intent) {
            dispatch(desk, intent, now);
        }
    }
}

/// Exhaustive, so a new intent cannot build until it is given a home.
fn dispatch(desk: &mut dyn Desk, intent: Intent, now: f64) {
    match intent {
        Intent::Split(_)
        | Intent::FocusPane(_)
        | Intent::CyclePane { .. }
        | Intent::ClosePane(_)
        | Intent::SetRatio(..)
        | Intent::Open(_)
        | Intent::Export { .. }
        | Intent::CopyPages { .. }
        | Intent::Zoom(_)
        | Intent::View(_)
        | Intent::Arrow(_)
        | Intent::Page { .. }
        | Intent::AutoScroll
        | Intent::FirstPage
        | Intent::LastPage
        | Intent::GoTo { .. }
        | Intent::Turn { .. } => panes::run(desk, intent, now),
        Intent::SetSidebarOverlay(_)
        | Intent::SetTitlebarPinned(_)
        | Intent::SetSettings(_)
        | Intent::SetField { .. }
        | Intent::SetInkContrast(_)
        | Intent::SetQuickText(_)
        | Intent::SetIndependence(_)
        | Intent::SavePreset { .. }
        | Intent::DeletePreset(_)
        | Intent::SetDictionary(_)
        | Intent::SetLibraryView(_) => {
            desk.session().apply(intent);
        }
        Intent::SetAppearance(look) => desk.set_appearance(look),
        Intent::OpenSettings(tab) => desk.open_settings(tab),
        Intent::WordList { .. }
        | Intent::WordListWindow { .. }
        | Intent::Study(_)
        | Intent::Explain(_)
        | Intent::LookUp(_)
        | Intent::Highlight { .. }
        | Intent::Note(_)
        | Intent::Bookmark(_)
        | Intent::SelectText { .. }
        | Intent::Dict(_)
        | Intent::Vocab(_)
        | Intent::Pronunciation(_) => study::run(desk, intent),
        Intent::PageAi(action) => desk.page_ai(action),
        #[cfg(not(target_arch = "wasm32"))]
        Intent::Ai(job) => desk.ai(job),
        Intent::PlaceReadMarker { .. }
        | Intent::ReadAloud { .. }
        | Intent::ToggleReadAloud
        | Intent::StopReadAloud
        | Intent::RevealVoice(..)
        | Intent::VoiceModel(..)
        | Intent::TryVoice
        | Intent::TryVoiceOf(_)
        | Intent::StopVoiceTry
        | Intent::SayWord(_)
        | Intent::SayWordWith { .. }
        | Intent::StopSaying => voice::run(desk, intent, now),
        Intent::OpenBook(_)
        | Intent::PlaceBook { .. }
        | Intent::Library(_)
        | Intent::ForgetPositions { .. } => library::run(desk, intent),
    }
}

fn note(desk: &mut dyn Desk, toast: Toast) {
    desk.session().emit(Event::Toast(toast));
}

/// One setting written by its path; a value that will not serialize is not.
fn set(desk: &mut dyn Desk, path: &str, json: serde_json::Result<String>) {
    if let Ok(value) = json {
        desk.session().apply(Intent::SetField {
            path: path.into(),
            value,
        });
    }
}
