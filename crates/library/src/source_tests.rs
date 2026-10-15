use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::disk::{self, FoundAgain, Limits, Sought};
use crate::{
    AddError, ContentId, DeviceId, FileStamp, Format, Found, Library, Measure, Opened, Origin,
    Position, RelinkError, Scan, SourceEdit,
};

const DEVICE: DeviceId = DeviceId(7);

fn measure(content: u128, size: u64) -> Measure {
    Measure {
        content: ContentId(content),
        stamp: FileStamp { size, mtime_ms: 1 },
    }
}

fn found(path: &str, content: u128) -> Found {
    Found {
        path: PathBuf::from(path),
        format: Format::Pdf,
        measure: measure(content, 100),
        title: None,
        author: None,
    }
}

fn opened(path: &str, content: u128) -> Opened {
    Opened {
        path: PathBuf::from(path),
        measure: measure(content, 100),
        format: Format::Pdf,
        title: format!("Book {content}"),
        author: None,
    }
}

fn scan(lib: &mut Library, source: crate::SourceId, found: Vec<Found>) -> crate::ScanReport {
    let scan = Scan {
        source,
        found,
        offline: false,
    };
    lib.apply_scan(scan, 5_000)
}

fn path_of(lib: &Library, id: crate::BookId) -> Option<PathBuf> {
    lib.book(id).and_then(|b| b.path()).map(Path::to_path_buf)
}

#[test]
fn a_folder_takes_its_known_files_and_never_nests() {
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/home/me/books/a.pdf", 1), 1_000);
    let books = lib.add_source("/home/me/books".into(), 2_000).unwrap();
    let origin = lib.book(a).unwrap().locations[0].origin;
    assert_eq!(origin, Origin::Source(books.id));
    assert_eq!(lib.books_in(books.id).count(), 1);
    let inner = lib.add_source("/home/me/books/sci-fi".into(), 3_000);
    assert_eq!(inner, Err(AddError::Inside("books".into())));
    assert_eq!(lib.add_source("/".into(), 3_000), Err(AddError::Disk));
    // A folder around it takes it over, books and all.
    let home = lib.add_source("/home/me".into(), 4_000).unwrap();
    assert_eq!(home.absorbed, ["books"]);
    assert!(lib.source(books.id).is_none());
    assert_eq!(lib.books_in(home.id).count(), 1);
}

#[test]
fn a_scan_adds_unread_books_named_by_their_files_once() {
    let mut lib = Library::new(DEVICE);
    let s = lib.add_source("/s".into(), 1_000).unwrap().id;
    lib.take_dirty();
    let report = scan(&mut lib, s, vec![found("/s/Dune (1).pdf", 1)]);
    let [id] = report.added[..] else {
        panic!("{report:?}")
    };
    let book = lib.book(id).unwrap();
    assert_eq!(book.title.value, "Dune");
    assert_eq!(book.opened.value, 0, "found, not opened");
    assert!(lib.take_dirty().library);
    // The same walk again changes nothing and writes nothing.
    assert!(scan(&mut lib, s, vec![found("/s/Dune (1).pdf", 1)]).is_empty());
    assert!(!lib.is_dirty());
}

#[test]
fn a_moved_file_is_followed_and_a_second_copy_is_a_second_place() {
    let mut lib = Library::new(DEVICE);
    let s = lib.add_source("/s".into(), 1_000).unwrap().id;
    let id = scan(&mut lib, s, vec![found("/s/a.pdf", 1)]).added[0];
    lib.record_position(id, page(40), 2_000);
    // Moved: the old path is gone by the time the walk sees the new one.
    lib.check_present(|p| p != Path::new("/s/a.pdf"));
    let report = scan(&mut lib, s, vec![found("/s/read/a.pdf", 1)]);
    assert_eq!(report.followed, [id]);
    assert_eq!(path_of(&lib, id), Some("/s/read/a.pdf".into()));
    assert_eq!(lib.book(id).unwrap().locations.len(), 1);
    assert_eq!(lib.position(id), Some(page(40)));
    // A copy beside it is the same book in a second place.
    let report = scan(
        &mut lib,
        s,
        vec![found("/s/read/a.pdf", 1), found("/s/b.pdf", 1)],
    );
    assert!(report.followed.is_empty() && report.added.is_empty());
    assert_eq!(lib.len(), 1);
    assert_eq!(lib.book(id).unwrap().locations.len(), 2);
}

