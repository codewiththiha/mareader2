//! What is heard: reading aloud, trying a voice, saying a word.

use mareader_speech::Item;

use super::{note, set};
use crate::command::Command;
use crate::dict::Download;
use crate::read_aloud::ReadFrom;
use crate::toast::Toast;
use crate::view::View;

pub(super) fn run(view: &mut dyn View, intent: Command, now: f64) {
    match intent {
        Command::PlaceReadMarker { pane, spot } => {
            view.place_read_marker(pane, spot, now);
            view.session().wake();
        }
        Command::ReadAloud { pane, from } => {
            // Reading from elsewhere sends a waiting pin off.
            view.dismiss_read_markers(now);
            view.read_aloud(pane, from);
        }
        Command::ToggleReadAloud => {
            view.dismiss_read_markers(now);
            if !view.toggle_read_aloud() {
                let active = view.session().active();
                view.read_aloud(active, ReadFrom::default());
            }
        }
        Command::StopReadAloud => view.stop_read_aloud(),
        Command::RevealVoice(model, language) => view.reveal_voice(model, language),
        Command::VoiceModel(item, action) => model(view, item, action),
        other => say(view, other),
    }
}

fn say(view: &mut dyn View, intent: Command) {
    match intent {
        Command::TryVoice => view.try_voice(),
        Command::TryVoiceOf(voice) => view.try_voice_of(&voice),
        Command::StopVoiceTry => view.stop_voice_try(),
        Command::SayWord(word) => heard(view, &word, None),
        Command::SayWordWith { word, voice } => heard(view, &word, Some(&voice)),
        Command::StopSaying => view.stop_saying(),
        _ => {}
    }
}

fn heard(view: &mut dyn View, word: &str, voice: Option<&str>) {
    if let Err(why) = view.say_word(word, voice) {
        note(view, Toast::error(why));
    }
}

/// A download may change the voice picked, so the settings follow it.
fn model(view: &mut dyn View, item: Item, action: Download) {
    let mut settings = view.session().settings().read_aloud.clone();
    match view.voice_model(item, action, &mut settings) {
        Ok(()) => set(view, "read_aloud", serde_json::to_string(&settings)),
        Err(why) => note(view, Toast::error(why)),
    }
}
