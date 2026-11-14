//! The two files the engine keeps: written whole, read back, and repaired.

use std::sync::Arc;

use mareader_library::{BookId, SpinePosition};

use super::*;
use crate::host::Fake;
use crate::session::Session;

/// A directory that goes when the test does.
fn dir() -> Result<tempfile::TempDir, String> {
    tempfile::tempdir().map_err(|e| e.to_string())
}

fn put(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Choices a round trip has to show, so defaults prove nothing.
fn choices() -> Settings {
    let mut settings = Settings::default();
    settings.text.font_size = 21.0;
    settings
}

/// A book read to a page, with the place that outlives a change of type.
fn last(path: &Path) -> LastDocument {
    LastDocument {
        path: path.to_path_buf(),
        page: 12,
        place: Some((3, 4)),
        spine: Some(SpinePosition {
            href: "chapter-2.xhtml".into(),
            item: 5,
            block: 3,
            offset: 4,
        }),
        pages: 30,
        book: Some(BookId(7)),
    }
}

/// A save that writes into `dir`, owing its first write.
fn owing(dir: &Path) -> Save {
    let mut save = Save::new(Some(dir.to_path_buf()));
    save.touch();
    save
}

#[test]
fn settings_and_the_last_document_come_back_from_disk() -> Result<(), String> {
    let dir = dir()?;
    let file = dir.path().join("sample.epub");
    let mut save = owing(dir.path());
    save.note(Some(last(&file)));
    assert_eq!(save.write(0.0, &choices()), None);
    assert!(dir.path().join(SETTINGS_FILE).exists());
    assert!(dir.path().join(SESSION_FILE).exists());
    let loaded = load(Some(dir.path()));
    assert_eq!(loaded.settings, choices());
    assert_eq!(loaded.last.as_ref(), Some(&last(&file)));
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    Ok(())
}

#[test]
fn a_temp_file_left_by_an_interrupted_write_is_not_read() -> Result<(), String> {
    let dir = dir()?;
    let mut save = owing(dir.path());
    save.write(0.0, &choices());
    let stale = dir.path().join(format!("{SETTINGS_FILE}.tmp"));
    put(&stale, "{\"text\": ")?;
    let loaded = load(Some(dir.path()));
    assert_eq!(loaded.settings, choices(), "the file beside it is read");
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    // The next write goes past the stale temp, which is still ignored.
    save.flush(&Settings::default());
    put(&stale, "{\"text\": ")?;
    assert_eq!(load(Some(dir.path())).settings, Settings::default());
    Ok(())
}

#[test]
fn choices_cut_in_half_load_as_defaults_and_say_so() -> Result<(), String> {
    let dir = dir()?;
    let cut = dir.path().join(SETTINGS_FILE);
    put(&cut, "{\"text\": {\"font_size\": 2")?;
    let loaded = load(Some(dir.path()));
    assert_eq!(loaded.settings, Settings::default());
    assert!(!loaded.notes.is_empty(), "a cut file is reported");
    Ok(())
}

#[test]
fn one_section_gone_leaves_the_rest_of_the_choices() -> Result<(), String> {
    let dir = dir()?;
    // Real settings, with one section replaced by something of the wrong shape.
    let value = serde_json::to_value(choices());
    let mut root = value.map_err(|e| e.to_string())?;
    let Some(map) = root.as_object_mut() else {
        return Err("choices serialize to an object".into());
    };
    map.insert("paper".into(), serde_json::json!("not a section"));
    let json = serde_json::to_string(&root).map_err(|e| e.to_string())?;
    put(&dir.path().join(SETTINGS_FILE), &json)?;
    let loaded = load(Some(dir.path()));
    assert_eq!(loaded.settings.text.font_size, 21.0, "the rest survives");
    assert!(
        loaded.notes.iter().any(|n| n.contains("paper")),
        "{:?}",
        loaded.notes
    );
    Ok(())
}

#[test]
fn a_session_file_that_will_not_parse_leaves_nothing_to_reopen() -> Result<(), String> {
    let dir = dir()?;
    put(&dir.path().join(SESSION_FILE), "{\"path\": ")?;
    let loaded = load(Some(dir.path()));
    assert_eq!(loaded.last, None);
    assert!(
        loaded.notes.iter().any(|n| n.contains(SESSION_FILE)),
        "{:?}",
        loaded.notes
    );
    Ok(())
}

#[test]
fn a_session_file_holding_null_reopens_nothing_and_says_nothing() -> Result<(), String> {
    let dir = dir()?;
    put(&dir.path().join(SESSION_FILE), "null")?;
    let loaded = load(Some(dir.path()));
    assert_eq!(loaded.last, None);
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    Ok(())
}

#[test]
fn without_a_directory_nothing_is_written_and_nothing_is_read() {
    let mut save = Save::default();
    save.touch();
    assert_eq!(save.write(0.0, &choices()), None);
    assert_eq!(save.dir(), None);
    let loaded = load(None);
    assert_eq!(loaded.settings, Settings::default());
    assert_eq!(loaded.last, None);
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
}

#[test]
fn a_session_that_changed_nothing_writes_no_file() -> Result<(), String> {
    let dir = dir()?;
    let mut save = Save::new(Some(dir.path().to_path_buf()));
    assert_eq!(save.write(0.0, &choices()), None);
    assert!(!save.dirty());
    assert!(!dir.path().join(SETTINGS_FILE).exists(), "nothing was owed");
    Ok(())
}

#[test]
fn a_write_waits_for_the_delay_and_a_flush_does_not() -> Result<(), String> {
    let dir = dir()?;
    let mut save = owing(dir.path());
    assert_eq!(save.write(0.0, &choices()), None, "due at once");
    save.touch();
    let wait = save.write(0.1, &choices());
    assert!(wait.is_some_and(|w| (w - 0.4).abs() < 1e-9), "{wait:?}");
    assert!(save.dirty(), "a skipped write is still owed");
    assert_eq!(save.write(0.6, &choices()), None, "the delay passed");
    save.touch();
    save.flush(&Settings::default());
    assert!(!save.dirty(), "a flush leaves nothing owed");
    assert_eq!(load(Some(dir.path())).settings, Settings::default());
    Ok(())
}

#[test]
fn a_refused_write_is_still_owed() -> Result<(), String> {
    let dir = dir()?;
    let file = dir.path().join("a-file");
    put(&file, "not a directory")?;
    let mut save = owing(&file.join("state"));
    assert_eq!(save.write(0.0, &choices()), None);
    assert!(save.dirty(), "a refused write is owed again");
    let why = save.take_failure();
    assert!(
        why.as_ref().is_some_and(|w| w.contains(SETTINGS_FILE)),
        "{why:?}"
    );
    assert_eq!(save.take_failure(), None, "the reason is told once");
    Ok(())
}

#[test]
fn a_document_remembered_twice_owes_one_write() -> Result<(), String> {
    let dir = dir()?;
    let file = dir.path().join("sample.pdf");
    let mut save = Save::new(Some(dir.path().to_path_buf()));
    save.note(Some(last(&file)));
    assert!(save.dirty(), "something to reopen is owed");
    save.note(Some(last(&file)));
    assert_eq!(save.write(0.0, &choices()), None);
    assert!(!save.dirty(), "the same document owes nothing more");
    save.note(Some(last(&file)));
    assert!(!save.dirty());
    save.note(None);
    assert!(save.dirty(), "a window left empty is worth a write");
    assert_eq!(save.write(0.5, &choices()), None);
    assert_eq!(load(Some(dir.path())).last, None);
    Ok(())
}

#[test]
fn a_session_keeps_its_own_two_files() -> Result<(), String> {
    let dir = dir()?;
    let file = dir.path().join("sample.epub");
    let nowhere = Session::new(Arc::new(Fake::default()), Arc::new(|| {}));
    assert!(!nowhere.persists(), "no directory, nothing written");
    let host = Arc::new(Fake::under(dir.path()));
    let mut session = Session::new(host, Arc::new(|| {}));
    session.import(choices());
    assert!(session.persists(), "the host named a directory");
    assert_eq!(session.keep(0.0, None), None, "nothing is owed yet");
    assert!(!dir.path().join(SETTINGS_FILE).exists());
    assert_eq!(session.keep(0.0, Some(last(&file))), None, "due at once");
    let loaded = load(Some(dir.path()));
    assert_eq!(loaded.settings, choices());
    assert_eq!(loaded.last.as_ref(), Some(&last(&file)));
    // A window left empty is written too, so the next launch starts clean.
    session.settle(None);
    assert_eq!(load(Some(dir.path())).last, None);
    assert_eq!(session.take_failure(), None);
    Ok(())
}
