//! What is heard: reading aloud, trying a voice, saying a word.

use mareader_speech::Item;

use super::{note, set};
use crate::command::Command;
use crate::desk::Desk;
use crate::dict::Download;
use crate::read_aloud::ReadFrom;
use crate::toast::Toast;

pub(super) fn run(desk: &mut dyn Desk, intent: Command, now: f64) {
    match intent {
        Command::PlaceReadMarker { pane, spot } => {
            desk.place_read_marker(pane, spot, now);
            desk.session().wake();
        }
        Command::ReadAloud { pane, from } => {
            // Reading from elsewhere sends a waiting pin off.
            desk.dismiss_read_markers(now);
            desk.read_aloud(pane, from);
        }
        Command::ToggleReadAloud => {
            desk.dismiss_read_markers(now);
            if !desk.toggle_read_aloud() {
                let active = desk.session().active();
                desk.read_aloud(active, ReadFrom::default());
            }
        }
        Command::StopReadAloud => desk.stop_read_aloud(),
        Command::RevealVoice(model, language) => desk.reveal_voice(model, language),
        Command::VoiceModel(item, action) => model(desk, item, action),
        other => say(desk, other),
    }
}

fn say(desk: &mut dyn Desk, intent: Command) {
    match intent {
        Command::TryVoice => desk.try_voice(),
        Command::TryVoiceOf(voice) => desk.try_voice_of(&voice),
        Command::StopVoiceTry => desk.stop_voice_try(),
        Command::SayWord(word) => heard(desk, &word, None),
        Command::SayWordWith { word, voice } => heard(desk, &word, Some(&voice)),
        Command::StopSaying => desk.stop_saying(),
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
