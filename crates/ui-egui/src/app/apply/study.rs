use super::*;
use crate::app::navigation::found_in;

pub(super) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::Intent(Intent::WordList { .. })
            | Command::Intent(Intent::WordListWindow { .. })
            | Command::Intent(Intent::Study(..))
            | Command::Intent(Intent::Explain(..))
            | Command::Intent(Intent::LookUp(..))
            | Command::Intent(Intent::Dict(..))
            | Command::Intent(Intent::Vocab(..))
            | Command::Intent(Intent::Pronunciation(..))
            | Command::Intent(Intent::SetDictionary(..))
            | Command::Intent(Intent::Highlight { .. })
            | Command::Intent(Intent::Note(..))
            | Command::Intent(Intent::Bookmark(..))
            | Command::Intent(Intent::SelectText { .. })
            | Command::Ui(UiCommand::FindStep { .. })
            | Command::Ui(UiCommand::ShowFind)
            | Command::Ui(UiCommand::ShowSearch)
            | Command::Ui(UiCommand::ShowDictionary)
    )
}

pub(super) fn apply(app: &mut MareaderApp, ctx: &Context, command: Command) {
    super::two_stage(app, ctx, command, handles_1, apply_1, handles_2, apply_2);
}

fn handles_1(command: &Command) -> bool {
    matches!(
        command,
        Command::Intent(Intent::WordList { .. })
            | Command::Intent(Intent::WordListWindow { .. })
            | Command::Intent(Intent::Study(..))
            | Command::Intent(Intent::Explain(..))
            | Command::Intent(Intent::LookUp(..))
            | Command::Intent(Intent::Dict(..))
            | Command::Intent(Intent::Vocab(..))
            | Command::Intent(Intent::Pronunciation(..))
            | Command::Intent(Intent::SetDictionary(..))
            | Command::Intent(Intent::Highlight { .. })
            | Command::Intent(Intent::Note(..))
            | Command::Intent(Intent::Bookmark(..))
            | Command::Intent(Intent::SelectText { .. })
    )
}

fn apply_1(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        Command::Intent(Intent::WordList { pane, pages }) => {
            let list = app.settings.study.words.instant().clone();
            if let Some(ask) = app.word_ask(pane, pages, list) {
                app.word_lists.start(ctx, ask);
            }
        }
        Command::Intent(Intent::WordListWindow { pane, pages }) => {
            let list = app.settings.study.words.last.clone();
            if let Some(ask) = app.word_ask(pane, pages, list) {
                let chapters = !ask.document.outline().is_empty();
                app.word_window.open(ctx, pane, ask, chapters);
            }
        }
        Command::Intent(Intent::Study(action)) => {
            let mut out = Commands::new();
            let model = app.library.model_mut();
            app.study.act(ctx, model, action, &mut out);
            app.queued.extend(out);
        }
        Command::Intent(Intent::Explain(pane)) => app.explain(ctx, pane),
        Command::Intent(Intent::LookUp(pane)) => app.look_up(pane),
        Command::Intent(Intent::Dict(crate::dict::Action::Window(w))) => app.dict_window(w),
        Command::Intent(Intent::Dict(crate::dict::Action::Pursue(word))) => app.pursue(&word),
        Command::Intent(Intent::Dict(action)) => app.dict.act(action),
        Command::Intent(Intent::Vocab(action)) => app.vocab.act(action),
        Command::Intent(Intent::Pronunciation(action)) => {
            if let Err(why) = app.sounds.act(action) {
                app.toasts.push(Toast::error(why));
            }
        }
        Command::Intent(Intent::SetDictionary(d)) => {
            app.settings.dictionary = *d;
            app.settings.dictionary.sanitize();
        }
        Command::Intent(Intent::Highlight { pane, pen, color }) => app.highlight(pane, pen, color),
        Command::Intent(Intent::Note(pane)) => app.note(pane),
        Command::Intent(Intent::Bookmark(pane)) => app.bookmark(pane.unwrap_or(app.active)),
        Command::Intent(Intent::SelectText { pane, page, text }) => {
            app.documents.select(pane, page, &text);
        }
        _ => {}
    }
}

fn handles_2(command: &Command) -> bool {
    matches!(
        command,
        Command::Ui(UiCommand::FindStep { .. })
            | Command::Ui(UiCommand::ShowFind)
            | Command::Ui(UiCommand::ShowSearch)
            | Command::Ui(UiCommand::ShowDictionary)
    )
}

fn apply_2(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        // With nothing to search in, find searches the library.
        Command::Ui(UiCommand::FindStep { forward }) if app.find.is_open() => {
            let found = found_in(&app.search, &app.documents, &app.find);
            app.find.key_step(found, forward, ctx.input(|i| i.time));
        }
        Command::Ui(UiCommand::FindStep { .. }) => app.apply(ctx, Command::Ui(UiCommand::ShowFind)),
        Command::Ui(UiCommand::ShowFind) => match (app.route(), app.documents.reading(app.active)) {
            // The page's own field is its find.
            (Route::Dictionary, _) => app.dict.arrive(),
            (Route::Reader, Some(_)) => {
                app.find.open(app.active);
                app.titlebar.reveal();
            }
            _ => {
                app.go(Route::Library);
                let focus = ui_library::SearchAction::Focus;
                app.apply(ctx, Command::Intent(Intent::Library(ui_library::Action::Search(focus))));
            }
        },
        Command::Ui(UiCommand::ShowSearch) if app.find.is_open() => app.find.close(),
        Command::Ui(UiCommand::ShowSearch) => app.apply(ctx, Command::Ui(UiCommand::ShowFind)),
        Command::Ui(UiCommand::ShowDictionary) => match app.route() {
            Route::Dictionary => app.go(app.dict_back()),
            // Over the page, unless Settings ask for the page.
            Route::Reader if app.settings.dictionary.opens == DictionaryOpens::Window => {
                app.dict_window(crate::dict::Window::Toggle);
            }
            _ => app.go(Route::Dictionary),
        },
        _ => {}
    }
}
