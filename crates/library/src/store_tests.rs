#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::disk::Folder;
use crate::store::Store;
use crate::tests::save;
use crate::{ContentId, DeviceId, FileStamp, Format, Kind, Library, Measure, Opened, Position};

const DEVICE: DeviceId = DeviceId(7);

fn opened(path: &str, content: u128) -> Opened {
    Opened {
        path: PathBuf::from(path),
        measure: Measure {
            content: ContentId(content),
            stamp: FileStamp::default(),
        },
        format: Format::Pdf,
        title: format!("Book {content}"),
        author: None,
    }
}

fn sql(dir: &Path) -> Connection {
    Connection::open(dir.join("library.sqlite")).unwrap()
}

#[test]
fn a_save_writes_only_the_rows_that_changed() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1), 1_000);
    lib.record_open(opened("/b.pdf", 2), 1_100);
    save(&folder, &mut lib);

    assert!(lib.rename_book(a, "Renamed", 2_000));
    let dirty = lib.take_dirty();
    let save = folder.writes(&lib, &dirty);
    let kinds: Vec<Kind> = save.changes.put.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [Kind::Book, Kind::Locations]);
    assert!(save.changes.put.iter().all(|r| r.id == a.to_string()));
    // A page turn is a state file, never a row.
    let position = Position {
        page: 3,
        pages: 9,
        place: None,
    };
    lib.record_position(a, position, 2_100);
    let dirty = lib.take_dirty();
    let turned = folder.writes(&lib, &dirty);
    assert!(turned.changes.is_empty() && turned.states.len() == 1);
}

#[test]
fn library_json_moves_into_the_store_once() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1), 1_000);
    let shelf = lib.create_shelf("Poems", None, 1_100).unwrap();
    lib.file(a, shelf, 1_200);
    let books: Vec<_> = lib.books.values().collect();
    let shelves: Vec<_> = lib.shelves.values().collect();
    let members: Vec<_> = lib.members.values().collect();
    let json = serde_json::json!({
        "schema": 4, "clock": lib.clock, "books": books,
        "shelves": shelves, "members": members,
    });
    let file = dir.path().join("library.json");
    std::fs::write(&file, json.to_string()).unwrap();

    let loaded = folder.load(DeviceId(99));
    assert!(
        loaded.writable && loaded.notes.is_empty(),
        "{:?}",
        loaded.notes
    );
    assert!(!file.exists() && dir.path().join("library.json.migrated").is_file());
    let back = loaded.library;
    assert_eq!(back.device(), DEVICE);
    assert_eq!(back.book_at(Path::new("/a.pdf")), Some(a));
    assert_eq!(back.shelf_books(shelf).first().map(|b| b.id), Some(a));
    drop(loaded.store);
    // Read again from the store alone.
    let again = folder.load(DEVICE).library;
    assert_eq!(again.book_at(Path::new("/a.pdf")), Some(a));
    // Looking for the files is never a change to save.
    assert!(!again.is_dirty());
}

#[test]
fn a_damaged_store_is_kept_aside_and_a_new_one_started() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("library.sqlite");
    std::fs::write(&file, vec![7u8; 4096]).unwrap();
    let loaded = Folder::new(dir.path()).load(DEVICE);
    assert!(loaded.writable && loaded.library.is_empty());
    assert_eq!(loaded.notes.len(), 1, "{:?}", loaded.notes);
    assert!(dir.path().join("library.unreadable.sqlite").is_file());
}

#[test]
fn a_newer_store_is_read_but_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1), 1_000);
    save(&folder, &mut lib);
    sql(dir.path())
        .pragma_update(None, "user_version", 99)
        .unwrap();

    let loaded = folder.load(DEVICE);
    assert!(!loaded.writable && loaded.store.is_none());
    assert_eq!(loaded.notes.len(), 1);
    assert!(loaded.library.book(a).is_some());
}

#[test]
fn rows_this_build_cant_read_are_kept_as_they_were() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    lib.record_open(opened("/a.pdf", 1), 1_000);
    save(&folder, &mut lib);
    let conn = sql(dir.path());
    let add = "INSERT INTO records(kind, id, body, seq) VALUES(?1, ?2, ?3, 1)";
    conn.execute(add, ["note", "n1", "{}"]).unwrap();
    conn.execute(add, ["book", "broken", "{ nope"]).unwrap();
    drop(conn);

    let mut loaded = folder.load(DEVICE);
    // An unknown kind is a newer build's: silent. A broken row is told.
    assert_eq!(loaded.notes.len(), 1, "{:?}", loaded.notes);
    let id = loaded.library.record_open(opened("/b.pdf", 2), 2_000);
    let dirty = loaded.library.take_dirty();
    let save = folder.writes(&loaded.library, &dirty);
    save.apply(loaded.store.as_mut()).unwrap();
    drop(loaded);
    let count = |kind: &str| -> i64 {
        let query = "SELECT count(*) FROM records WHERE kind = ?1";
        sql(dir.path())
            .query_row(query, [kind], |r| r.get(0))
            .unwrap()
    };
    assert_eq!((count("note"), count("book")), (1, 3));
    assert!(folder.load(DEVICE).library.book(id).is_some());
}

#[test]
fn the_change_cursor_names_what_each_save_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1), 1_000);
    let b = lib.record_open(opened("/b.pdf", 2), 1_100);
    save(&folder, &mut lib);
    lib.rename_book(b, "Later", 2_000);
    save(&folder, &mut lib);

    let store = Store::open(&dir.path().join("library.sqlite")).unwrap();
    let all = store.changed_since(0).unwrap();
    let first = all.iter().find(|c| c.row.id == a.to_string()).unwrap().seq;
    let since = store.changed_since(first).unwrap();
    let ids: Vec<&str> = since.iter().map(|c| c.row.id.as_str()).collect();
    assert_eq!(ids, [b.to_string()]);
    // Paths are this machine's: they never leave as records.
    assert!(all.iter().all(|c| c.row.kind.synced()));
}
