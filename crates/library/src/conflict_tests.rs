use std::path::PathBuf;

use crate::conflict::{Arrival, Entry, Placement, collide, collide_shelf, next_name};
use crate::id::{BookId, LinkId, ShelfId};

fn book(n: u128) -> Entry {
    Entry::Book(BookId(n))
}

#[test]
fn a_name_already_on_the_level_asks_and_an_empty_level_never_does() {
    let at = Arrival::import(PathBuf::from("/in/Dune.pdf"), None, None);
    assert_eq!(at.name, "Dune", "an arrival carries its own name");
    assert_eq!(collide([(book(1), "dune")], &at), Some(book(1)));
    assert_eq!(collide([(book(1), "Hyperion")], &at), None);
    assert_eq!(collide(std::iter::empty(), &at), None);
}

#[test]
fn a_link_neither_asks_nor_blocks_and_a_row_never_meets_itself() {
    let link = Entry::Link(LinkId(4));
    let at = Arrival::moved(book(1), "Dune", Some(ShelfId(2)), None);
    assert_eq!(collide([(link, "Dune"), (book(1), "Dune")], &at), None);
    assert_eq!(collide([(book(3), "Dune")], &at), Some(book(3)));
}

#[test]
fn a_counter_copy_beside_its_original_never_asks() {
    let at = Arrival::moved(book(2), "Dune_1", None, None);
    assert_eq!(collide([(book(1), "Dune")], &at), None);
}

#[test]
fn the_counter_counts_against_the_level_it_lands_on() {
    assert_eq!(next_name(["Dune", "Dune_1"], "Dune"), "Dune_2");
    assert_eq!(next_name(["Hyperion"], "Dune"), "Dune");
}

#[test]
fn a_shelf_name_the_level_already_holds_asks() {
    let kids = [(ShelfId(1), "Fiction"), (ShelfId(2), "History")];
    assert_eq!(collide_shelf(kids, " fiction"), Some(ShelfId(1)));
    assert_eq!(collide_shelf(kids, "Poetry"), None);
}

#[test]
fn a_move_names_the_level_it_leaves_and_nothing_else_does() {
    let at = Arrival::moved(book(1), "Dune", None, Some(0)).leaving(Some(ShelfId(5)));
    assert_eq!(at.from, Some(Some(ShelfId(5))));
    assert!(!at.is_import());
    assert_eq!(
        Arrival::import(PathBuf::from("/a.pdf"), None, None).from,
        None
    );
}

#[test]
fn each_ask_offers_only_the_answers_it_can_apply() {
    assert!(
        !Placement::FILE.contains(&Placement::Replace),
        "a file displaces nothing"
    );
    assert!(!Placement::FILE.contains(&Placement::Merge));
    assert!(!Placement::MOVE.contains(&Placement::Open));
    assert!(!Placement::MOVE_KEEPING_BOTH.contains(&Placement::Replace));
    assert_eq!(Placement::COVERED.len(), 2);
    assert!(!Placement::SHELF_IN_PLACE.contains(&Placement::KeepBoth));
}
