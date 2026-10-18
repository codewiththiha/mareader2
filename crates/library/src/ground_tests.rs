use std::path::{Path, PathBuf};

use crate::clock::{Field, NEVER};
use crate::extra::Extra;
use crate::folder::{Folder, Mode, Options};
use crate::ground::{
    Coverage, Seat, Shelves, ancestors, can_nest, converts_on_move, covering, departing_moves,
    departs_on_move, family, mode_of, seat_of, shelf_tracked, subtree, tree_of,
};
use crate::id::{ContentId, FolderId, ShelfId};
use crate::shelves::{Shelf, ShelfKind};

fn s(n: u128) -> ShelfId {
    ShelfId(n)
}

fn shelf(n: u128, parent: Option<u128>, kind: ShelfKind) -> Shelf {
    Shelf {
        id: s(n),
        name: Field::new(format!("S{n}"), NEVER),
        parent: Field::new(parent.map(s), NEVER),
        from: None,
        added: 0,
        key: Field::default(),
        kind: Field::new(kind, NEVER),
        moved: Field::default(),
        removed: None,
        restored: None,
        extra: Extra::default(),
    }
}

fn rung(key: &str) -> ShelfKind {
    ShelfKind::Rung {
        folder: FolderId(1),
        key: key.into(),
    }
}

fn map(list: Vec<Shelf>) -> Shelves {
    list.into_iter().map(|sh| (sh.id, sh)).collect()
}

/// `/books`: root 1, rungs a (2) and a/b (3), own 4 in a.
fn books(mode: Mode) -> (Shelves, Vec<Folder>) {
    let mut opts = Options::default();
    opts.set_mode(mode);
    let mut f = Folder::new(FolderId(1), PathBuf::from("/books"), opts, 0);
    f.shelf_map.insert(String::new(), s(1));
    f.shelf_map.insert("a".into(), s(2));
    f.shelf_map.insert("a/b".into(), s(3));
    f.placed.insert(ContentId(7));
    let shelves = map(vec![
        shelf(1, None, rung("")),
        shelf(2, Some(1), rung("a")),
        shelf(3, Some(2), rung("a/b")),
        shelf(4, Some(2), ShelfKind::Own),
        shelf(5, None, ShelfKind::Own),
        shelf(6, Some(2), ShelfKind::Departed),
        shelf(7, Some(6), ShelfKind::Own),
    ]);
    (shelves, vec![f])
}

#[test]
fn the_way_out_of_a_shelf_is_the_chain_above_it() {
    let (sh, _) = books(Mode::InPlace);
    assert_eq!(ancestors(&sh, s(3)), vec![s(2), s(1)]);
    assert!(ancestors(&sh, s(1)).is_empty());
    let mut looped = sh.clone();
    if let Some(top) = looped.get_mut(&s(1)) {
        top.parent.value = Some(s(3));
    }
    assert_eq!(ancestors(&looped, s(3)).len(), 2, "a loop ends the walk");
}

#[test]
fn a_shelf_cannot_be_filed_inside_itself_or_its_own_children() {
    let (sh, _) = books(Mode::InPlace);
    assert!(!can_nest(&sh, s(2), Some(s(2))));
    assert!(!can_nest(&sh, s(2), Some(s(3))));
    assert!(can_nest(&sh, s(3), Some(s(5))));
    assert!(can_nest(&sh, s(3), None));
}

#[test]
fn the_subtree_is_everything_below_and_never_the_root_itself() {
    let (sh, _) = books(Mode::InPlace);
    let mut below = subtree(&sh, &[s(2)]);
    below.sort();
    assert_eq!(below, vec![s(3), s(4), s(6), s(7)]);
}

#[test]
fn a_shelf_the_reader_made_answers_for_the_tree_it_stands_inside() {
    let (sh, f) = books(Mode::InPlace);
    let seat = |folder, rung: &str| Seat {
        folder,
        rung: rung.into(),
    };
    assert_eq!(tree_of(&sh, &f, s(3)), Some(seat(FolderId(1), "a/b")));
    assert_eq!(tree_of(&sh, &f, s(4)), Some(seat(FolderId(1), "a")));
    assert_eq!(tree_of(&sh, &f, s(5)), None, "the reader's own ground");
    assert_eq!(
        tree_of(&sh, &f, s(6)),
        None,
        "a departed shelf answers no tree"
    );
    assert_eq!(tree_of(&sh, &f, s(7)), None, "nor does a shelf inside one");
}

