use std::path::{Path, PathBuf};

use crate::disk::{self, Folder, write_atomic};
use crate::{BookId, ContentId, DeviceId, FileStamp, Format, Library, Measure, Opened, Position};

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
    }
}

fn page(page: usize) -> Position {
    Position {
        page,
        pages: 100,
        place: None,
    }
}

#[test]
fn a_file_opened_twice_is_one_book_last_opened_first() {
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1), 1_000);
    let b = lib.record_open(opened("/b.pdf", 2), 2_000);
    assert_eq!(lib.record_open(opened("/a.pdf", 1), 3_000), a);
    assert_eq!(lib.len(), 2);
    let order: Vec<BookId> = lib.recent().iter().map(|b| b.id).collect();
    assert_eq!(order, [a, b]);
    // The title is the document's at first sight; later opens keep it.
    let mut renamed = opened("/a.pdf", 1);
    renamed.title = "Other".into();
    lib.record_open(renamed, 4_000);
    assert_eq!(lib.book(a).map(|b| b.title.value.as_str()), Some("Book 1"));
}

#[test]
fn a_moved_file_is_the_same_book_and_keeps_its_place() {
    let mut lib = Library::new(DEVICE);
    let id = lib.record_open(opened("/old/a.pdf", 1), 1_000);
    lib.record_position(id, page(41), 1_100);
    assert_eq!(lib.record_open(opened("/new/a.pdf", 1), 2_000), id);
    let book = lib.book(id).unwrap();
    assert_eq!(book.path(), Some(Path::new("/new/a.pdf")));
    assert_eq!(lib.book_at(Path::new("/new/a.pdf")), Some(id));
    assert_eq!(lib.position(id), Some(page(41)));
}

#[test]
fn a_file_edited_in_place_is_the_same_book_with_new_content() {
    let mut lib = Library::new(DEVICE);
    let id = lib.record_open(opened("/notes.md", 1), 1_000);
    lib.record_position(id, page(3), 1_100);
    assert_eq!(lib.record_open(opened("/notes.md", 2), 2_000), id);
    assert_eq!(lib.book(id).unwrap().content, ContentId(2));
    assert_eq!(lib.position(id), Some(page(3)));
    assert_eq!(lib.len(), 1);
}

#[test]
fn a_path_holds_one_book() {
    // b.pdf is overwritten with a copy of a.pdf: it now reads book A.
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1), 1_000);
    let b = lib.record_open(opened("/b.pdf", 2), 2_000);
    lib.record_open(opened("/b.pdf", 1), 3_000);
    assert_eq!(lib.book_at(Path::new("/b.pdf")), Some(a));
    assert!(lib.book(b).unwrap().missing());
    assert_eq!(lib.book(a).unwrap().locations.len(), 2);
}

#[test]
fn only_changes_are_written_and_positions_apart_from_books() {
    let mut lib = Library::new(DEVICE);
    let id = lib.record_open(opened("/a.pdf", 1), 1_000);
    let dirty = lib.take_dirty();
    assert!(dirty.library && dirty.states.is_empty());
    assert!(lib.record_position(id, page(5), 1_100));
    assert!(!lib.record_position(id, page(5), 1_200));
    let dirty = lib.take_dirty();
    assert!(!dirty.library, "a page turn leaves library.json alone");
    assert_eq!(dirty.states.into_iter().collect::<Vec<_>>(), [id]);
    assert!(!lib.is_dirty());
    // An unknown book has no state to write.
    assert!(!lib.record_position(BookId(99), page(1), 1_300));
}

fn save(folder: &Folder, lib: &mut Library) {
    let dirty = lib.take_dirty();
    for (path, text) in folder.writes(lib, &dirty) {
        write_atomic(&path, text.as_bytes()).unwrap();
    }
}

#[test]
fn the_library_comes_back_from_its_files_with_missing_books_marked() {
    let dir = tempfile::tempdir().unwrap();
    let book_file = dir.path().join("a.pdf");
    std::fs::write(&book_file, b"%PDF").unwrap();
    let folder = Folder::new(dir.path().join("library"));
    let mut lib = folder.load(DEVICE).library;
    let here = Opened {
        path: book_file.clone(),
        ..opened("", 1)
    };
    let a = lib.record_open(here, 1_000);
    let gone = lib.record_open(opened("/nowhere/b.pdf", 2), 2_000);
    lib.record_position(a, page(12), 2_100);
    save(&folder, &mut lib);
    assert!(folder.root().join(format!("state/{a}.json")).is_file());

    let loaded = folder.load(DeviceId(99));
    assert!(
        loaded.writable && loaded.notes.is_empty(),
        "{:?}",
        loaded.notes
    );
    let back = loaded.library;
    // The device is the library's own, not the one asked for.
    assert_eq!(back.device(), DEVICE);
    assert_eq!(back.len(), 2);
    assert_eq!(back.position(a), Some(page(12)));
    assert_eq!(back.book_at(&book_file), Some(a));
    assert!(back.book(gone).unwrap().missing());
    assert!(!back.book(a).unwrap().missing());
}

#[test]
fn a_newer_library_is_left_alone_and_a_broken_one_kept_aside() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let file = dir.path().join("library.json");
    let newer =
        r#"{"schema":99,"clock":{"last":"0000000000000-00000000-0000000000000007"},"books":[]}"#;
    std::fs::write(&file, newer).unwrap();
    let loaded = folder.load(DEVICE);
    assert!(!loaded.writable);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), newer);

    std::fs::write(&file, "{ not json").unwrap();
    let loaded = folder.load(DEVICE);
    assert!(loaded.writable && loaded.library.is_empty());
    assert_eq!(loaded.notes.len(), 1);
    assert!(dir.path().join("library.unreadable.json").is_file());
    assert!(!file.exists());
}

#[test]
fn text_is_hashed_whole_and_large_books_by_their_ends() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let id = |path: &Path, format| disk::measure(path, format).unwrap().content;
    // A same-length edit in the middle of a note is new content.
    let mut note = vec![b'a'; 300 << 10];
    let pdf = id(&write("p0.pdf", &note), Format::Pdf);
    let before = id(&write("n1.md", &note), Format::Markdown);
    if let Some(middle) = note.get_mut(150 << 10) {
        *middle = b'b';
    }
    let after = id(&write("n2.md", &note), Format::Markdown);
    assert_ne!(before, after);
    // A PDF's middle isn't read; its tail (the xref) changes with edits.
    assert_eq!(id(&write("p1.pdf", &note), Format::Pdf), pdf);
    let mut tail = note.clone();
    if let Some(last) = tail.last_mut() {
        *last = b'z';
    }
    assert_ne!(id(&write("p2.pdf", &tail), Format::Pdf), pdf);
    // Size counts, so a truncated copy differs.
    let short = note.get(..note.len() - 1).unwrap();
    assert_ne!(id(&write("p3.pdf", short), Format::Pdf), pdf);
    let stamp = disk::measure(&dir.path().join("p1.pdf"), Format::Pdf)
        .unwrap()
        .stamp;
    assert_eq!(stamp.size, 300 << 10);
}
