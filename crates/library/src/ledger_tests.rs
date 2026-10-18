use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::book::Format;
use crate::folder::{Folder, Options, Tombstone};
use crate::id::{BookId, ContentId, FolderId};
use crate::ledger::{
    Action, Held, Known, Recovered, Registry, decide, decide_import, prune, recoverables, tombstone,
};

fn c(n: u128) -> ContentId {
    ContentId(n)
}

fn folder(placed: &[u128], watch: bool) -> Folder {
    let mut f = Folder::new(FolderId(1), PathBuf::from("/books"), Options::default(), 0);
    f.placed = placed.iter().map(|n| c(*n)).collect();
    if watch {
        f.set_tracking("", true);
    }
    f
}

fn known(id: u128, path: &str, missing: bool) -> Known {
    Known {
        id: BookId(id),
        path: PathBuf::from(path),
        missing,
        source: None,
    }
}

fn registry(rows: &[(u128, Known)]) -> Registry {
    rows.iter().map(|(n, k)| (c(*n), k.clone())).collect()
}

fn copy(id: u128, source: &str) -> Known {
    Known {
        source: Some(PathBuf::from(source)),
        ..known(id, "/store/copy.pdf", false)
    }
}

fn stone(n: u128, moved: bool) -> Tombstone {
    Tombstone {
        content: c(n),
        book: BookId(n),
        title: "Dune".into(),
        format: Format::Pdf,
        last_path: PathBuf::from("/books/dune.pdf"),
        shelf: None,
        removed_ms: 5,
        moved,
        returned: None,
        size: 0,
    }
}

fn p(s: &str) -> &Path {
    Path::new(s)
}

#[test]
fn an_unknown_file_is_added_only_on_a_watched_rung() {
    let r = Registry::new();
    assert_eq!(
        decide(&folder(&[], true), &r, p("/books/a.pdf"), c(1)),
        Action::Add
    );
    assert_eq!(
        decide(&folder(&[], false), &r, p("/books/a.pdf"), c(1)),
        Action::Skip,
        "a rung nobody watches adds nothing on a rescan"
    );
    assert_eq!(
        decide_import(&folder(&[], false), &r, p("/books/a.pdf"), c(1)),
        Action::Add,
        "but an import does"
    );
}

#[test]
fn a_known_book_at_its_own_address_is_skipped_and_a_moved_one_relinked() {
    let f = folder(&[1], true);
    let r = registry(&[(1, known(7, "/books/a.pdf", false))]);
    assert_eq!(decide(&f, &r, p("/books/a.pdf"), c(1)), Action::Skip);
    assert_eq!(
        decide(&f, &r, p("/books/sub/a.pdf"), c(1)),
        Action::Relink(BookId(7))
    );
}

#[test]
fn a_missing_book_is_relinked_by_a_folder_that_never_placed_it() {
    let r = registry(&[(1, known(7, "/gone/a.pdf", true))]);
    assert_eq!(
        decide(&folder(&[], true), &r, p("/books/a.pdf"), c(1)),
        Action::Relink(BookId(7))
    );
}

#[test]
fn a_second_folder_never_relinks_a_book_it_did_not_place() {
    let r = registry(&[(1, known(7, "/other/a.pdf", false))]);
    assert_eq!(
        decide(&folder(&[], true), &r, p("/books/a.pdf"), c(1)),
        Action::Skip
    );
}

#[test]
fn a_book_moved_off_the_folder_is_never_re_added() {
    assert_eq!(
        decide(
            &folder(&[1], true),
            &Registry::new(),
            p("/books/a.pdf"),
            c(1)
        ),
        Action::Skip,
        "a placed row that went stays gone"
    );
}

#[test]
fn a_removed_book_stays_removed_until_an_import_asks_for_it() {
    let mut f = folder(&[1], true);
    f.ignored.push(stone(1, false));
    let r = Registry::new();
    assert_eq!(decide(&f, &r, p("/books/dune.pdf"), c(1)), Action::Skip);
    assert_eq!(
        decide_import(&f, &r, p("/books/dune.pdf"), c(1)),
        Action::Add
    );
}

#[test]
fn an_explicit_import_duplicates_nothing_this_folder_placed() {
    let f = folder(&[1], false);
    let r = registry(&[(1, known(7, "/books/a.pdf", false))]);
    assert_eq!(decide_import(&f, &r, p("/books/a.pdf"), c(1)), Action::Skip);
    assert_eq!(
        decide_import(&f, &r, p("/books/moved/a.pdf"), c(1)),
        Action::Relink(BookId(7))
    );
    let elsewhere = registry(&[(1, known(7, "/other/a.pdf", false))]);
    assert_eq!(
        decide_import(&folder(&[], false), &elsewhere, p("/books/a.pdf"), c(1)),
        Action::Add,
        "content held elsewhere is the import's question"
    );
}