#[test]
fn a_copying_tree_gives_the_badge_but_no_seat() {
    let (sh, f) = books(Mode::Copy);
    assert_eq!(seat_of(&sh, &f, s(2)), None);
    assert_eq!(mode_of(&sh, &f, s(2)), Some(Mode::Copy));
    assert_eq!(
        mode_of(&sh, &f, s(6)),
        Some(Mode::Copy),
        "a departed shelf is copies"
    );
    assert_eq!(mode_of(&sh, &f, s(5)), None);
}

#[test]
fn a_watch_dot_is_the_rungs_answer_not_the_trees() {
    let (sh, mut f) = books(Mode::InPlace);
    f[0].set_tracking("a/b", true);
    assert!(shelf_tracked(&sh, &f, s(3)));
    assert!(!shelf_tracked(&sh, &f, s(2)));
    assert!(!shelf_tracked(&sh, &f, s(5)));
}

#[test]
fn only_a_linked_book_of_a_reading_folder_owes_the_copy() {
    let (_, f) = books(Mode::InPlace);
    let path = Path::new("/books/a/x.pdf");
    let content = ContentId(7);
    assert!(
        !converts_on_move(&f, content, path, Some(s(2))),
        "its own rung"
    );
    assert!(
        converts_on_move(&f, content, path, Some(s(3))),
        "another rung departs"
    );
    assert!(converts_on_move(&f, content, path, Some(s(5))));
    assert!(
        converts_on_move(&f, content, path, None),
        "the top level is nobody's ground"
    );
    assert!(
        !converts_on_move(&f, ContentId(9), path, None),
        "a book it never placed"
    );
    let (_, copies) = books(Mode::Copy);
    assert!(!converts_on_move(&copies, content, path, None));
}

#[test]
fn a_rung_of_a_reading_folder_departs_whatever_it_leaves_for() {
    let (sh, f) = books(Mode::InPlace);
    assert!(
        !departs_on_move(&sh, &f, s(3), Some(s(2))),
        "a reorder on its seat"
    );
    assert!(departs_on_move(&sh, &f, s(3), Some(s(5))));
    assert!(departs_on_move(&sh, &f, s(3), None));
    assert!(
        !departs_on_move(&sh, &f, s(4), None),
        "the reader's own shelf moves freely"
    );
    let (sh, copies) = books(Mode::Copy);
    assert!(
        !departs_on_move(&sh, &copies, s(3), None),
        "a copying folder's too"
    );
}

#[test]
fn a_return_to_its_seat_copies_nothing() {
    let (mut sh, f) = books(Mode::InPlace);
    if let Some(b) = sh.get_mut(&s(3)) {
        b.parent.value = Some(s(5));
    }
    assert!(!departs_on_move(&sh, &f, s(3), Some(s(2))), "back home");
}

#[test]
fn a_departing_shelf_inside_another_departing_one_rides_with_it() {
    let (sh, f) = books(Mode::InPlace);
    let (clean, departing) = departing_moves(&sh, &f, &[s(2), s(3), s(4)], Some(s(5)));
    assert_eq!(clean, vec![s(4)]);
    assert_eq!(departing, vec![s(2)], "3 rides inside 2");
}

#[test]
fn a_tree_covers_its_own_root_and_a_rung_inside_it() {
    let (sh, f) = books(Mode::InPlace);
    assert_eq!(
        covering(&sh, &f, Path::new("/books/a")),
        Some(Coverage {
            folder: FolderId(1),
            rel: "a".into(),
            shelf: s(2),
        })
    );
    assert_eq!(covering(&sh, &f, Path::new("/books/z")), None);
    let (sh, copies) = books(Mode::Copy);
    assert_eq!(
        covering(&sh, &copies, Path::new("/books")),
        None,
        "a copying tree covers nothing"
    );
}

#[test]
fn the_family_is_the_deepest_tree_whose_rung_for_the_ground_is_free() {
    let (sh, f) = books(Mode::InPlace);
    assert_eq!(
        family(&sh, &f, Path::new("/books/a/c")),
        Some((FolderId(1), "a/c".into()))
    );
    assert_eq!(
        family(&sh, &f, Path::new("/books/a")),
        None,
        "a standing rung"
    );
    let mut gone = sh.clone();
    gone.remove(&s(1));
    assert_eq!(
        family(&gone, &f, Path::new("/books/a/c")),
        None,
        "a taken-out root leaves no family behind it"
    );
}
