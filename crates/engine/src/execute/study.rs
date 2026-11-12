//! Study tools: marks, the word card, word lists and the dictionaries.

use super::note;
use crate::command::Command;
use crate::desk::Desk;
use crate::dict::Action as DictAction;
use crate::toast::Toast;

pub(super) fn run(desk: &mut dyn Desk, intent: Command) {
    match intent {
        Command::WordList { pane, pages } => {
            let list = desk.session().settings().study.words.instant().clone();
            desk.word_list(pane, pages, list);
        }
        Command::WordListWindow { pane, pages } => {
            let list = desk.session().settings().study.words.last.clone();
            desk.word_window(pane, pages, list);
        }
        Command::Study(action) => desk.study(action),
        Command::Explain(pane) => desk.explain(pane),
        Command::LookUp(pane) => desk.look_up(pane),
        Command::Highlight { pane, pen, color } => desk.highlight(pane, pen, color),
        Command::Note(pane) => desk.note(pane),
        Command::Bookmark(pane) => {
            let pane = pane.unwrap_or(desk.session().active());
            desk.bookmark(pane);
        }
        Command::SelectText { pane, page, text } => desk.select(pane, page, &text),
        other => words(desk, other),
    }
}

/// The dictionaries, the word levels and how words sound.
fn words(desk: &mut dyn Desk, intent: Command) {
    match intent {
        Command::Dict(DictAction::Window(window)) => desk.dict_window(window),
        Command::Dict(DictAction::Pursue(word)) => desk.pursue(&word),
        Command::Dict(action) => desk.dict(action),
        Command::Vocab(action) => desk.vocab(action),
        Command::Pronunciation(action) => {
            if let Err(why) = desk.pronunciation(action) {
                note(desk, Toast::error(why));
            }
        }
        _ => {}
    }
}
