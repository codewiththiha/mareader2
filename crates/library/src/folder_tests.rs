use std::path::{Path, PathBuf};

use crate::book::Format;
use crate::folder::{
    Folder, MIN_SIZE_DEFAULT, MIN_SIZE_MAX, Mode, Options, Rungs, dir_of_rung, key_chain,
    key_in_zone, parent_key, rel_under,
};
use crate::id::{ShelfId, SourceId};

fn folder(opts: Options) -> Folder {
    Folder::new(SourceId(1), PathBuf::from("/books"), opts, 0)
}

#[test]
fn a_key_is_a_chain_of_rungs_root_first() {
    assert_eq!(key_chain(""), vec![""]);
    assert_eq!(key_chain("a"), vec!["", "a"]);
    assert_eq!(key_chain("a/b/c"), vec!["", "a", "a/b", "a/b/c"]);
    assert_eq!(parent_key(""), None);
    assert_eq!(parent_key("a"), Some(""));
    assert_eq!(parent_key("a/b"), Some("a"));
}

#[test]
fn inside_a_folder_starts_on_a_directory_edge() {
    let root = Path::new("/books");
    assert_eq!(rel_under(Path::new("/books"), root).as_deref(), Some(""));
    assert_eq!(
        rel_under(Path::new("/books/a/b"), root).as_deref(),
        Some("a/b")
    );
    assert_eq!(rel_under(Path::new("/bookshelf/a"), root), None);
    assert_eq!(dir_of_rung(root, "a/b"), PathBuf::from("/books/a/b"));
    assert_eq!(dir_of_rung(root, ""), PathBuf::from("/books"));
}

#[test]
fn a_zone_holds_its_own_key_and_the_rungs_below_it() {
    assert!(key_in_zone("a/b", ""));
    assert!(key_in_zone("a", "a"));
    assert!(key_in_zone("a/b", "a"));
    assert!(!key_in_zone("ab", "a"));
    assert!(!key_in_zone("", "a"));
}

#[test]
fn a_file_stands_on_the_rung_its_own_subfolder_names() {
    let f = folder(Options::default());
    assert_eq!(f.rung_of(Path::new("/books/x.pdf")).as_deref(), Some(""));
    assert_eq!(
        f.rung_of(Path::new("/books/a/b/x.pdf")).as_deref(),
        Some("a/b")
    );
    assert_eq!(f.rung_of(Path::new("/elsewhere/x.pdf")), None);
}

#[test]
fn a_folder_that_does_not_group_has_one_rung_for_every_file() {
    let f = folder(Options {
        groups: false,
        ..Options::default()
    });
    assert_eq!(
        f.rung_of(Path::new("/books/a/b/x.pdf")).as_deref(),
        Some("")
    );
}

#[test]
fn a_shape_answered_for_one_rung_cuts_the_rungs_under_it_alone() {
    let mut f = folder(Options {
        groups: false,
        ..Options::default()
    });
    f.shelf_map.insert(String::new(), ShelfId(1));
    f.shelf_map.insert("Fiction".into(), ShelfId(2));
    assert_eq!(f.rung_for("Fiction/SciFi"), "", "one shelf files all");
    let rungs = |f: &Folder, p: &str| f.rungs_for(Path::new(p)).0;
    assert_eq!(rungs(&f, "/books/Reference/x.pdf"), Some(ShelfId(1)));
    f.set_shape("Fiction", true);
    assert_eq!(f.rung_for("Fiction"), "Fiction");
    assert_eq!(f.rung_for("Fiction/SciFi"), "Fiction/SciFi");
    assert_eq!(f.rung_for("Reference"), "", "the rest stays where it was");
    assert_eq!(
        rungs(&f, "/books/Fiction/SciFi/dune.pdf"),
        None,
        "no shelf stands for the newly cut rung yet"
    );
    assert_eq!(rungs(&f, "/books/Fiction/other.pdf"), Some(ShelfId(2)));
}

#[test]
fn the_roots_answer_stands_for_the_whole_tree() {
    let mut f = folder(Options::default());
    f.set_shape("a", false);
    f.set_shape("", true);
    assert!(f.shapes.is_empty(), "the root's answer clears the zone");
    assert!(f.opts.groups);
    assert_eq!(f.rung_for("a/b"), "a/b");
}

#[test]
fn the_rungs_a_path_names_are_its_own_and_the_root() {
    let mut f = folder(Options::default());
    f.shelf_map.insert(String::new(), ShelfId(1));
    f.shelf_map.insert("a".into(), ShelfId(2));
    assert_eq!(
        f.rungs_for(Path::new("/books/a/x.pdf")),
        (Some(ShelfId(2)), Some(ShelfId(1)))
    );
    assert_eq!(
        f.rungs_for(Path::new("/books/b/x.pdf")),
        (None, Some(ShelfId(1))),
        "a rung with no shelf names none"
    );
    assert_eq!(f.rungs_for(Path::new("/other/x.pdf")), (None, None));
}

#[test]
fn the_defaults_are_the_ones_the_sheet_opens_on() {
    let o = Options::default();
    assert_eq!(o.min_size, MIN_SIZE_DEFAULT);
    assert_eq!(o.min_size_label(), "30 KB");
    assert_eq!(o.mode(), Mode::InPlace, "read in place, not watched");
    assert!(o.groups && o.include);
    assert_eq!(o.formats.len(), Format::ALL.len());
}

