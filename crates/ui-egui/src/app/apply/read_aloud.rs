use super::*;
use crate::command::{Intent, UiCommand};

pub(super) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::Intent(Intent::PlaceReadMarker { .. })
            | Command::Intent(Intent::ReadAloud { .. })
            | Command::Intent(Intent::ToggleReadAloud)
            | Command::Intent(Intent::StopReadAloud)
            | Command::Intent(Intent::TryVoice)
            | Command::Intent(Intent::TryVoiceOf(..))
            | Command::Intent(Intent::StopVoiceTry)
            | Command::Intent(Intent::SayWord(..))
            | Command::Intent(Intent::SayWordWith { .. })
            | Command::Intent(Intent::RevealVoice(..))
            | Command::Intent(Intent::StopSaying)
            | Command::Intent(Intent::VoiceModel(..))
            | Command::Ui(UiCommand::ShowReading)
    )
}

pub(super) fn apply(app: &mut MareaderApp, ctx: &Context, command: Command) {
    super::two_stage(app, ctx, command, handles_1, apply_1, handles_2, apply_2);
}

fn handles_1(command: &Command) -> bool {
    matches!(
        command,
        Command::Intent(Intent::PlaceReadMarker { .. })
            | Command::Intent(Intent::ReadAloud { .. })
            | Command::Intent(Intent::ToggleReadAloud)
            | Command::Intent(Intent::StopReadAloud)
            | Command::Intent(Intent::TryVoice)
            | Command::Intent(Intent::TryVoiceOf(..))
            | Command::Intent(Intent::StopVoiceTry)
            | Command::Intent(Intent::SayWord(..))
            | Command::Intent(Intent::SayWordWith { .. })
    )
}

fn apply_1(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        Command::Intent(Intent::PlaceReadMarker { pane, spot }) => {
            let now = ctx.input(|i| i.time);
            app.documents.place_read_marker(pane, spot, now);
            ctx.request_repaint();
        }
        Command::Intent(Intent::ReadAloud { pane, from }) => {
            // Reading from elsewhere sends a waiting pin off.
            app.documents.dismiss_read_markers(ctx.input(|i| i.time));
            if let Some(reading) = app.documents.reading(pane) {
                app.clips.stop();
                let settings = app.settings().read_aloud.clone();
                app.read_aloud.start(ctx, &reading, from, &settings);
            }
        }
        Command::Intent(Intent::ToggleReadAloud) => {
            app.documents.dismiss_read_markers(ctx.input(|i| i.time));
            if !app.read_aloud.toggle()
                && let Some(reading) = app.documents.reading(app.active())
            {
                app.clips.stop();
                let settings = app.settings().read_aloud.clone();
                let from = ReadFrom::default();
                app.read_aloud.start(ctx, &reading, from, &settings);
            }
        }
        Command::Intent(Intent::StopReadAloud) => app.read_aloud.stop(),
        Command::Intent(Intent::TryVoice) => {
            app.hush();
            let settings = app.settings().read_aloud.clone();
            app.read_aloud.booth.try_voice(&settings);
        }
        Command::Intent(Intent::TryVoiceOf(voice)) => {
            app.hush();
            let settings = app.settings().read_aloud.clone();
            let (booth, shelf) = (&mut app.read_aloud.booth, &app.read_aloud.shelf);
            booth.try_voice_of(ctx, &settings, shelf, &voice);
        }
        Command::Intent(Intent::StopVoiceTry) => app.read_aloud.booth.stop_trying(),
        Command::Intent(Intent::SayWord(word)) => {
            app.hush();
            let settings = app.settings().read_aloud.clone();
            if let Err(why) = app.read_aloud.say_word(ctx, &settings, (&word, None)) {
                app.toasts.push(Toast::error(why));
            }
        }
        Command::Intent(Intent::SayWordWith { word, voice }) => {
            app.hush();
            let settings = app.settings().read_aloud.clone();
            let said = (word.as_str(), Some(voice.as_str()));
            if let Err(why) = app.read_aloud.say_word(ctx, &settings, said) {
                app.toasts.push(Toast::error(why));
            }
        }
        _ => {}
    }
}

fn handles_2(command: &Command) -> bool {
    matches!(
        command,
        Command::Intent(Intent::RevealVoice(..))
            | Command::Intent(Intent::StopSaying)
            | Command::Intent(Intent::VoiceModel(..))
            | Command::Ui(UiCommand::ShowReading)
    )
}

fn apply_2(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        Command::Intent(Intent::RevealVoice(model, language)) => {
            app.open_settings(Some(Tab::Model(model)));
            let fold = crate::settings_window::voice_fold(model, language);
            crate::settings_window::reveal(ctx, fold);
        }
        Command::Intent(Intent::StopSaying) => app.read_aloud.stop_word(),
        Command::Intent(Intent::VoiceModel(item, action)) => {
            let mut settings = app.settings().read_aloud.clone();
            match app.read_aloud.act((item, action), &mut settings) {
                Ok(()) => {
                    let _ = app.set_field("read_aloud", settings);
                }
                Err(why) => app.toasts.push(Toast::error(why)),
            }
        }
        Command::Ui(UiCommand::ShowReading) => {
            if let Some((pane, page, _)) = app.read_aloud.spot() {
                if app.route() != Route::Reader {
                    app.go(Route::Reader);
                }
                app.focus(pane);
                app.documents.go_to(pane, page);
            } else if let Some(origin) = app.read_aloud.away().cloned() {
                app.reopen_reading(ctx, origin);
            }
        }
        _ => {}
    }
}