#[test]
fn a_file_edited_in_place_keeps_its_book() {
    let mut lib = Library::new(DEVICE);
    let s = lib.add_source("/s".into(), 1_000).unwrap().id;
    let id = scan(&mut lib, s, vec![found("/s/notes.pdf", 1)]).added[0];
    let report = scan(&mut lib, s, vec![found("/s/notes.pdf", 2)]);
    assert_eq!(report.edited, [id]);
    assert_eq!(lib.book(id).unwrap().content, ContentId(2));
    assert_eq!(lib.len(), 1);
}

#[test]
fn a_folder_removed_and_added_again_brings_its_books_back_with_their_places() {
    let mut lib = Library::new(DEVICE);
    let s = lib.add_source("/s".into(), 1_000).unwrap().id;
    let id = scan(&mut lib, s, vec![found("/s/a.pdf", 1)]).added[0];
    let kept = lib.record_open(opened("/elsewhere/b.pdf", 2), 1_500);
    scan(
        &mut lib,
        s,
        vec![found("/s/a.pdf", 1), found("/s/b.pdf", 2)],
    );
    lib.record_position(id, page(12), 2_000);
    assert_eq!(lib.remove_source(s, 3_000), 1, "b is also elsewhere");
    assert!(lib.book(id).is_none());
    assert_eq!(path_of(&lib, kept), Some("/elsewhere/b.pdf".into()));
    let again = lib.add_source("/s".into(), 4_000).unwrap().id;
    lib.take_dirty();
    let report = scan(&mut lib, again, vec![found("/s/a.pdf", 1)]);
    assert_eq!(report.back, [id]);
    assert_eq!(lib.position(id), Some(page(12)));
    assert_eq!(
        lib.book_at(Path::new("/s/a.pdf")),
        Some(id),
        "indexed again"
    );
    assert!(lib.is_dirty(), "the return is saved");
}

#[test]
fn what_a_folder_counts_and_hides_stays_out_and_an_offline_one_changes_nothing() {
    let mut lib = Library::new(DEVICE);
    let s = lib.add_source("/s".into(), 1_000).unwrap().id;
    let edit = SourceEdit {
        formats: BTreeSet::from([Format::Epub]),
        min_size: 50,
        watch: false,
    };
    assert!(lib.edit_source(s, edit.clone()));
    let none = SourceEdit {
        formats: BTreeSet::new(),
        ..edit
    };
    assert!(!lib.edit_source(s, none), "a folder must count something");
    let mut small = found("/s/small.epub", 1);
    (small.format, small.measure.stamp.size) = (Format::Epub, 10);
    let report = scan(&mut lib, s, vec![found("/s/a.pdf", 2), small]);
    assert!(report.is_empty());
    let offline = Scan {
        source: s,
        found: vec![found("/s/a.pdf", 3)],
        offline: true,
    };
    assert!(lib.apply_scan(offline, 6_000).is_empty());
    assert!(lib.is_empty());
}

#[test]
fn relinking_heals_the_same_content_and_asks_about_other() {
    let mut lib = Library::new(DEVICE);
    let id = lib.record_open(opened("/old/a.pdf", 1), 1_000);
    let other = lib.record_open(opened("/b.pdf", 2), 1_000);
    lib.check_present(|p| p != Path::new("/old/a.pdf"));
    assert!(lib.book(id).unwrap().missing());
    let to = Path::new("/new/renamed.pdf");
    assert_eq!(
        lib.relink(id, to, Format::Pdf, measure(1, 100), false),
        Ok(())
    );
    assert_eq!(path_of(&lib, id), Some(to.into()));
    assert_eq!(
        lib.book(id).unwrap().locations.len(),
        1,
        "the dead path goes"
    );
    // Another book's file, another format, other content: refused or asked.
    let b = lib.relink(id, Path::new("/x.pdf"), Format::Pdf, measure(2, 1), false);
    assert_eq!(b, Err(RelinkError::OtherBook("Book 2".into())));
    let epub = lib.relink(id, Path::new("/x.epub"), Format::Epub, measure(9, 1), true);
    let wrong = RelinkError::Format {
        file: Format::Epub,
        book: Format::Pdf,
    };
    assert_eq!(epub, Err(wrong));
    let edition = Path::new("/x.pdf");
    let asked = lib.relink(id, edition, Format::Pdf, measure(9, 1), false);
    assert_eq!(asked, Err(RelinkError::Differs));
    assert_eq!(
        lib.relink(id, edition, Format::Pdf, measure(9, 1), true),
        Ok(())
    );
    assert_eq!(lib.book(id).unwrap().content, ContentId(9));
    assert!(lib.book(other).is_some());
}

