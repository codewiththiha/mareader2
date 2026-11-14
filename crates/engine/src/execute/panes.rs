//! Panes, and the pages in them: splits, focus, opening, turning.

use mareader_settings::EdgeParking;
use mareader_workspace::{Edge, PaneId, SplitAxis};

use super::note;
use crate::command::Command;
use crate::documents::{OpenRequest, Placement};
use crate::event::Event;
use crate::route::Route;
use crate::toast::Toast;
use crate::view::View;

pub(super) fn run(view: &mut dyn View, intent: Command, now: f64) {
    match intent {
        Command::Split(axis) => split(view, axis),
        Command::FocusPane(id) => view.session().focus(id),
        Command::CyclePane { forward } => cycle(view, forward),
        Command::ClosePane(id) => {
            if view.session().close(id) {
                view.close_pane(id);
            }
        }
        Command::SetRatio(..) => {
            view.session().apply(intent);
        }
        Command::Open(request) => open(view, request),
        Command::Export {
            name,
            format,
            contents,
        } => view.save(name, format, contents),
        Command::CopyPages { pane, pages } => view.copy_pages(pane, pages),
        other => pages(view, other, now),
    }
}

/// What moves the page a pane shows; most act on the focused pane.
fn pages(view: &mut dyn View, intent: Command, now: f64) {
    let active = view.session().active();
    match intent {
        Command::Zoom(zoom) => view.zoom(active, zoom, now),
        Command::View(mode) => view.set_mode(active, mode, now),
        Command::Arrow(arrow) => view.arrow(active, arrow, now),
        Command::Page { forward } => view.page_key(active, forward),
        Command::AutoScroll => {
            if !view.toggle_drift(active, now) {
                let why = "Auto-scroll runs in the scrolling views";
                note(view, Toast::info(why));
            }
        }
        Command::FirstPage => view.go_to(active, 0),
        Command::LastPage => view.go_to(active, usize::MAX),
        Command::GoTo { pane, page } => view.go_to(pane, page),
        Command::Turn { pane, forward } => view.turn(pane, forward),
        _ => {}
    }
}

/// The new pane takes the focus, as a split asks it to.
fn split(view: &mut dyn View, axis: SplitAxis) {
    let edge = match axis {
        SplitAxis::Horizontal => Edge::Right,
        SplitAxis::Vertical => Edge::Bottom,
    };
    let area = view.area();
    let session = view.session();
    if let Some(id) = session.split_at(session.active(), edge, area) {
        session.focus(id);
    }
}

/// F6: the next pane in reading order, by way of the parked tab if shown.
fn cycle(view: &mut dyn View, forward: bool) {
    let tab = view.edge_tab();
    let session = view.session();
    let panes = session.tree().panes();
    let active = session.active();
    let at = panes.iter().position(|p| *p == active).unwrap_or(0);
    let n = panes.len().max(1);
    let parked = session.settings().workspace.edge_parking == EdgeParking::Tab;
    let ends = (forward && at + 1 == n) || (!forward && at == 0);
    if parked && tab.shown && !tab.focused && ends {
        session.emit(Event::FocusEdgeTab);
        return;
    }
    let next = match (tab.focused, forward) {
        (true, true) => 0,
        (true, false) => n - 1,
        (false, true) => (at + 1) % n,
        (false, false) => (at + n - 1) % n,
    };
    if let Some(pane) = panes.get(next) {
        session.focus(*pane);
    }
    session.emit(Event::ReleaseFocus);
}

fn open(view: &mut dyn View, request: OpenRequest) {
    // Held while the library loads, to open where it was left.
    let Some(request) = view.resume(request) else {
        return;
    };
    go(view, Route::Reader);
    let area = view.area();
    let session = view.session();
    let mut pane = request.pane.unwrap_or(session.active());
    let edge = match request.placement {
        Placement::Into => None,
        Placement::Beside => Some(session.beside(pane, area)),
        Placement::Edge(edge) => Some(edge),
    };
    if let Some(edge) = edge {
        match session.split_at(pane, edge, area) {
            Some(new) => pane = new,
            None => return,
        }
    }
    if session.tree().panes().contains(&pane) {
        session.focus(pane);
        if let Some(toast) = view.open(pane, request) {
            note(view, toast);
        }
    }
}

/// The one way between routes; the page left behind is told.
pub(super) fn go(view: &mut dyn View, route: Route) {
    view.session().show(route);
    view.arrived(route);
}

/// The panes showing one of `books`, or a file at one of `paths`.
pub(super) fn showing(
    view: &mut dyn View,
    books: &[mareader_library::BookId],
    paths: &[std::path::PathBuf],
) -> Vec<PaneId> {
    let panes = view.session().tree().panes();
    let shows = |pane: &PaneId| match view.book(*pane) {
        Some(book) => books.contains(&book),
        None => view.path(*pane).is_some_and(|open| paths.contains(&open)),
    };
    panes.into_iter().filter(shows).collect()
}
