//! The library's books as panes take them.

use mareader_settings::LibraryClick;

use super::note;
use super::panes::showing;
use crate::command::Command;
use crate::documents::Placement;
use crate::view::View;

pub(super) fn run(view: &mut dyn View, intent: Command) {
    match intent {
        Command::OpenBook(book) => {
            // Do nothing is for the rail; the page has nothing to drag to.
            let click = view.session().settings().workspace.library_click;
            let placement = match click {
                LibraryClick::Split => Placement::Beside,
                LibraryClick::Replace | LibraryClick::DragOnly => Placement::Into,
            };
            view.open_book(book, None, placement);
        }
        Command::PlaceBook {
            book,
            pane,
            placement,
        } => view.open_book(book, pane, placement),
        Command::Library(action) => view.library(action),
        Command::ForgetPositions { books, paths } => {
            let panes = showing(view, &books, &paths);
            for toast in view.restart_at_start(&panes) {
                note(view, toast);
            }
        }
        _ => {}
    }
}
