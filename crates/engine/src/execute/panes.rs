//! Panes, and the pages in them: splits, focus, opening, turning.

use mareader_settings::EdgeParking;
use mareader_workspace::{Edge, PaneId, SplitAxis};

use super::note;
use crate::command::Command;
use crate::desk::Desk;
use crate::documents::{OpenRequest, Placement};
use crate::event::Event;
use crate::route::Route;
use crate::toast::Toast;

pub(super) fn run(desk: &mut dyn Desk, intent: Command, now: f64) {
    match intent {
        Command::Split(axis) => split(desk, axis),
        Command::FocusPane(id) => desk.session().focus(id),
        Command::CyclePane { forward } => cycle(desk, forward),
        Command::ClosePane(id) => {
            if desk.session().close(id) {
                desk.close_pane(id);
            }
        }
        Command::SetRatio(..) => {
            desk.session().apply(intent);
        }
        Command::Open(request) => open(desk, request),
        Command::Export {
            name,
            format,
            contents,
        } => desk.save(name, format, contents),
        Command::CopyPages { pane, pages } => desk.copy_pages(pane, pages),
        other => pages(desk, other, now),
    }
}

/// What moves the page a pane shows; most act on the focused pane.
fn pages(desk: &mut dyn Desk, intent: Command, now: f64) {
    let active = desk.session().active();
    match intent {
        Command::Zoom(zoom) => desk.zoom(active, zoom, now),
        Command::View(mode) => desk.set_mode(active, mode, now),
        Command::Arrow(arrow) => desk.arrow(active, arrow, now),
        Command::Page { forward } => desk.page_key(active, forward),
        Command::AutoScroll => {
            if !desk.toggle_drift(active, now) {
                let why = "Auto-scroll runs in the scrolling views";
                note(desk, Toast::info(why));
            }
        }
        Command::FirstPage => desk.go_to(active, 0),
        Command::LastPage => desk.go_to(active, usize::MAX),
        Command::GoTo { pane, page } => desk.go_to(pane, page),
        Command::Turn { pane, forward } => desk.turn(pane, forward),
        _ => {}
    }
}

/// The new pane takes the focus, as a split asks it to.
fn split(desk: &mut dyn Desk, axis: SplitAxis) {
    let edge = match axis {
        SplitAxis::Horizontal => Edge::Right,
        SplitAxis::Vertical => Edge::Bottom,
    };
    let area = desk.area();
    let session = desk.session();
    if let Some(id) = session.split_at(session.active(), edge, area) {
        session.focus(id);
    }
}

/// F6: the next pane in reading order, by way of the parked tab if shown.
fn cycle(desk: &mut dyn Desk, forward: bool) {
    let tab = desk.edge_tab();
    let session = desk.session();
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

fn open(desk: &mut dyn Desk, request: OpenRequest) {
    // Held while the library loads, to open where it was left.
    let Some(request) = desk.resume(request) else {
        return;
    };
    go(desk, Route::Reader);
    let area = desk.area();
    let session = desk.session();
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
        if let Some(toast) = desk.open(pane, request) {
            note(desk, toast);
        }
    }
}

/// The one way between routes; the page left behind is told.
pub(super) fn go(desk: &mut dyn Desk, route: Route) {
    desk.session().show(route);
    desk.arrived(route);
}

/// The panes showing one of `books`, or a file at one of `paths`.
pub(super) fn showing(
    desk: &mut dyn Desk,
    books: &[mareader_library::BookId],
    paths: &[std::path::PathBuf],
) -> Vec<PaneId> {
    let panes = desk.session().tree().panes();
    let shows = |pane: &PaneId| match desk.book(*pane) {
        Some(book) => books.contains(&book),
        None => desk.path(*pane).is_some_and(|open| paths.contains(&open)),
    };
    panes.into_iter().filter(shows).collect()
}
