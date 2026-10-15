//! The library: opens are recorded, positions kept, books reopened.

use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_library::{CoverFit, Format, Layout, Library, SortKey};
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;

/// Canonical, as the library records it; opens take any spelling.
fn fixture(name: &str) -> PathBuf {
    long(name).canonicalize().unwrap()
}

/// The fixture by way of `crates/ui/..`.
fn long(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../epub/tests/books");
    dir.join(name)
}

fn raven() -> PathBuf {
    fixture("the-raven-gutenberg-17192.epub")
}

fn harness(app: MareaderApp) -> Harness<'static, MareaderApp> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn reader(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    harness(app)
}

fn open(h: &mut Harness<'static, MareaderApp>, key: &str, value: &str) {
    h.state_mut().set_option(key, value).ok();
    h.run();
}

fn library<'a>(h: &'a Harness<'static, MareaderApp>) -> &'a Library {
    h.state().library()
}

/// The page the focused pane shows, as the library would record it.
fn page(h: &Harness<'static, MareaderApp>) -> usize {
    h.state().last_document().map_or(usize::MAX, |d| d.page)
}

fn go_to(h: &mut Harness<'static, MareaderApp>, page: usize) {
    let pane = h.state().active_pane();
    h.state_mut().queue(Command::GoTo { pane, page });
    h.run();
}

/// What the app holds, for a failing assertion's message.
fn state(h: &Harness<'static, MareaderApp>) -> String {
    let app = h.state();
    let pane = app.active_pane();
    let open = app.documents().reading(pane).map(|r| r.name);
    let toast = app.toast().map(|t| t.message.clone());
    let books: Vec<_> = app.library().books().map(|b| b.locations.clone()).collect();
    format!(
        "open {open:?}, title {}, toast {toast:?}, books {books:?}",
        app.title()
    )
}

#[test]
fn an_opened_book_is_recorded_once_and_reopens_where_it_was_left() {
    let path = raven();
    let mut h = reader(&[("epub", path.to_str().unwrap())]);
    let id = library(&h).book_at(&path);
    let id = id.unwrap_or_else(|| panic!("{path:?} not recorded: {}", state(&h)));
    let book = library(&h).book(id).unwrap();
    assert_eq!(book.format, Format::Epub);
    assert!(!book.title.value.is_empty());
    let pages = h.state().documents().page_count(h.state().active_pane());
    assert!(pages.unwrap_or(0) > 3, "{pages:?}");
    go_to(&mut h, 2);
    assert_eq!(library(&h).position(id).map(|p| p.page), Some(2));
    // Something else, then the book again: page 2, still one book.
    open(&mut h, "pdf", "sample:5");
    assert_eq!(h.state().title(), "A Mareader Sample");
    // Any spelling of the file finds its book.
    let long = long("the-raven-gutenberg-17192.epub");
    open(&mut h, "epub", long.to_str().unwrap());
    assert_eq!(page(&h), 2);
    assert_eq!(library(&h).len(), 1, "samples aren't files, so not books");
}

#[test]
fn text_files_are_recorded_as_themselves() {
    let path = fixture("river.md");
    let h = reader(&[("epub", path.to_str().unwrap())]);
    let id = library(&h).book_at(&path);
    let id = id.unwrap_or_else(|| panic!("{path:?} not recorded: {}", state(&h)));
    assert_eq!(
        library(&h).book(id).map(|b| b.format),
        Some(Format::Markdown)
    );
}

#[test]
fn the_library_page_opens_a_book_where_it_was_left() {
    let path = raven();
    let mut h = reader(&[("epub", path.to_str().unwrap())]);
    go_to(&mut h, 3);
    open(&mut h, "pdf", "sample:5");
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    assert!(h.state().library_showing());
    assert!(h.query_by_label("1 book").is_some(), "{}", state(&h));
    let title = library(&h).recent()[0].title.value.clone();
    h.get_by_label(&title).click();
    h.run();
    assert!(!h.state().library_showing());
    assert_eq!(page(&h), 3);
    assert_eq!(h.state().last_document().map(|d| d.path), Some(path));
}

