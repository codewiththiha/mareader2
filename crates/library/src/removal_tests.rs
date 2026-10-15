use std::path::PathBuf;

use crate::disk::{self, Folder};
use crate::{
    BookId, ContentId, DeviceId, FileStamp, Format, Found, Imported, Library, Measure, Opened,
    Origin, Position, Receipt, Scan, SourceId,
};

const DEVICE: DeviceId = DeviceId(7);

fn measure(content: u128) -> Measure {
    Measure {
        content: ContentId(content),
        stamp: FileStamp {
            size: 100,
            mtime_ms: 1,
        },
    }
}

fn opened(path: &str, content: u128) -> Opened {
    Opened {
        path: PathBuf::from(path),
        measure: measure(content),
        format: Format::Pdf,
        title: format!("Book {content}"),
        author: None,
    }
}

fn scan(lib: &mut Library, source: SourceId, files: &[(&str, u128)]) -> crate::ScanReport {
    let found = files.iter().map(|(path, content)| Found {
        path: PathBuf::from(path),
        format: Format::Pdf,
        measure: measure(*content),
        title: None,
        author: None,
    });
    let scan = Scan {
        source,
        found: found.collect(),
        offline: false,
    };
    lib.apply_scan(scan, 5_000)
}

fn page(page: usize) -> Position {
    Position {
        page,
        pages: 10,
        place: None,
    }
}

#[test]
fn a_folder_book_removed_stays_hidden_until_restored_with_its_place() {
    let mut lib = Library::new(DEVICE);
    let s = lib.add_source("/s".into(), 1_000).unwrap().id;
    let id = scan(&mut lib, s, &[("/s/a.pdf", 1)]).added[0];
    lib.record_position(id, page(4), 2_000);
    let shelf = lib.create_shelf("Read", None, 2_000).unwrap();
    lib.file(id, shelf, 2_000);
    let receipt = lib.receipt(&[id]);
    let want = Receipt {
        books: 1,
        positions: 1,
        spots: 1,
        copies: 0,
        hidden: 1,
    };
    assert_eq!(receipt, want);
    let removed = lib.remove_books(&[id], 3_000);
    assert_eq!((removed.books, removed.delete.len()), (1, 0));
    assert!(lib.book(id).is_none() && lib.book_at("/s/a.pdf".as_ref()).is_none());
    // The file is still there; a rescan leaves it out.
    assert!(scan(&mut lib, s, &[("/s/a.pdf", 1)]).is_empty());
    assert!(lib.is_empty());
    let hidden: Vec<BookId> = lib.hidden_in(s).iter().map(|b| b.id).collect();
    assert_eq!(hidden, [id]);
    assert!(lib.restore(id, 4_000));
    assert!(lib.hidden_in(s).is_empty());
    assert_eq!(lib.position(id), Some(page(4)));
    assert_eq!(lib.shelf_books(shelf).len(), 1);
    assert_eq!(lib.book_at("/s/a.pdf".as_ref()), Some(id));
    assert!(!lib.restore(id, 5_000));
}

#[test]
fn opening_a_removed_folder_book_brings_it_back_unhidden() {
    let mut lib = Library::new(DEVICE);
    let s = lib.add_source("/s".into(), 1_000).unwrap().id;
    let id = scan(&mut lib, s, &[("/s/a.pdf", 1)]).added[0];
    lib.remove_books(&[id], 2_000);
    assert_eq!(lib.record_open(opened("/s/a.pdf", 1), 3_000), id);
    assert!(lib.hidden_in(s).is_empty());
    assert!(lib.source(s).unwrap().hidden.is_empty());
}

