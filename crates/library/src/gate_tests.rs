use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::folder::{Mode, Options};
use crate::gate::{Answered, Gate};
use crate::run::{Asked, Copied, Fold, Landed, Plan};
use crate::run_tests::{DEVICE, found, import, opts, run, shelf_named, titles_on};
use crate::screen::{Answer, Why};
use crate::{BookId, Data, Entry, FolderId, Found, Library, Origin, Placement, ShelfKind};

/// A whole explicit run with these options, copies made in `/store`.
fn run_opts(lib: &mut Library, root: &str, o: Options, files: &[Found], plan: Plan) -> Landed {
    let copying = !o.in_place;
    let id = lib.open_folder(PathBuf::from(root), o, &plan, 1_000);
    let planned = lib
        .plan_walk(id, files.to_vec(), Asked::Explicitly, plan)
        .unwrap();
    let copies: HashMap<PathBuf, Copied> = (planned.to_copy(copying).into_iter())
        .map(|f| (f.path.clone(), copy_of(f)))
        .collect();
    lib.land_walk(planned, &copies, 2_000)
}

fn copy_of(f: &Found) -> Copied {
    Copied {
        path: Path::new("/store").join(f.path.file_name().unwrap()),
        measure: f.measure,
    }
}

fn shaped(groups: bool) -> Options {
    Options {
        groups,
        ..opts(Mode::InPlace)
    }
}

fn book_titled(lib: &Library, title: &str) -> BookId {
    lib.books().find(|b| b.title.value == title).unwrap().id
}

fn plan_of(gate: Gate) -> (PathBuf, Plan) {
    match gate {
        Gate::Walk { root, plan } => (root, plan),
        Gate::Clash(c) => panic!("asked about {}", c.name),
    }
}

#[test]
fn a_pick_inside_a_tree_walks_the_tree_and_lights_its_rung() {
    let mut lib = Library::new(DEVICE);
    import(
        &mut lib,
        "/root",
        Mode::InPlace,
        &[found("/root/fic/a.pdf", 1)],
    );
    let fic = shelf_named(&lib, "fic");
    let (root, plan) = plan_of(lib.gate(Path::new("/root/fic"), &opts(Mode::InPlace)));
    assert_eq!(root, PathBuf::from("/root"), "the walk runs on the tree");
    assert_eq!(plan.continuation, Some(fic));
    assert_eq!(plan.rung, "fic");
    assert_eq!(plan.fold, None);
    let files = [found("/root/fic/a.pdf", 1), found("/root/fic/b.pdf", 2)];
    let landed = run_opts(&mut lib, "/root", opts(Mode::InPlace), &files, plan);
    assert_eq!(
        landed.root,
        Some(fic),
        "the run lights the pick's own shelf"
    );
    assert_eq!(titles_on(&lib, fic), ["a", "b"]);
    assert_eq!(lib.folders().count(), 1, "no second ledger for the pick");
}

#[test]
fn a_re_pick_of_a_tree_root_continues_its_root_shelf() {
    let mut lib = Library::new(DEVICE);
    import(&mut lib, "/root", Mode::InPlace, &[found("/root/a.pdf", 1)]);
    let root = shelf_named(&lib, "root");
    let (_, plan) = plan_of(lib.gate(Path::new("/root"), &opts(Mode::InPlace)));
    assert_eq!(plan.continuation, Some(root));
    assert_eq!(plan.rung, "");
}

#[test]
fn a_rung_taken_out_folds_back_into_its_tree_when_picked() {
    let mut lib = Library::new(DEVICE);
    let (tree, _) = import(
        &mut lib,
        "/root",
        Mode::InPlace,
        &[found("/root/fic/a.pdf", 1)],
    );
    let fic = shelf_named(&lib, "fic");
    lib.remove_shelves(&[fic], false, Data::Keep, 3);
    let (root, plan) = plan_of(lib.gate(Path::new("/root/fic"), &opts(Mode::InPlace)));
    assert_eq!(root, PathBuf::from("/root/fic"));
    let fold = Fold {
        tree,
        rel: "fic".into(),
    };
    assert_eq!(plan.fold, Some(fold));
    let files = [found("/root/fic/a.pdf", 1)];
    let landed = run_opts(&mut lib, "/root/fic", opts(Mode::InPlace), &files, plan);
    let seat = landed.root.unwrap();
    let shelf = lib.shelf(seat).unwrap();
    assert_eq!(shelf.name.value, "fic");
    let kind = ShelfKind::Rung {
        folder: tree,
        key: "fic".into(),
    };
    assert_eq!(shelf.kind.value, kind, "the seat is the tree's rung again");
    assert_eq!(shelf.parent.value, Some(shelf_named(&lib, "root")));
    assert_eq!(titles_on(&lib, seat), ["a"]);
    assert_eq!(lib.folders().count(), 1, "the pick's own ledger retired");
}