#[test]
fn escape_leaves_the_library_only_for_something_to_read() {
    let mut h = reader(&[("library", "show")]);
    assert!(h.query_by_label("Your library is empty").is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().library_showing(), "nothing open to go back to");
    open(&mut h, "pdf", "sample:5");
    assert!(!h.state().library_showing(), "an open shows the reader");
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().library_showing());
}

#[test]
fn a_book_whose_file_is_gone_says_so_instead_of_opening() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    assert!(h.query_by_label("4 books").is_some());
    let missing = library(&h).books().find(|b| b.missing()).unwrap();
    let title = missing.title.value.clone();
    h.get_by_label(&title).click();
    h.run();
    assert!(h.state().library_showing());
    let toast = h
        .state()
        .toast()
        .map(|t| t.message.clone())
        .unwrap_or_default();
    assert!(toast.contains("isn't where it was"), "{toast}");
}

#[test]
fn the_library_and_positions_come_back_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    let path = raven();
    let launch = || {
        let mut app = MareaderApp::default();
        app.set_option("workers", "0").ok();
        app.attach_library(root.clone());
        harness(app)
    };
    let mut h = launch();
    open(&mut h, "epub", path.to_str().unwrap());
    go_to(&mut h, 2);
    // Writes leave half a second apart; give them a second.
    h.run_steps(60);
    let id = library(&h).book_at(&path);
    let id = id.unwrap_or_else(|| panic!("{path:?} not recorded: {}", state(&h)));
    assert!(root.join("library.json").is_file());
    assert!(root.join(format!("state/{id}.json")).is_file());
    drop(h);

    let mut h = launch();
    assert_eq!(library(&h).position(id).map(|p| p.page), Some(2));
    open(&mut h, "epub", path.to_str().unwrap());
    assert_eq!(page(&h), 2);
}

fn top(h: &Harness<'static, MareaderApp>, label: &str) -> f32 {
    h.get_by_label(label).rect().top()
}

#[test]
fn titles_with_numbers_sort_by_their_value() {
    let options = [("books", "10"), ("layout", "list"), ("sort", "title")];
    let h = reader(&[options.as_slice(), &[("library", "show")]].concat());
    let order = [
        "A Field Guide to Ferns",
        "An Atlas of Rivers",
        "Collected Essays, Volume 2",
        "Collected Essays, Volume 10",
    ];
    let tops: Vec<f32> = order.iter().map(|t| top(&h, t)).collect();
    assert!(tops.windows(2).all(|w| w[0] < w[1]), "{tops:?}");
}

#[test]
fn the_view_menu_sets_layout_columns_covers_and_order() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let view = |h: &Harness<'static, MareaderApp>| h.state().settings().library;
    assert_eq!(view(&h).layout, Layout::Grid);
    h.get_by_label("View options").click();
    h.run();
    h.get_by_label("List").click();
    h.run();
    assert_eq!(view(&h).layout, Layout::List);
    h.get_by_label("Grid").click();
    h.run();
    // 900 wide: five automatic columns, so one more pins six.
    h.get_by_label("More columns").click();
    h.run();
    assert_eq!(view(&h).columns, Some(6));
    h.get_by_label("Auto").click();
    h.run();
    assert_eq!(view(&h).columns, None);
    h.get_by_label("Crop").click();
    h.run();
    assert_eq!(view(&h).cover, CoverFit::Crop);
    h.get_by_label("Title").click();
    h.run();
    assert_eq!((view(&h).sort, view(&h).ascending), (SortKey::Title, true));
    h.get_by_label("Z–A").click();
    h.run();
    assert!(!view(&h).ascending);
}

#[test]
fn a_book_with_a_cover_shows_it_in_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("covered.epub");
    std::fs::write(&path, mareader_epub::sample::epub(2, "Covered")).unwrap();
    let mut h = reader(&[("epub", path.to_str().unwrap())]);
    assert_eq!(h.state().covers_shown(), 0, "none until the page shows");
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    assert!(h.query_by_label("Covered").is_some(), "{}", state(&h));
    assert_eq!(h.state().covers_shown(), 1, "{}", state(&h));
    open(&mut h, "layout", "list");
    assert_eq!(h.state().covers_shown(), 1, "the list reuses it");
}