#[test]
fn a_twin_reads_separately_and_follows_its_originals_file() {
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1), 1_000);
    lib.record_position(a, page(5), 1_000);
    let shelf = lib.create_shelf("S", None, 1_000).unwrap();
    let other = lib.record_open(opened("/b.pdf", 2), 1_000);
    lib.file(a, shelf, 1_000);
    lib.file(other, shelf, 1_000);
    let t = lib.twin(a, 2_000).unwrap();
    let t3 = lib.twin(t, 2_000).unwrap();
    let title = |id| lib.book(id).unwrap().title.value.clone();
    assert_eq!(
        (title(t), title(t3)),
        ("Book 1 (2)".into(), "Book 1 (3)".into())
    );
    assert_eq!(lib.book(t3).unwrap().twin_of, Some(a));
    assert_eq!(lib.position(t), None);
    // Each twin lands right behind the book it was made from.
    let order: Vec<BookId> = lib.shelf_books(shelf).iter().map(|b| b.id).collect();
    assert_eq!(order, [a, t, t3, other]);
    // Paths still name the original; a move reaches every twin.
    assert_eq!(lib.book_at("/a.pdf".as_ref()), Some(a));
    lib.record_open(opened("/moved.pdf", 1), 3_000);
    assert_eq!(lib.book(t).unwrap().path(), Some("/moved.pdf".as_ref()));
    assert_eq!(lib.record_open(opened("/moved.pdf", 1), 3_000), a);
    // Read as the twin, only the twin was opened.
    lib.record_open_as(opened("/moved.pdf", 1), 9_000, Some(t));
    assert_eq!(lib.book(t).unwrap().opened.value, 9_000);
    assert_eq!(lib.book(a).unwrap().opened.value, 3_000);
}

#[test]
fn removing_an_original_hands_its_files_to_a_twin() {
    let mut lib = Library::new(DEVICE);
    let s = lib.add_source("/s".into(), 1_000).unwrap().id;
    let a = scan(&mut lib, s, &[("/s/a.pdf", 1)]).added[0];
    let t = lib.twin(a, 2_000).unwrap();
    assert_eq!(lib.receipt(&[a]).hidden, 0);
    lib.remove_books(&[a], 3_000);
    assert!(lib.source(s).unwrap().hidden.is_empty());
    assert_eq!(lib.book(t).unwrap().twin_of, None);
    assert_eq!(lib.book_at("/s/a.pdf".as_ref()), Some(t));
    // The heir is the folder's book now: a rescan adds nothing.
    assert!(scan(&mut lib, s, &[("/s/a.pdf", 1)]).is_empty());
    assert_eq!(lib.len(), 1);
    // Both gone together, the file is hidden under each.
    let t2 = lib.twin(t, 4_000).unwrap();
    assert_eq!(lib.receipt(&[t, t2]).hidden, 2);
    lib.remove_books(&[t, t2], 5_000);
    assert!(scan(&mut lib, s, &[("/s/a.pdf", 1)]).is_empty());
    assert!(lib.is_empty());
}

#[test]
fn a_restored_book_reads_beside_a_book_of_its_content() {
    let mut lib = Library::new(DEVICE);
    let s = lib.add_source("/s".into(), 1_000).unwrap().id;
    let a = scan(&mut lib, s, &[("/s/a.pdf", 1)]).added[0];
    let t = lib.twin(a, 2_000).unwrap();
    lib.remove_books(&[a, t], 3_000);
    // Imported again meanwhile: the same record comes back.
    let copy = opened("/lib/files/1/a.pdf", 1);
    assert_eq!(lib.import(copy, 4_000), Imported::New(a));
    assert!(lib.source(s).unwrap().hidden.contains(&t));
    assert!(lib.restore(t, 5_000));
    assert_eq!(lib.book(t).unwrap().twin_of, Some(a));
    assert!(lib.hidden_in(s).is_empty());
    assert_eq!(
        lib.book(t).unwrap().path(),
        Some("/lib/files/1/a.pdf".as_ref())
    );
}