fn page(page: usize) -> Position {
    Position {
        page,
        pages: 100,
        place: None,
    }
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Old enough not to be settling.
fn limits() -> Limits {
    Limits {
        settle_ms: 0,
        ..Limits::new(u64::MAX)
    }
}

#[test]
fn a_walk_keeps_books_and_skips_hidden_litter_and_deep_folders() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("a.pdf"), b"%PDF a");
    write(&root.join("sub/b.EPUB"), b"b");
    write(&root.join("sub/notes.docx"), b"no");
    write(&root.join(".hidden/c.pdf"), b"c");
    write(&root.join(".d.pdf"), b"d");
    write(&root.join("~$lock.pdf"), b"e");
    write(&root.join("node_modules/f.pdf"), b"f");
    write(&root.join("1/2/3/deep.pdf"), b"g");
    let shallow = Limits {
        depth: 2,
        ..limits()
    };
    let walked = disk::walk(root, |_, _| true, shallow, &[]);
    let names: Vec<_> = walked
        .files
        .iter()
        .map(|f| f.0.strip_prefix(root).unwrap())
        .collect();
    assert_eq!(names, [Path::new("a.pdf"), Path::new("sub/b.EPUB")]);
    assert!(walked.truncated, "too deep to see everything");
    assert!(!walked.offline);
    // Just written: maybe still copying, so left for a later look.
    let now = Limits::new(u64::MAX / 2);
    let fresh = Limits {
        now_ms: walked.files[0].2.mtime_ms,
        ..now
    };
    assert_eq!(disk::walk(root, |_, _| true, fresh, &[]).settling, 3);
    assert!(disk::walk(&root.join("gone"), |_, _| true, limits(), &[]).offline);
}

#[cfg(unix)]
#[test]
fn a_walk_never_follows_links() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("books");
    write(&root.join("a.pdf"), b"a");
    write(&dir.path().join("outside/b.pdf"), b"b");
    std::os::unix::fs::symlink(dir.path().join("outside"), root.join("link")).unwrap();
    std::os::unix::fs::symlink(root.join("a.pdf"), root.join("again.pdf")).unwrap();
    let walked = disk::walk(&root, |_, _| true, limits(), &[]);
    assert_eq!(walked.files.len(), 1);
}

#[test]
fn a_lost_file_is_found_again_by_content_under_any_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let old = root.join("book.pdf");
    write(&old, b"%PDF the very book");
    let content = disk::measure(&old, Format::Pdf).unwrap();
    std::fs::remove_file(&old).unwrap();
    write(&root.join("x/renamed.pdf"), b"%PDF the very book");
    write(&root.join("y/book.pdf"), b"%PDF a later edition");
    let sought = Sought {
        content: content.content,
        format: Format::Pdf,
        name: Some("book.pdf".into()),
        size: Some(content.stamp.size),
    };
    let FoundAgain::Same(path, _) = disk::find_again(root, &sought, limits()) else {
        panic!("not found by content");
    };
    assert_eq!(path, root.join("x/renamed.pdf"));
    std::fs::remove_file(&path).unwrap();
    let named = disk::find_again(root, &sought, limits());
    assert!(matches!(named, FoundAgain::Named(p, _) if p == root.join("y/book.pdf")));
    std::fs::remove_file(root.join("y/book.pdf")).unwrap();
    assert_eq!(
        disk::find_again(root, &sought, limits()),
        FoundAgain::Nothing
    );
}