#[test]
fn the_library_keeps_its_own_title_bar_pin() {
    let mut h = reader(&[("pdf", "sample:5")]);
    let chrome = |h: &Harness<'static, MareaderApp>| {
        let c = &h.state().settings().chrome;
        (c.titlebar_pinned, c.library_titlebar_pinned)
    };
    assert!(!h.state().titlebar().pinned);
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    assert!(h.state().titlebar().pinned, "the library pins by default");
    h.state_mut().queue(Command::SetTitlebarPinned(false));
    h.run();
    assert_eq!(chrome(&h), (false, false));
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    assert!(!h.state().library_showing());
    h.state_mut().queue(Command::SetTitlebarPinned(true));
    h.run();
    assert_eq!(chrome(&h), (true, false), "the reader's pin alone");
    assert!(h.state().titlebar().pinned);
}

#[test]
fn the_library_shows_no_rail_and_only_its_own_bar_tools() {
    let mut h = reader(&[("pdf", "sample:5"), ("sidebar", "outline")]);
    assert!(h.query_by_label("Close sidebar").is_some());
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    // The library's bar starts pinned, so all of it is on show.
    let reader_only = [
        "Close sidebar",
        "Toggle sidebar",
        "Settings",
        "Search",
        "Split down",
        "Split right",
        "View & tools",
    ];
    for gone in reader_only {
        assert!(h.query_by_label(gone).is_none(), "{gone} in the library");
    }
    for kept in [
        "Back to reading",
        "View options",
        "Appearance",
        "Pin title bar",
    ] {
        assert!(h.query_by_label(kept).is_some(), "{kept} missing");
    }
    assert!(h.state().sidebar().is_open(), "kept for the way back");
    h.get_by_label("Back to reading").click();
    h.run();
    assert!(!h.state().library_showing());
    assert!(h.query_by_label("Close sidebar").is_some());
}

#[test]
fn an_empty_library_offers_no_way_back() {
    let h = reader(&[("library", "show")]);
    assert!(h.query_by_label("Back to reading").is_none());
    assert!(h.query_by_label("View options").is_some());
}

#[test]
fn reader_commands_and_keys_do_nothing_in_the_library() {
    let mut h = reader(&[("pdf", "sample:5")]);
    h.state_mut().queue(Command::ShowFind);
    h.run();
    assert!(h.state().find().is_open());
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    assert!(
        !h.state().find().is_open(),
        "the find bar ends with the panes"
    );
    for c in [
        Command::Split(mareader_workspace::SplitAxis::Horizontal),
        Command::ShowSettings,
        Command::ToggleSidebar,
        Command::LastPage,
    ] {
        h.state_mut().queue(c);
    }
    h.key_press(egui::Key::End);
    h.run();
    assert!(h.state().library_showing());
    assert_eq!(h.state().panes().len(), 1);
    assert!(!h.state().settings_window().is_open());
    assert!(!h.state().sidebar().is_open());
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    let app = h.state();
    let page = app.documents().current(app.active_pane());
    assert_eq!(page, Some(0), "no page turned behind the library");
}

#[test]
fn the_library_appearance_menu_has_no_page_texture() {
    let mut h = reader(&[("pdf", "sample:5"), ("pinned", "true")]);
    h.get_by_label("Appearance").click();
    h.run();
    assert!(h.query_by_label("Page texture").is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    h.get_by_label("Appearance").click();
    h.run();
    assert!(h.query_by_label("Mode & colour").is_some());
    assert!(
        h.query_by_label("Page texture").is_none(),
        "the shelf has no page"
    );
}

#[test]
fn the_view_menu_key_opens_the_librarys_own_menu() {
    let mut h = reader(&[("library", "show"), ("books", "3")]);
    h.state_mut().queue(Command::ShowViewMenu);
    h.run();
    assert!(h.query_by_label("Sort by").is_some());
    assert!(
        h.query_by_label("Full screen").is_none(),
        "not the reader's"
    );
}

#[test]
fn library_show_after_an_open_lands_in_the_library() {
    let h = reader(&[("pdf", "sample:5"), ("library", "show")]);
    assert!(h.state().library_showing());
    assert!(h.query_by_label("Back to reading").is_some());
}
