//! The library's books as panes take them.

use mareader_settings::LibraryClick;

use super::note;
use super::panes::showing;
use crate::command::Command;
use crate::documents::Placement;
use crate::host::Host;

pub(super) fn run(host: &mut dyn Host, intent: Command) {
    match intent {
        Command::OpenBook(book) => {
            // Do nothing is for the rail; the page has nothing to drag to.
            let click = host.session().settings().workspace.library_click;
            let placement = match click {
                LibraryClick::Split => Placement::Beside,
                LibraryClick::Replace | LibraryClick::DragOnly => Placement::Into,
            };
            host.open_book(book, None, placement);
        }
        Command::PlaceBook {
            book,
            pane,
            placement,
        } => host.open_book(book, pane, placement),
        Command::Library(action) => host.library(action),
        Command::ForgetPositions { books, paths } => {
            let panes = showing(host, &books, &paths);
            for toast in host.restart_at_start(&panes) {
                note(host, toast);
            }
        }
        _ => {}
    }
}
