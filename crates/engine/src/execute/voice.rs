//! What is heard: reading aloud, trying a voice, saying a word.

use mareader_speech::Item;

use super::{note, set};
use crate::command::Intent;
use crate::desk::Desk;
use crate::dict::Download;
use crate::read_aloud::ReadFrom;
use crate::toast::Toast;

pub(super) fn run(desk: &mut dyn Desk, intent: Intent, now: f64) {
    match intent {
        Intent::PlaceReadMarker { pane, spot } => {
            desk.place_read_marker(pane, spot, now);
            desk.session().wake();
        }
        Intent::ReadAloud { pane, from } => {
            // Reading from elsewhere sends a waiting pin off.
            desk.dismiss_read_markers(now);
            desk.read_aloud(pane, from);
        }
        Intent::ToggleReadAloud => {
            desk.dismiss_read_markers(now);
            if !desk.toggle_read_aloud() {
                let active = desk.session().active();
                desk.read_aloud(active, ReadFrom::default());
            }
        }
        Intent::StopReadAloud => desk.stop_read_aloud(),
        Intent::RevealVoice(model, language) => desk.reveal_voice(model, language),
        Intent::VoiceModel(item, action) => model(desk, item, action),
        other => say(desk, other),
    }
}

fn say(desk: &mut dyn Desk, intent: Intent) {
    match intent {
        Intent::TryVoice => desk.try_voice(),
        Intent::TryVoiceOf(voice) => desk.try_voice_of(&voice),
        Intent::StopVoiceTry => desk.stop_voice_try(),
        Intent::SayWord(word) => heard(desk, &word, None),
        Intent::SayWordWith { word, voice } => heard(desk, &word, Some(&voice)),
        Intent::StopSaying => desk.stop_saying(),
        _ => {}
    }
}

fn heard(desk: &mut dyn Desk, word: &str, voice: Option<&str>) {
    if let Err(why) = desk.say_word(word, voice) {
        note(desk, Toast::error(why));
    }
}

/// A download may change the voice picked, so the settings follow it.
fn model(desk: &mut dyn Desk, item: Item, action: Download) {
    let mut settings = desk.session().settings().read_aloud.clone();
    match desk.voice_model(item, action, &mut settings) {
        Ok(()) => set(desk, "read_aloud", serde_json::to_string(&settings)),
        Err(why) => note(desk, Toast::error(why)),
    }
}