#[test]
fn a_copy_never_relinks_its_own_source() {
    let r = registry(&[(1, copy(7, "/books/a.pdf"))]);
    assert_eq!(
        decide(&folder(&[1], true), &r, p("/books/a.pdf"), c(1)),
        Action::Skip
    );
    assert_eq!(
        decide(&folder(&[], true), &r, p("/books/a.pdf"), c(1)),
        Action::Skip
    );
    let moved = registry(&[(1, copy(7, "/books/elsewhere.pdf"))]);
    assert_eq!(
        decide(&folder(&[1], true), &moved, p("/books/moved.pdf"), c(1)),
        Action::Relink(BookId(7)),
        "the source moving is provenance to follow"
    );
}

#[test]
fn an_explicit_import_of_a_copy_source_asks_for_the_files_own_book() {
    let r = registry(&[(1, copy(7, "/books/a.pdf"))]);
    assert_eq!(
        decide_import(&folder(&[1], false), &r, p("/books/a.pdf"), c(1)),
        Action::Add
    );
}

#[test]
fn only_the_folders_that_placed_a_book_take_its_tombstone() {
    let mut folders = vec![folder(&[1], true), folder(&[2], true)];
    tombstone(&mut folders, &stone(1, false));
    tombstone(&mut folders, &stone(1, false));
    assert_eq!(folders[0].ignored.len(), 1, "once, however often");
    assert!(folders[1].ignored.is_empty());
}

#[test]
fn pruning_drops_a_returned_book_and_keeps_a_moved_out_log() {
    let mut f = folder(&[1, 2], true);
    f.ignored = vec![stone(1, false), stone(2, true)];
    let r = registry(&[(1, known(1, "/b", false)), (2, known(2, "/c", false))]);
    prune(&mut f, &r);
    assert_eq!(f.ignored, vec![stone(2, true)]);
}

fn held(id: u128, shelves: &[(&str, Option<FolderId>)], missing: bool) -> Held {
    Held {
        id: BookId(id),
        title: format!("Book {id}"),
        missing,
        shelves: shelves.iter().map(|(n, f)| (n.to_string(), *f)).collect(),
    }
}

#[test]
fn a_removed_book_is_offered_back_and_removals_come_first() {
    let mut f = folder(&[1, 2], true);
    f.ignored.push(stone(1, false));
    f.last_seen = vec![(c(2), PathBuf::from("/books/b.pdf"))];
    let mut books = HashMap::new();
    books.insert(c(2), held(2, &[("Fiction", None)], false));
    let got = recoverables(&f, &books);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0], Recovered::Removed(stone(1, false)));
    assert_eq!(
        got[1],
        Recovered::Moved {
            book: BookId(2),
            title: "Book 2".into(),
            path: PathBuf::from("/books/b.pdf"),
            home: Some("Fiction".into()),
        }
    );
}

#[test]
fn a_moved_out_log_and_a_returned_book_are_not_offered_as_removals() {
    let mut f = folder(&[1, 2], true);
    f.ignored = vec![stone(1, true), stone(2, false)];
    let mut books = HashMap::new();
    books.insert(c(2), held(2, &[], false));
    assert!(
        recoverables(&f, &books).is_empty(),
        "a book that came back by another route is not a recovery"
    );
}

#[test]
fn a_book_still_on_its_folders_shelf_or_missing_is_not_a_move() {
    let mut f = folder(&[1, 2, 3], true);
    f.last_seen = vec![
        (c(1), PathBuf::from("/books/a.pdf")),
        (c(2), PathBuf::from("/books/b.pdf")),
        (c(3), PathBuf::from("/books/c.pdf")),
    ];
    let mut books = HashMap::new();
    books.insert(
        c(1),
        held(1, &[("Theirs", None), ("Ours", Some(FolderId(1)))], false),
    );
    books.insert(c(2), held(2, &[], true));
    books.insert(c(3), held(3, &[("Other tree", Some(FolderId(9)))], false));
    let got = recoverables(&f, &books);
    assert_eq!(
        got.len(),
        1,
        "only the book off every shelf this folder owns"
    );
    assert!(matches!(&got[0], Recovered::Moved { book, home, .. }
        if *book == BookId(3) && home.as_deref() == Some("Other tree")));
    books.insert(c(3), held(3, &[], false));
    assert!(matches!(
        &recoverables(&f, &books)[0],
        Recovered::Moved { home: None, .. }
    ));
}

#[test]
fn a_tombstone_of_a_book_never_named_labels_itself_from_the_file() {
    let mut s = stone(1, false);
    assert_eq!(s.label(), "Dune");
    s.title.clear();
    assert_eq!(s.label(), "dune");
}

#[test]
fn the_last_walk_is_remembered_only_for_books_this_folder_placed() {
    let mut f = folder(&[1], true);
    f.record_seen([(c(1), p("/books/a.pdf")), (c(2), p("/books/b.pdf"))]);
    assert_eq!(f.last_seen, vec![(c(1), PathBuf::from("/books/a.pdf"))]);
}