#[test]
fn a_held_name_asks_and_offers_by_how_the_folder_is_read() {
    let mut lib = Library::new(DEVICE);
    let shelf = lib.create_shelf("books", None, 1).unwrap();
    let Gate::Clash(copied) = lib.gate(Path::new("/books"), &opts(Mode::Copy)) else {
        panic!("no ask");
    };
    assert_eq!(copied.existing, shelf);
    assert!(!copied.own, "a shelf of the reader's own");
    assert_eq!(copied.offers(), Placement::SHELF_STORED);
    let Gate::Clash(read) = lib.gate(Path::new("/books"), &opts(Mode::InPlace)) else {
        panic!("no ask");
    };
    assert_eq!(read.offers(), Placement::SHELF_IN_PLACE);
}

#[test]
fn a_folder_s_own_shelf_is_its_own_on_a_second_import() {
    let mut lib = Library::new(DEVICE);
    import(&mut lib, "/books", Mode::Copy, &[found("/books/a.pdf", 1)]);
    let Gate::Clash(clash) = lib.gate(Path::new("/books"), &opts(Mode::Copy)) else {
        panic!("no ask");
    };
    assert!(clash.own);
}

#[test]
fn the_shelf_answers_merge_link_and_add_as_new() {
    let mut lib = Library::new(DEVICE);
    let shelf = lib.create_shelf("books", None, 1).unwrap();
    let Gate::Clash(clash) = lib.gate(Path::new("/books"), &opts(Mode::InPlace)) else {
        panic!("no ask");
    };
    let Answered::Walk { plan, .. } = lib.answer_clash(&clash, Placement::Merge, 2) else {
        panic!("merge walks");
    };
    assert_eq!(plan.into, Some(shelf));
    let linked = lib.answer_clash(&clash, Placement::LinkOnly, 3);
    assert_eq!(linked, Answered::Done(Some(shelf)));
    assert_eq!(lib.links().filter(|l| l.alive()).count(), 1);
    let refused = lib.answer_clash(&clash, Placement::Replace, 4);
    assert_eq!(refused, Answered::Done(None), "not offered for in place");
    let Gate::Clash(copy) = lib.gate(Path::new("/books"), &opts(Mode::Copy)) else {
        panic!("no ask");
    };
    let Answered::Walk { plan, .. } = lib.answer_clash(&copy, Placement::KeepBoth, 5) else {
        panic!("as new walks");
    };
    assert_eq!(plan.rename.as_deref(), Some("books_1"));
}

#[test]
fn replace_takes_the_shelf_s_books_out_and_files_the_folder_there() {
    let mut lib = Library::new(DEVICE);
    import(&mut lib, "/old", Mode::Copy, &[found("/old/x.pdf", 9)]);
    let shelf = lib.create_shelf("books", None, 1).unwrap();
    let x = book_titled(&lib, "x");
    lib.file(x, shelf, 2);
    let Gate::Clash(clash) = lib.gate(Path::new("/books"), &opts(Mode::Copy)) else {
        panic!("no ask");
    };
    let Answered::Walk { plan, .. } = lib.answer_clash(&clash, Placement::Replace, 3) else {
        panic!("replace walks");
    };
    assert!(
        !lib.book(x).is_some_and(|b| b.alive()),
        "the shelf's book left"
    );
    let swept = lib.take_swept();
    assert_eq!(
        swept,
        [std::path::PathBuf::from("/store/x.pdf")],
        "its copy goes"
    );
    assert_eq!(plan.into, Some(shelf));
    let files = [found("/books/a.pdf", 1)];
    run_opts(&mut lib, "/books", opts(Mode::Copy), &files, plan);
    assert_eq!(titles_on(&lib, shelf), ["a"]);
}

