//! The library's books as panes take them.

use mareader_settings::LibraryClick;

use super::note;
use super::panes::showing;
use crate::command::Intent;
use crate::desk::Desk;
use crate::documents::Placement;

pub(super) fn run(desk: &mut dyn Desk, intent: Intent) {
    match intent {
        Intent::OpenBook(book) => {
            // Do nothing is for the rail; the page has nothing to drag to.
            let click = desk.session().settings().workspace.library_click;
            let placement = match click {
                LibraryClick::Split => Placement::Beside,
                LibraryClick::Replace | LibraryClick::DragOnly => Placement::Into,
            };
            desk.open_book(book, None, placement);
        }
        Intent::PlaceBook {
            book,
            pane,
            placement,
        } => desk.open_book(book, pane, placement),
        Intent::Library(action) => desk.library(action),
        Intent::ForgetPositions { books, paths } => {
            let panes = showing(desk, &books, &paths);
            for toast in desk.restart_at_start(&panes) {
                note(desk, toast);
            }
        }
        _ => {}
    }
}
