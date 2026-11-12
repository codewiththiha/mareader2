//! A script replayed against the engine alone: no window, no toolkit, no frame.
//!
//! Open the sample PDF, split, go to a page, highlight, export the notes:
//! every step is an command through `Session::execute`, and what a controller
//! would do in the app is done by `common::Headless` with real documents.

mod common;

use mareader_engine::command::Command;
use mareader_engine::command_spec::SPECS;
use mareader_engine::documents::OpenRequest;
use mareader_engine::event::Event;
use mareader_engine::export::{Format, Sheet, render};
use mareader_engine::route::Route;
use mareader_engine::session::Session;
use mareader_library::Pen;
use mareader_workspace::SplitAxis;

use common::{Headless, export_name};

const PAGES: usize = 6;

fn script(out: &std::path::Path) -> Headless {
    let mut desk = Headless::new(out.to_path_buf());
    let sample = mareader_pdf::sample::pdf(PAGES, "Sample");
    let open = Command::Open(OpenRequest::bytes(sample, "Sample.pdf"));
    Session::execute(&mut desk, open, 0.0);
    desk
}

#[test]
fn a_script_opens_splits_turns_marks_and_exports_with_no_window() {
    let out = tempfile::tempdir().expect("a scratch folder");
    let mut desk = script(out.path());
    let first = desk.session.active();
    assert_eq!(
        desk.panes.get(&first).map(|o| o.doc.page_count()),
        Some(PAGES)
    );

    Session::execute(&mut desk, Command::Split(SplitAxis::Horizontal), 1.0);
    let panes = desk.session.tree().panes();
    assert_eq!(panes.len(), 2, "a split makes a pane");
    let second = desk.session.active();
    assert_ne!(first, second, "and focuses it");
    assert_eq!(desk.session.recent().first(), Some(&second));

    Session::execute(&mut desk, Command::FocusPane(first), 2.0);
    let go = Command::GoTo {
        pane: first,
        page: 3,
    };
    Session::execute(&mut desk, go, 3.0);
    assert_eq!(desk.panes.get(&first).map(|o| o.page), Some(3));

    let text = desk.page_text(first);
    let word = text.split_whitespace().find(|w| w.len() > 2);
    let word = word.expect("the sample page has words").to_owned();
    let select = Command::SelectText {
        pane: first,
        page: 3,
        text: word.clone(),
    };
    Session::execute(&mut desk, select, 4.0);
    let mark = Command::Highlight {
        pane: first,
        pen: Pen::default(),
        color: None,
    };
    Session::execute(&mut desk, mark, 5.0);
    assert_eq!(desk.marks.len(), 1, "the selection became a mark");
    assert_eq!(desk.marks.first().map(|m| m.page), Some(Some(3)));

    let sheet = Sheet {
        title: "Sample",
        entries: &desk.marks,
        now_ms: 0,
    };
    let name = export_name("Sample", Format::Markdown);
    let export = Command::Export {
        name: name.clone(),
        format: Format::Markdown,
        contents: render(&sheet, Format::Markdown),
    };
    Session::execute(&mut desk, export, 6.0);
    let written = std::fs::read_to_string(out.path().join(name)).expect("the export");
    assert!(written.contains(&word), "the notes carry the quote");
    assert!(desk.session.drain().is_empty(), "nothing was refused");
}

#[test]
fn the_library_page_drops_what_acts_on_panes() {
    let out = tempfile::tempdir().expect("a scratch folder");
    let mut desk = script(out.path());
    let pane = desk.session.active();
    desk.session.show(Route::Library);
    Session::execute(&mut desk, Command::GoTo { pane, page: 4 }, 0.0);
    assert_eq!(desk.panes.get(&pane).map(|o| o.page), Some(0));
    Session::execute(&mut desk, Command::Split(SplitAxis::Horizontal), 0.0);
    assert_eq!(desk.session.tree().panes().len(), 1);
}

#[test]
fn a_refused_split_and_a_failure_leave_as_toasts() {
    let out = tempfile::tempdir().expect("a scratch folder");
    let mut desk = script(out.path());
    for _ in 0..8 {
        Session::execute(&mut desk, Command::Split(SplitAxis::Horizontal), 0.0);
    }
    let said = desk.session.drain();
    assert!(!said.is_empty(), "the workspace is full, and says so");
    assert!(said.iter().all(|e| matches!(e, Event::Toast(_))));
    assert!(desk.session.tree().panes().len() <= mareader_workspace::MAX_PANES);

    let ask = Command::Pronunciation(mareader_engine::dict::Download::Start);
    Session::execute(&mut desk, ask, 0.0);
    let said = desk.session.drain();
    assert!(matches!(said.as_slice(), [Event::Toast(t)] if t.message.contains("pronunciation")));
}

#[test]
fn opening_shows_the_reader_and_a_dial_is_written_by_its_path() {
    let out = tempfile::tempdir().expect("a scratch folder");
    let mut desk = script(out.path());
    assert_eq!(desk.arrivals, vec![Route::Reader]);
    let set = Command::SetField {
        path: "chrome.sidebar_overlay".into(),
        value: "true".into(),
    };
    Session::execute(&mut desk, set, 0.0);
    assert!(desk.session.settings().chrome.sidebar_overlay);
    Session::execute(&mut desk, Command::SetSidebarOverlay(false), 0.0);
    assert!(!desk.session.settings().chrome.sidebar_overlay);
}

#[test]
fn the_registry_lists_every_intent_once() {
    let ids: Vec<&str> = SPECS.iter().map(|s| s.id).collect();
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(ids.len(), unique.len());
    let go = Command::GoTo {
        pane: mareader_workspace::PaneId(1),
        page: 0,
    };
    assert!(ids.contains(&go.id()), "a script can name a jump");
}