#[test]
fn the_size_dial_steps_in_kb_and_stops_at_its_bounds() {
    let mut o = Options::default();
    o.step_min_size(1);
    assert_eq!(o.min_size_label(), "40 KB");
    o.step_min_size(-100);
    assert_eq!(o.min_size, 0);
    o.step_min_size(500);
    assert_eq!(o.min_size, MIN_SIZE_MAX);
}

#[test]
fn a_file_counts_by_its_format_and_a_strict_size() {
    let mut o = Options {
        formats: [Format::Pdf].into_iter().collect(),
        ..Options::default()
    };
    assert!(o.admits(Format::Pdf, MIN_SIZE_DEFAULT + 1));
    assert!(!o.admits(Format::Pdf, MIN_SIZE_DEFAULT), "strictly larger");
    assert!(!o.admits(Format::Epub, 1 << 20));
    o.include = false;
    assert!(o.admits(Format::Epub, 1 << 20), "excluding admits the rest");
    assert!(!o.admits(Format::Pdf, 1 << 20));
}

#[test]
fn an_empty_format_set_is_not_a_folder_that_admits_nothing() {
    let f = folder(Options {
        formats: Default::default(),
        min_size: u64::MAX,
        ..Options::default()
    });
    assert_eq!(f.opts.formats.len(), Format::ALL.len());
    assert_eq!(f.opts.min_size, MIN_SIZE_MAX);
}

#[test]
fn the_three_answers_are_one_mode() {
    let mut o = Options::default();
    for mode in Mode::ALL {
        o.set_mode(mode);
        assert_eq!(o.mode(), mode);
    }
    o.in_place = false;
    o.watch = true;
    assert_eq!(o.mode(), Mode::Copy, "a watching copy is a copy");
    assert_eq!(Mode::Copy.badge(), "Copied");
    assert_eq!(Mode::Watched.badge(), "On disk");
}

#[test]
fn turning_a_rung_off_below_a_watched_root_leaves_the_root_watching() {
    let mut f = folder(Options::default());
    f.set_tracking("", true);
    assert!(f.opts.watch && f.owes_walk());
    f.set_tracking("a", false);
    assert!(f.tracks_rung(""));
    assert!(f.tracks_rung("b/c"), "a sibling inherits the root");
    assert!(!f.tracks_rung("a/b"), "the rung and its subtree are off");
    f.set_tracking("a/b", true);
    assert!(f.tracks_rung("a/b/c") && !f.tracks_rung("a/q"));
    f.set_tracking("", false);
    assert!(
        !f.opts.watch && !f.tracks_rung("a/b"),
        "the root speaks for all"
    );
}

#[test]
fn a_lone_rung_watched_under_an_unwatched_root_owes_a_walk() {
    let mut f = folder(Options::default());
    assert!(!f.owes_walk());
    f.set_tracking("a", true);
    assert!(f.owes_walk() && !f.opts.watch);
    assert!(f.tracks_rung("a/b") && !f.tracks_rung("b"));
}

#[test]
fn a_folder_the_library_copies_is_owed_no_walk() {
    let mut f = folder(Options::default());
    f.set_tracking("", true);
    f.opts.in_place = false;
    assert!(!f.owes_walk() && !f.tracks_rung(""));
}

#[test]
fn a_blob_from_before_tracking_was_a_tree_keeps_watching() {
    let f = folder(Options {
        watch: true,
        ..Options::default()
    });
    assert!(f.tracks_rung("deep/down"));
}

#[test]
fn a_rung_tree_inherits_and_prunes_by_zone() {
    let mut r = Rungs::default();
    assert_eq!(r.at("a"), None);
    r.set("a", true);
    r.set("a/b", false);
    r.set("c", true);
    assert_eq!(r.at("a/x"), Some(true));
    assert_eq!(r.at("a/b/x"), Some(false));
    r.clear_zone("a");
    assert_eq!(r.at("a/b"), None);
    assert_eq!(r.at("c"), Some(true), "outside the zone stays");
    r.inherit("c");
    assert!(r.is_empty());
}

#[test]
fn a_rung_is_named_by_its_directory_and_the_root_by_its_folder() {
    let f = folder(Options::default());
    assert_eq!(f.rung_name(""), "books");
    assert_eq!(f.rung_name("a/Sci-fi"), "Sci-fi");
}

#[test]
fn a_backslash_never_survives_into_the_shelf_map() {
    let mut f = folder(Options::default());
    f.shelf_map.insert("a\\b".into(), ShelfId(3));
    f.sanitize();
    assert_eq!(f.shelf_map.get("a/b"), Some(&ShelfId(3)));
}

#[test]
fn a_folder_survives_a_round_trip_through_storage() {
    let mut f = folder(Options::default());
    f.set_tracking("a", true);
    f.set_shape("b", false);
    f.shelf_map.insert(String::new(), ShelfId(9));
    let json = serde_json::to_string(&f).unwrap_or_default();
    let back: Option<Folder> = serde_json::from_str(&json).ok();
    assert_eq!(back.as_ref(), Some(&f));
    let bare: Option<Folder> =
        serde_json::from_str(r#"{"id":"0000000000000001","root":"/b"}"#).ok();
    assert!(bare.is_some_and(|b| b.opts == Options::default()));
}
