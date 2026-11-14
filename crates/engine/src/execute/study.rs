//! Study tools: marks, the word card, word lists and the dictionaries.

use super::note;
use crate::command::Command;
use crate::dict::Action as DictAction;
use crate::toast::Toast;
use crate::view::View;

pub(super) fn run(view: &mut dyn View, intent: Command) {
    match intent {
        Command::WordList { pane, pages } => {
            let list = view.session().settings().study.words.instant().clone();
            view.word_list(pane, pages, list);
        }
        Command::WordListWindow { pane, pages } => {
            let list = view.session().settings().study.words.last.clone();
            view.word_window(pane, pages, list);
        }
        Command::Study(action) => view.study(action),
        Command::Explain(pane) => view.explain(pane),
        Command::LookUp(pane) => view.look_up(pane),
        Command::Highlight { pane, pen, color } => view.highlight(pane, pen, color),
        Command::Note(pane) => view.note(pane),
        Command::Bookmark(pane) => {
            let pane = pane.unwrap_or(view.session().active());
            view.bookmark(pane);
        }
        Command::SelectText { pane, page, text } => view.select(pane, page, &text),
        other => words(view, other),
    }
}

/// The dictionaries, the word levels and how words sound.
fn words(view: &mut dyn View, intent: Command) {
    match intent {
        Command::Dict(DictAction::Window(window)) => view.dict_window(window),
        Command::Dict(DictAction::Pursue(word)) => view.pursue(&word),
        Command::Dict(action) => view.dict(action),
        Command::Vocab(action) => view.vocab(action),
        Command::Pronunciation(action) => {
            if let Err(why) = view.pronunciation(action) {
                note(view, Toast::error(why));
            }
        }
        _ => {}
    }
}
