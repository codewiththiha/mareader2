//! Study tools: marks, the word card, word lists and the dictionaries.

use super::note;
use crate::command::Intent;
use crate::desk::Desk;
use crate::dict::Action as DictAction;
use crate::toast::Toast;

pub(super) fn run(desk: &mut dyn Desk, intent: Intent) {
    match intent {
        Intent::WordList { pane, pages } => {
            let list = desk.session().settings().study.words.instant().clone();
            desk.word_list(pane, pages, list);
        }
        Intent::WordListWindow { pane, pages } => {
            let list = desk.session().settings().study.words.last.clone();
            desk.word_window(pane, pages, list);
        }
        Intent::Study(action) => desk.study(action),
        Intent::Explain(pane) => desk.explain(pane),
        Intent::LookUp(pane) => desk.look_up(pane),
        Intent::Highlight { pane, pen, color } => desk.highlight(pane, pen, color),
        Intent::Note(pane) => desk.note(pane),
        Intent::Bookmark(pane) => {
            let pane = pane.unwrap_or(desk.session().active());
            desk.bookmark(pane);
        }
        Intent::SelectText { pane, page, text } => desk.select(pane, page, &text),
        other => words(desk, other),
    }
}

/// The dictionaries, the word levels and how words sound.
fn words(desk: &mut dyn Desk, intent: Intent) {
    match intent {
        Intent::Dict(DictAction::Window(window)) => desk.dict_window(window),
        Intent::Dict(DictAction::Pursue(word)) => desk.pursue(&word),
        Intent::Dict(action) => desk.dict(action),
        Intent::Vocab(action) => desk.vocab(action),
        Intent::Pronunciation(action) => {
            if let Err(why) = desk.pronunciation(action) {
                note(desk, Toast::error(why));
            }
        }
        _ => {}
    }
}
