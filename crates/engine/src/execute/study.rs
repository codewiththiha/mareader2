//! Study tools: marks, the word card, word lists and the dictionaries.

use super::note;
use crate::command::Command;
use crate::dict::Action as DictAction;
use crate::host::Host;
use crate::toast::Toast;

pub(super) fn run(host: &mut dyn Host, intent: Command) {
    match intent {
        Command::WordList { pane, pages } => {
            let list = host.session().settings().study.words.instant().clone();
            host.word_list(pane, pages, list);
        }
        Command::WordListWindow { pane, pages } => {
            let list = host.session().settings().study.words.last.clone();
            host.word_window(pane, pages, list);
        }
        Command::Study(action) => host.study(action),
        Command::Explain(pane) => host.explain(pane),
        Command::LookUp(pane) => host.look_up(pane),
        Command::Highlight { pane, pen, color } => host.highlight(pane, pen, color),
        Command::Note(pane) => host.note(pane),
        Command::Bookmark(pane) => {
            let pane = pane.unwrap_or(host.session().active());
            host.bookmark(pane);
        }
        Command::SelectText { pane, page, text } => host.select(pane, page, &text),
        other => words(host, other),
    }
}

/// The dictionaries, the word levels and how words sound.
fn words(host: &mut dyn Host, intent: Command) {
    match intent {
        Command::Dict(DictAction::Window(window)) => host.dict_window(window),
        Command::Dict(DictAction::Pursue(word)) => host.pursue(&word),
        Command::Dict(action) => host.dict(action),
        Command::Vocab(action) => host.vocab(action),
        Command::Pronunciation(action) => {
            if let Err(why) = host.pronunciation(action) {
                note(host, Toast::error(why));
            }
        }
        _ => {}
    }
}