#[test]
fn replace_turns_a_folder_s_own_tree_into_copies_with_its_data() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1), found("/books/b.pdf", 2)];
    import(&mut lib, "/books", Mode::InPlace, &files);
    let a = book_titled(&lib, "a");
    let Gate::Clash(clash) = lib.gate(Path::new("/books"), &opts(Mode::Copy)) else {
        panic!("no ask");
    };
    assert!(clash.own);
    let Answered::Walk { plan, .. } = lib.answer_clash(&clash, Placement::Replace, 3) else {
        panic!("replace walks");
    };
    assert_eq!(plan, Plan::default());
    run_opts(&mut lib, "/books", opts(Mode::Copy), &files, plan);
    let shelf = shelf_named(&lib, "books");
    assert_eq!(titles_on(&lib, shelf), ["a", "b"]);
    let book = lib.book(a).unwrap();
    assert!(book.alive(), "the same book came back");
    assert_eq!(book.home().unwrap().origin, Origin::Store);
    assert_eq!(lib.folders().count(), 1);
}

#[test]
fn a_subfolder_imported_on_its_own_folds_into_the_tree_around_it() {
    let mut lib = Library::new(DEVICE);
    let (sub, _) = import(
        &mut lib,
        "/root/sub",
        Mode::InPlace,
        &[found("/root/sub/a.pdf", 1)],
    );
    let shelf = shelf_named(&lib, "sub");
    let files = [found("/root/sub/a.pdf", 1), found("/root/b.pdf", 2)];
    let (tree, _) = import(&mut lib, "/root", Mode::InPlace, &files);
    assert!(lib.folder(sub).is_none(), "the member's ledger retired");
    let kind = ShelfKind::Rung {
        folder: tree,
        key: "sub".into(),
    };
    assert_eq!(lib.shelf(shelf).unwrap().kind.value, kind);
    let root = shelf_named(&lib, "root");
    assert_eq!(lib.shelf(shelf).unwrap().parent.value, Some(root));
    assert_eq!(titles_on(&lib, shelf), ["a"]);
    assert_eq!(titles_on(&lib, root), ["b"]);
    let named_sub = lib.shelves().filter(|s| s.name.value == "sub").count();
    assert_eq!(named_sub, 1, "the member's shelf is reused, not minted");
    let a = lib.book(book_titled(&lib, "a")).unwrap();
    assert_eq!(a.home().unwrap().origin, Origin::Folder(tree));
}

#[test]
fn a_member_the_walk_found_nothing_under_stays_its_own() {
    let mut lib = Library::new(DEVICE);
    let (sub, _) = import(
        &mut lib,
        "/root/sub",
        Mode::InPlace,
        &[found("/root/sub/a.pdf", 1)],
    );
    import(&mut lib, "/root", Mode::InPlace, &[found("/root/b.pdf", 2)]);
    assert!(lib.folder(sub).is_some());
}

#[test]
fn a_copying_subfolder_is_no_member() {
    let mut lib = Library::new(DEVICE);
    let (sub, _) = import(
        &mut lib,
        "/root/sub",
        Mode::Copy,
        &[found("/root/sub/a.pdf", 1)],
    );
    import(
        &mut lib,
        "/root",
        Mode::InPlace,
        &[found("/root/sub/a.pdf", 1)],
    );
    assert!(lib.folder(sub).is_some());
}

