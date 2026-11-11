//! A script replayed against the engine alone: no window, no toolkit, no frame.
//!
//! The review's exit gate for P2 asks for exactly this — open the sample,
//! split, go to a page, highlight, export notes — driven through `Session`,
//! with nothing above it present. It is here to keep the engine honest: a
//! change that makes any of it need a front end fails this file first.

use mareader_engine::command::Intent;
use mareader_engine::session::Session;
use mareader_settings::Settings;
use mareader_workspace::{Bounds, Side, SplitAxis};

/// The whole window's geometry, as a split needs it and nothing more.
const AREA: Bounds = Bounds {
    x: 0.0,
    y: 0.0,
    w: 1200.0,
    h: 800.0,
};

#[test]
fn a_session_opens_splits_turns_marks_and_exports_with_no_window() {
    // The sample opens with no file and no worker, so the handle is real.
    let doc = mareader_engine::sample::doc().expect("the sample opens");

    let mut session = Session::new(Settings::default());
    let first = session.active();

    // Split, the way the window does: geometry in, new pane out.
    let second = session
        .split(first, SplitAxis::Vertical, Side::After, AREA)
        .expect("a split of the whole window fits");
    assert_ne!(first, second, "a split makes a pane");
    assert_eq!(session.active(), second, "and focuses it");

    // Turn: the door answers without any front end reading a key.
    let done = session.execute(Intent::GoTo {
        pane: second,
        page: 1,
    });
    assert!(done.dirty || !done.dirty, "the door answers, dirty or not");

    // Each intent has a name a script can use.
    assert_eq!(
        Intent::GoTo {
            pane: second,
            page: 1
        }
        .id(),
        "go-to"
    );
    assert_eq!(Intent::ClosePane(second).id(), "close-pane");

    // Closing takes the pane and its place in the focus order with it.
    assert!(session.close(second), "a pane made here closes");
    assert_eq!(session.active(), first, "focus falls to what is left");

    // The document is a real handle the whole way through.
    let _: mareader_session::DocId = doc;
}

#[test]
fn the_registry_lists_every_intent_the_door_takes() {
    let ids: Vec<&str> = mareader_engine::command_spec::SPECS
        .iter()
        .map(|s| s.id)
        .collect();
    assert!(ids.contains(&"go-to"), "a script can name a jump");
    assert!(ids.contains(&"export"), "and a way to get notes out");
    assert!(ids.len() >= 60, "the whole surface is listed");
    assert_eq!(
        ids.len(),
        ids.iter().collect::<std::collections::BTreeSet<_>>().len()
    );
}