#[test]
fn store_copies_are_deleted_with_their_book_and_known_content_is_kept() {
    let mut lib = Library::new(DEVICE);
    let a = match lib.import(opened("/lib/files/1/a.pdf", 1), 1_000) {
        Imported::New(id) => id,
        Imported::Known(_) => panic!("new content"),
    };
    let origin = lib.book(a).unwrap().locations[0].origin;
    assert_eq!(origin, Origin::Store);
    assert_eq!(lib.import(opened("/x/a.pdf", 1), 2_000), Imported::Known(a));
    let t = lib.twin(a, 2_000).unwrap();
    assert_eq!(lib.receipt(&[a]).copies, 0);
    assert!(lib.remove_books(&[a], 3_000).delete.is_empty());
    assert_eq!(lib.receipt(&[t]).copies, 1);
    let removed = lib.remove_books(&[t], 4_000);
    assert_eq!(removed.delete, [PathBuf::from("/lib/files/1/a.pdf")]);
}

#[test]
fn renames_are_trimmed_and_blank_ones_refused() {
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1), 1_000);
    assert!(!lib.rename_book(a, "  \n ", 2_000));
    assert!(!lib.rename_book(a, "Book 1", 2_000));
    assert!(lib.rename_book(a, "  Moby\n Dick ", 2_000));
    assert_eq!(lib.book(a).unwrap().title.value, "Moby Dick");
    // A reopen keeps the reader's own name.
    lib.record_open(opened("/a.pdf", 1), 3_000);
    assert_eq!(lib.book(a).unwrap().title.value, "Moby Dick");
}

#[test]
fn copies_land_in_the_store_and_only_store_files_are_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path().join("lib"));
    let file = dir.path().join("Dune.pdf");
    std::fs::write(&file, b"%PDF-1.7 dune").unwrap();
    let content = disk::measure(&file, Format::Pdf).unwrap().content;
    let (copy, stamp) = disk::copy_in(&folder.store(), &file, content).unwrap();
    assert!(copy.ends_with(format!("files/{content}/Dune.pdf")));
    assert_eq!(stamp.size, 13);
    // A second import of the same file reuses the copy.
    assert_eq!(
        disk::copy_in(&folder.store(), &file, content).unwrap().0,
        copy
    );
    let outside = [file.clone(), copy.clone()];
    assert_eq!(disk::delete_copies(&folder.store(), &outside), 1);
    assert!(file.is_file() && !copy.exists() && !copy.parent().unwrap().exists());
}

#[test]
fn twins_and_store_copies_come_back_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1), 1_000);
    let t = lib.twin(a, 2_000).unwrap();
    lib.import(opened("/lib/files/2/b.pdf", 2), 2_000);
    let dirty = lib.take_dirty();
    for (path, text) in folder.writes(&lib, &dirty) {
        disk::write_atomic(&path, text.as_bytes()).unwrap();
    }
    let back = folder.load(DEVICE).library;
    assert_eq!(back.book(t).unwrap().twin_of, Some(a));
    assert_eq!(back.book_at("/a.pdf".as_ref()), Some(a));
    let store = back.books().find(|b| b.content == ContentId(2)).unwrap();
    assert_eq!(store.locations[0].origin, Origin::Store);
}

#[test]
fn a_newer_library_that_wont_parse_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("library.json");
    let newer = r#"{"schema":99,"books":"a shape this build can't read"}"#;
    std::fs::write(&file, newer).unwrap();
    let loaded = Folder::new(dir.path()).load(DEVICE);
    assert!(!loaded.writable);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), newer);
}

#[test]
fn files_added_in_place_are_read_there_and_never_deleted() {
    let mut lib = Library::new(DEVICE);
    let a = match lib.link(opened("/books/a.pdf", 1), 1_000) {
        Imported::New(id) => id,
        Imported::Known(_) => panic!("new content"),
    };
    let book = lib.book(a).unwrap();
    assert_eq!(book.locations[0].origin, Origin::Opened);
    assert_eq!(book.opened.value, 0, "added, not read");
    // The same content elsewhere is another place of the same book.
    assert_eq!(lib.link(opened("/usb/a.pdf", 1), 2_000), Imported::Known(a));
    assert_eq!(lib.book(a).unwrap().locations.len(), 2);
    assert_eq!(lib.book_at(&PathBuf::from("/usb/a.pdf")), Some(a));
    assert_eq!(lib.receipt(&[a]).copies, 0);
    assert!(lib.remove_books(&[a], 3_000).delete.is_empty());
}