#[test]
fn a_re_import_with_the_other_shape_re_files_the_tree() {
    let mut lib = Library::new(DEVICE);
    let files = [
        found("/root/scifi/dune.pdf", 1),
        found("/root/notes.pdf", 2),
    ];
    run_opts(&mut lib, "/root", shaped(true), &files, Plan::default());
    let root = shelf_named(&lib, "root");
    let scifi = shelf_named(&lib, "scifi");
    let reader = lib.create_shelf("Read next", Some(scifi), 3).unwrap();
    run_opts(&mut lib, "/root", shaped(false), &files, Plan::default());
    assert!(
        lib.shelf(scifi).is_none(),
        "the rung the shape has no place for went"
    );
    assert_eq!(titles_on(&lib, root), ["dune", "notes"]);
    let reader = lib.shelf(reader).unwrap();
    assert_eq!(reader.parent.value, Some(root), "a reader's shelf comes up");
    run_opts(&mut lib, "/root", shaped(true), &files, Plan::default());
    let scifi = shelf_named(&lib, "scifi");
    assert_eq!(titles_on(&lib, scifi), ["dune"]);
    assert_eq!(titles_on(&lib, root), ["notes"]);
    assert_eq!(lib.shelf(scifi).unwrap().parent.value, Some(root));
}

#[test]
fn the_same_shape_moves_nothing() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/root/scifi/dune.pdf", 1)];
    run_opts(&mut lib, "/root", shaped(true), &files, Plan::default());
    let shelves = lib.shelves().count();
    run_opts(&mut lib, "/root", shaped(true), &files, Plan::default());
    assert_eq!(lib.shelves().count(), shelves);
}

fn in_place_dune(lib: &mut Library) -> (FolderId, BookId) {
    let (id, landed) = import(lib, "/books", Mode::InPlace, &[found("/books/dune.pdf", 1)]);
    (id, landed.placed[0])
}

#[test]
fn a_loose_file_a_tree_reads_asks_open_or_keep_both() {
    let mut lib = Library::new(DEVICE);
    let (id, dune) = in_place_dune(&mut lib);
    let shelf = shelf_named(&lib, "books");
    let file = found("/books/dune.pdf", 1);
    let screened = lib.screen_files(vec![file.clone()], Some(shelf), 3);
    assert!(screened.clean.is_empty());
    let ask = &screened.asks[0];
    assert_eq!(ask.why, Why::Covered(id));
    assert_eq!(ask.existing, Entry::Book(dune));
    assert_eq!(ask.offers(), Placement::COVERED);
    let shown = lib.answer_file(ask, Placement::Open, None, 4);
    assert_eq!(shown, Answer::Reveal(Entry::Book(dune)));
    let copy = copy_of(&file);
    let Answer::Landed(kept) = lib.answer_file(ask, Placement::KeepBoth, Some(&copy), 5) else {
        panic!("keep both lands");
    };
    assert_ne!(kept, dune);
    assert_eq!(lib.book(kept).unwrap().title.value, "dune_1");
    assert_eq!(
        lib.book(kept).unwrap().home().unwrap().origin,
        Origin::Store
    );
}

#[test]
fn a_loose_file_a_tree_removed_comes_back_in_place() {
    let mut lib = Library::new(DEVICE);
    let (id, dune) = in_place_dune(&mut lib);
    let shelf = shelf_named(&lib, "books");
    lib.remove_books(&[dune], Data::Keep, 3);
    let screened = lib.screen_files(vec![found("/books/dune.pdf", 1)], None, 4);
    assert_eq!(screened.restored.len(), 1);
    assert!(screened.asks.is_empty() && screened.clean.is_empty());
    let back = lib.book(screened.restored[0]).unwrap();
    assert_eq!(back.home().unwrap().origin, Origin::Folder(id));
    assert_eq!(titles_on(&lib, shelf), ["dune"], "on the shelf it left");
    assert!(lib.folder(id).unwrap().tombstone(back.content).is_none());
}

#[test]
fn held_content_asks_unless_its_book_is_missing() {
    let mut lib = Library::new(DEVICE);
    import(&mut lib, "/a", Mode::Copy, &[found("/a/x.pdf", 1)]);
    let screened = lib.screen_files(vec![found("/b/x.pdf", 1)], None, 3);
    assert_eq!(screened.asks[0].why, Why::AlreadyHave);
    lib.check_present(|_| false);
    let screened = lib.screen_files(vec![found("/b/x.pdf", 1)], None, 4);
    assert!(screened.asks.is_empty());
    assert_eq!(screened.clean.len(), 1, "a missing book is no answer");
}

