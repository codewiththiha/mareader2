//! Panes, and the pages in them: splits, focus, opening, turning.

use mareader_settings::EdgeParking;
use mareader_workspace::{Edge, PaneId, SplitAxis};

use super::note;
use crate::command::Command;
use crate::documents::{OpenRequest, Placement};
use crate::event::Event;
use crate::host::Host;
use crate::route::Route;
use crate::toast::Toast;

pub(super) fn run(host: &mut dyn Host, intent: Command, now: f64) {
    match intent {
        Command::Split(axis) => split(host, axis),
        Command::FocusPane(id) => host.session().focus(id),
        Command::CyclePane { forward } => cycle(host, forward),
        Command::ClosePane(id) => {
            if host.session().close(id) {
                host.close_pane(id);
            }
        }
        Command::SetRatio(..) => {
            host.session().apply(intent);
        }
        Command::Open(request) => open(host, request),
        Command::Export {
            name,
            format,
            contents,
        } => host.save(name, format, contents),
        Command::CopyPages { pane, pages } => host.copy_pages(pane, pages),
        other => pages(host, other, now),
    }
}

/// What moves the page a pane shows; most act on the focused pane.
fn pages(host: &mut dyn Host, intent: Command, now: f64) {
    let active = host.session().active();
    match intent {
        Command::Zoom(zoom) => host.zoom(active, zoom, now),
        Command::View(mode) => host.set_mode(active, mode, now),
        Command::Arrow(arrow) => host.arrow(active, arrow, now),
        Command::Page { forward } => host.page_key(active, forward),
        Command::AutoScroll => {
            if !host.toggle_drift(active, now) {
                let why = "Auto-scroll runs in the scrolling views";
                note(host, Toast::info(why));
            }
        }
        Command::FirstPage => host.go_to(active, 0),
        Command::LastPage => host.go_to(active, usize::MAX),
        Command::GoTo { pane, page } => host.go_to(pane, page),
        Command::Turn { pane, forward } => host.turn(pane, forward),
        _ => {}
    }
}

/// The new pane takes the focus, as a split asks it to.
fn split(host: &mut dyn Host, axis: SplitAxis) {
    let edge = match axis {
        SplitAxis::Horizontal => Edge::Right,
        SplitAxis::Vertical => Edge::Bottom,
    };
    let area = host.area();
    let session = host.session();
    if let Some(id) = session.split_at(session.active(), edge, area) {
        session.focus(id);
    }
}

/// F6: the next pane in reading order, by way of the parked tab if shown.
fn cycle(host: &mut dyn Host, forward: bool) {
    let tab = host.edge_tab();
    let session = host.session();
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

fn open(host: &mut dyn Host, request: OpenRequest) {
    // Held while the library loads, to open where it was left.
    let Some(request) = host.resume(request) else {
        return;
    };
    go(host, Route::Reader);
    let area = host.area();
    let session = host.session();
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
        if let Some(toast) = host.open(pane, request) {
            note(host, toast);
        }
    }
}

/// The one way between routes; the page left behind is told.
pub(super) fn go(host: &mut dyn Host, route: Route) {
    host.session().show(route);
    host.arrived(route);
}

/// The panes showing one of `books`, or a file at one of `paths`.
pub(super) fn showing(
    host: &mut dyn Host,
    books: &[mareader_library::BookId],
    paths: &[std::path::PathBuf],
) -> Vec<PaneId> {
    let panes = host.session().tree().panes();
    let shows = |pane: &PaneId| match host.book(*pane) {
        Some(book) => books.contains(&book),
        None => host.path(*pane).is_some_and(|open| paths.contains(&open)),
    };
    panes.into_iter().filter(shows).collect()
}
