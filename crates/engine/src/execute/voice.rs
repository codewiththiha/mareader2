//! What is heard: reading aloud, trying a voice, saying a word.

use mareader_speech::Item;

use super::{note, set};
use crate::command::Command;
use crate::dict::Download;
use crate::host::Host;
use crate::read_aloud::ReadFrom;
use crate::toast::Toast;

pub(super) fn run(host: &mut dyn Host, intent: Command, now: f64) {
    match intent {
        Command::PlaceReadMarker { pane, spot } => {
            host.place_read_marker(pane, spot, now);
            host.session().wake();
        }
        Command::ReadAloud { pane, from } => {
            // Reading from elsewhere sends a waiting pin off.
            host.dismiss_read_markers(now);
            host.read_aloud(pane, from);
        }
        Command::ToggleReadAloud => {
            host.dismiss_read_markers(now);
            if !host.toggle_read_aloud() {
                let active = host.session().active();
                host.read_aloud(active, ReadFrom::default());
            }
        }
        Command::StopReadAloud => host.stop_read_aloud(),
        Command::RevealVoice(model, language) => host.reveal_voice(model, language),
        Command::VoiceModel(item, action) => model(host, item, action),
        other => say(host, other),
    }
}

fn say(host: &mut dyn Host, intent: Command) {
    match intent {
        Command::TryVoice => host.try_voice(),
        Command::TryVoiceOf(voice) => host.try_voice_of(&voice),
        Command::StopVoiceTry => host.stop_voice_try(),
        Command::SayWord(word) => heard(host, &word, None),
        Command::SayWordWith { word, voice } => heard(host, &word, Some(&voice)),
        Command::StopSaying => host.stop_saying(),
        _ => {}
    }
}

fn heard(host: &mut dyn Host, word: &str, voice: Option<&str>) {
    if let Err(why) = host.say_word(word, voice) {
        note(host, Toast::error(why));
    }
}

/// A download may change the voice picked, so the settings follow it.
fn model(host: &mut dyn Host, item: Item, action: Download) {
    let mut settings = host.session().settings().read_aloud.clone();
    match host.voice_model(item, action, &mut settings) {
        Ok(()) => set(host, "read_aloud", serde_json::to_string(&settings)),
        Err(why) => note(host, Toast::error(why)),
    }
}