#[test]
fn a_name_the_level_holds_asks_and_may_link() {
    let mut lib = Library::new(DEVICE);
    import(&mut lib, "/a", Mode::Copy, &[found("/a/dune.pdf", 1)]);
    let shelf = shelf_named(&lib, "a");
    let screened = lib.screen_files(vec![found("/b/dune.pdf", 2)], Some(shelf), 3);
    let ask = &screened.asks[0];
    assert_eq!(ask.why, Why::Name);
    assert_eq!(ask.offers(), Placement::FILE);
    assert_eq!(
        lib.answer_file(ask, Placement::LinkOnly, None, 4),
        Answer::Linked
    );
    let link = lib.links().find(|l| l.alive()).unwrap();
    assert_eq!(link.level.value, Some(shelf));
    assert_eq!(
        lib.answer_file(ask, Placement::Replace, None, 5),
        Answer::Nothing
    );
}

#[test]
fn a_clean_copy_spends_the_log_and_keeps_its_name() {
    let mut lib = Library::new(DEVICE);
    let (id, dune) = in_place_dune(&mut lib);
    lib.rename_book(dune, "Dune Messiah", 2);
    lib.remove_books(&[dune], Data::Keep, 3);
    let file = found("/elsewhere/dune.pdf", 1);
    let screened = lib.screen_files(vec![file.clone()], None, 4);
    assert_eq!(screened.clean.len(), 1);
    let landed = lib.land_copies(&[(file.clone(), copy_of(&file))], None, 5);
    let book = lib.book(landed[0]).unwrap();
    assert_eq!(book.title.value, "Dune Messiah");
    assert!(
        lib.folder(id)
            .unwrap()
            .tombstone(file.measure.content)
            .is_none()
    );
}

#[test]
fn a_merge_replace_takes_the_old_book_s_slot_and_drops_it() {
    let mut lib = Library::new(DEVICE);
    let shelf = lib.create_shelf("books", None, 1).unwrap();
    let (_, held) = import(
        &mut lib,
        "/old",
        Mode::InPlace,
        &[found("/old/dune.pdf", 1)],
    );
    let old = held.placed[0];
    lib.file(old, shelf, 2);
    let plan = Plan {
        into: Some(shelf),
        ..Plan::default()
    };
    let files = vec![found("/books/dune.pdf", 2)];
    let id = lib.open_folder("/books".into(), opts(Mode::InPlace), &plan, 3);
    let planned = lib.plan_walk(id, files, Asked::Explicitly, plan).unwrap();
    let ask = planned.asks[0].clone();
    lib.land_walk(planned, &HashMap::new(), 4);
    let Answer::Landed(new) = lib.answer_merge(&ask, Placement::Replace, None, 5) else {
        panic!("replace lands");
    };
    assert!(!lib.book(old).is_some_and(|b| b.alive()));
    assert_eq!(titles_on(&lib, shelf), ["dune"]);
    assert_eq!(lib.book(new).unwrap().content, crate::ContentId(2));
    let Answer::Landed(both) = lib.answer_merge(&ask, Placement::KeepBoth, None, 6) else {
        panic!("keep both lands");
    };
    assert_eq!(lib.book(both).unwrap().title.value, "dune_1");
}

#[test]
fn a_copy_pick_over_a_tree_read_in_place_lands_beside_it() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1)];
    let (tree, _) = import(&mut lib, "/books", Mode::InPlace, &files);
    let Gate::Clash(clash) = lib.gate(Path::new("/books"), &opts(Mode::Copy)) else {
        panic!("no ask");
    };
    let Answered::Walk { plan, .. } = lib.answer_clash(&clash, Placement::KeepBoth, 2) else {
        panic!("as new walks");
    };
    assert!(plan.beside);
    let (copies, _) = run(
        &mut lib,
        "/books",
        Mode::Copy,
        &files,
        Asked::Explicitly,
        plan,
    );
    assert_ne!(copies, tree, "a ledger of its own");
    assert!(
        lib.folder(tree).unwrap().mode().in_place(),
        "the tree is left alone"
    );
    let beside = shelf_named(&lib, "books_1");
    assert_eq!(titles_on(&lib, beside), ["a"]);
}
