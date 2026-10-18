use std::path::Path;

use crate::words::{free_name, human_age, human_size, name_or_stem, plural, same_name};

const KB: u64 = 1024;
const MB: u64 = KB * KB;
const GB: u64 = MB * KB;
const MINUTE: u64 = 60_000;

#[test]
fn a_size_keeps_one_decimal_until_it_does_not_need_one() {
    assert_eq!(human_size(0), "0 B");
    assert_eq!(human_size(1023), "1023 B");
    assert_eq!(human_size(KB), "1.0 KB");
    assert_eq!(human_size(30 * KB), "30 KB");
    assert_eq!(human_size(3 * MB + 200 * KB), "3.2 MB");
    assert_eq!(human_size(12 * MB + 400 * KB), "12 MB");
    assert_eq!(human_size(14 * GB), "14 GB");
    assert_eq!(human_size(2048 * GB), "2.0 TB");
    assert_eq!(
        human_size(u64::MAX),
        "16777216 TB",
        "the units stop at the last"
    );
}

#[test]
fn an_age_is_coarse_where_coarse_is_enough() {
    let now = 1_700_000_000_000;
    assert_eq!(human_age(now - 30_000, now), "just now");
    assert_eq!(human_age(now - MINUTE, now), "1 minute ago");
    assert_eq!(human_age(now - 3 * MINUTE, now), "3 minutes ago");
    assert_eq!(human_age(now - 60 * MINUTE, now), "1 hour ago");
    assert_eq!(human_age(now - 3 * 24 * 60 * MINUTE, now), "3 days ago");
    assert_eq!(human_age(now - 21 * 24 * 60 * MINUTE, now), "3 weeks ago");
    assert_eq!(human_age(now + MINUTE, now), "just now", "never negative");
}

#[test]
fn a_count_reads_as_one_thing_or_many() {
    assert_eq!(plural(1, "book", "books"), "1 book");
    assert_eq!(plural(0, "shelf", "shelves"), "0 shelves");
    assert_eq!(plural(2, "shelf", "shelves"), "2 shelves");
}

#[test]
fn a_blank_name_falls_back_to_the_stem() {
    let path = Path::new("/books/rust-book.pdf");
    assert_eq!(name_or_stem("Dune", path), "Dune");
    assert_eq!(name_or_stem("  ", path), "rust-book");
}

#[test]
fn a_taken_name_counts_up_to_the_first_free_one() {
    assert_eq!(free_name("Dune", ["Hyperion"]), "Dune");
    assert_eq!(
        free_name("Dune", ["dune "]),
        "Dune_1",
        "case and spaces blind"
    );
    assert_eq!(free_name("Dune", ["Dune", "Dune_1"]), "Dune_2");
    assert_eq!(
        free_name("Dune_1", ["Dune_1", "Dune"]),
        "Dune_2",
        "a copy of a copy"
    );
    assert_eq!(free_name("Dune", ["Dune", "Dune_2"]), "Dune_1", "gaps fill");
    assert_eq!(free_name(" ", [""]), "Book_1");
    assert!(same_name(" Sci-Fi", "sci-fi "));
}
