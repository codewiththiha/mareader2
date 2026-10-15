//! The library: opens are recorded, positions kept, books reopened.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_library::{BookId, CoverFit, Format, Home, Layout, Library, Origin, ShelfId, SortKey};
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::library::{Action, BookAction, Pick, PickAction, ShelfAction};

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
fn a_missing_book_asks_where_it_went_instead_of_opening() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    assert!(h.query_by_label("4 books").is_some());
    let missing = library(&h).books().find(|b| b.missing()).unwrap();
    let title = missing.title.value.clone();
    h.get_by_label(&title).click();
    h.run();
    assert!(h.state().library_showing());
    assert!(h.query_by_label(&format!("Find “{title}”")).is_some());
    assert!(h.query_by_label("Choose the file…").is_some());
    assert!(h.query_by_label("Search a folder…").is_some());
    // Escape closes the sheet, not the library; nothing changed.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("Choose the file…").is_none());
    assert!(h.state().library_showing());
    assert!(library(&h).books().any(|b| b.missing()));
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

const NOTES: &[u8] = b"---\ntitle: Field Notes\nauthor: A. Walker\n---\n\n# One\n\nText.\n";

fn raven_bytes() -> Vec<u8> {
    std::fs::read(raven()).unwrap()
}

/// Writes a file an hour old, so no scan waits for it to settle.
fn put(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    let old = SystemTime::now() - Duration::from_secs(3_600);
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(old).unwrap();
    path.canonicalize().unwrap()
}

fn act(h: &mut Harness<'static, MareaderApp>, action: Action) {
    h.state_mut().queue(Command::Library(action));
    h.run();
    // Inline disk work answers on the next frame.
    h.run();
}

fn toast(h: &Harness<'static, MareaderApp>) -> String {
    let toast = h.state().toast().map(|t| t.message.clone());
    toast.unwrap_or_default()
}

fn moved(from: &Path, to: &Path) -> PathBuf {
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::rename(from, to).unwrap();
    to.canonicalize().unwrap()
}

#[test]
fn a_folder_brings_in_its_books_where_they_are_and_says_how_many() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Books");
    put(&root, "raven.epub", &raven_bytes());
    put(&root, "notes/notes.md", NOTES);
    put(&root, "letter.docx", b"not a book");
    put(&root, ".trash/old.md", b"# Old");
    let mut h = reader(&[("library", "show")]);
    act(&mut h, Action::AddFolder(root.clone()));
    assert_eq!(toast(&h), "Added 2 books from “Books”.", "{}", state(&h));
    let titles: Vec<String> = library(&h).books().map(|b| b.title.value.clone()).collect();
    assert!(titles.contains(&"Field Notes".to_owned()), "{titles:?}");
    assert!(titles.iter().any(|t| t.contains("Raven")), "{titles:?}");
    // Found, not opened; nothing was copied or moved.
    assert!(library(&h).books().all(|b| b.opened.value == 0));
    assert!(root.join("raven.epub").is_file());
    assert!(h.query_by_label("Books · 2").is_some(), "the folder's chip");
    act(&mut h, Action::AddFolder(root.join("notes")));
    assert!(toast(&h).contains("already read as part of “Books”"));
}

#[test]
fn coming_back_to_the_window_finds_new_files_and_notices_gone_ones() {
    let dir = tempfile::tempdir().unwrap();
    let raven = put(dir.path(), "raven.epub", &raven_bytes());
    let mut h = reader(&[("library", "show")]);
    act(&mut h, Action::AddFolder(dir.path().into()));
    let first = library(&h).book_at(&raven).unwrap();
    std::fs::remove_file(&raven).unwrap();
    let notes = put(dir.path(), "notes.md", NOTES);
    // Rescans on focus wait out a short gap after the last.
    h.run_steps(150);
    h.input_mut().focused = false;
    h.run();
    h.input_mut().focused = true;
    h.run();
    h.run();
    assert!(library(&h).book_at(&notes).is_some(), "{}", state(&h));
    assert!(library(&h).book(first).unwrap().missing());
}

#[test]
fn a_file_moved_within_a_folder_keeps_its_book_and_place() {
    let dir = tempfile::tempdir().unwrap();
    let raven = put(dir.path(), "new/raven.epub", &raven_bytes());
    let mut h = reader(&[("library", "show")]);
    act(&mut h, Action::AddFolder(dir.path().into()));
    let id = library(&h).book_at(&raven).unwrap();
    h.state_mut().queue(Command::OpenBook(id));
    h.run();
    go_to(&mut h, 2);
    let now = moved(&raven, &dir.path().join("read/The Raven.epub"));
    act(&mut h, Action::Rescan(None));
    assert_eq!(library(&h).book_at(&now), Some(id), "{}", state(&h));
    assert_eq!(library(&h).len(), 1);
    assert_eq!(library(&h).position(id).map(|p| p.page), Some(2));
}

#[test]
fn a_lost_book_is_found_again_by_choosing_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let raven = put(&dir.path().join("in"), "raven.epub", &raven_bytes());
    let mut h = reader(&[("library", "show")]);
    act(&mut h, Action::AddFolder(dir.path().join("in")));
    let id = library(&h).book_at(&raven).unwrap();
    let elsewhere = moved(&raven, &dir.path().join("out/renamed.epub"));
    act(&mut h, Action::Rescan(None));
    assert!(library(&h).book(id).unwrap().missing());
    act(&mut h, Action::FindAgain(id));
    let path = elsewhere.clone();
    act(&mut h, Action::Relink { book: id, path });
    assert!(!library(&h).book(id).unwrap().missing(), "{}", state(&h));
    assert_eq!(library(&h).book_at(&elsewhere), Some(id));
    assert!(!h.state().library_showing(), "a found book opens");
}

#[test]
fn searching_a_folder_finds_a_lost_book_under_any_name() {
    let dir = tempfile::tempdir().unwrap();
    let raven = put(&dir.path().join("in"), "raven.epub", &raven_bytes());
    let mut h = reader(&[("library", "show")]);
    act(&mut h, Action::AddFolder(dir.path().join("in")));
    let id = library(&h).book_at(&raven).unwrap();
    let deep = moved(&raven, &dir.path().join("attic/old/poems.epub"));
    act(&mut h, Action::Rescan(None));
    act(&mut h, Action::FindAgain(id));
    let folder = dir.path().join("attic");
    act(&mut h, Action::Seek { book: id, folder });
    assert_eq!(library(&h).book_at(&deep), Some(id), "{}", state(&h));
}

#[test]
fn a_namesake_with_other_content_is_offered_not_taken() {
    let dir = tempfile::tempdir().unwrap();
    let notes = put(&dir.path().join("in"), "notes.md", NOTES);
    let mut h = reader(&[("library", "show")]);
    act(&mut h, Action::AddFolder(dir.path().join("in")));
    let id = library(&h).book_at(&notes).unwrap();
    let before = library(&h).book(id).unwrap().content;
    std::fs::remove_file(&notes).unwrap();
    let edited = put(
        &dir.path().join("backup"),
        "notes.md",
        b"# One\n\nText, revised.\n",
    );
    act(&mut h, Action::Rescan(None));
    act(&mut h, Action::FindAgain(id));
    let folder = dir.path().join("backup");
    act(&mut h, Action::Seek { book: id, folder });
    assert!(library(&h).book(id).unwrap().missing(), "asked first");
    h.get_by_label("Use this file").click();
    h.run();
    h.run();
    let book = library(&h).book(id).unwrap();
    assert!(!book.missing(), "{}", state(&h));
    assert_ne!(book.content, before);
    assert_eq!(library(&h).book_at(&edited), Some(id));
}

#[test]
fn removing_a_folder_asks_first_and_adding_it_again_restores_its_books() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Shelf");
    let raven = put(&root, "raven.epub", &raven_bytes());
    put(&root, "notes.md", NOTES);
    let mut h = reader(&[("library", "show")]);
    act(&mut h, Action::AddFolder(root.clone()));
    let id = library(&h).book_at(&raven).unwrap();
    let source = library(&h).sources().next().unwrap().id;
    act(&mut h, Action::AskRemoveFolder(source));
    assert_eq!(library(&h).len(), 2, "nothing goes before the answer");
    h.get_by_label("Remove").click();
    h.run();
    assert!(library(&h).is_empty(), "{}", state(&h));
    assert_eq!(toast(&h), "Removed “Shelf” and its 2 books.");
    assert!(raven.is_file(), "files stay where they are");
    act(&mut h, Action::AddFolder(root));
    assert_eq!(library(&h).len(), 2);
    assert_eq!(library(&h).book_at(&raven), Some(id), "the same book, back");
}

#[test]
fn adding_a_folder_leaves_other_missing_books_missing() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "notes.md", NOTES);
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let lost = library(&h).books().filter(|b| b.missing()).count();
    assert_eq!(lost, 1);
    act(&mut h, Action::AddFolder(dir.path().into()));
    assert_eq!(library(&h).len(), 5, "{}", state(&h));
    assert_eq!(library(&h).books().filter(|b| b.missing()).count(), 1);
}

#[test]
fn an_empty_library_offers_to_add_a_folder() {
    let h = reader(&[("library", "show")]);
    assert!(h.query_by_label("Add folder…").is_some());
}

fn open_shelf(h: &Harness<'static, MareaderApp>) -> Option<ShelfId> {
    h.state().library_state().at()
}

fn shelve(h: &mut Harness<'static, MareaderApp>, action: ShelfAction) {
    act(h, Action::Shelf(action));
}

/// A shelf made through the page's own action, at the top level.
fn new_shelf(h: &mut Harness<'static, MareaderApp>, name: &str) -> ShelfId {
    shelve(h, ShelfAction::New);
    let id = open_shelf(h).expect("a new shelf opens");
    shelve(h, ShelfAction::Rename(id, name.into()));
    shelve(h, ShelfAction::GoTo(None));
    id
}

fn title_of(h: &Harness<'static, MareaderApp>, n: usize) -> String {
    let mut books: Vec<_> = library(h).books().collect();
    books.sort_by_key(|b| b.id);
    books[n].title.value.clone()
}

fn book_id(h: &Harness<'static, MareaderApp>, n: usize) -> BookId {
    let mut ids: Vec<BookId> = library(h).books().map(|b| b.id).collect();
    ids.sort();
    ids[n]
}

#[test]
fn a_new_shelf_opens_and_is_named_in_the_breadcrumb() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    h.get_by_label("New shelf").click();
    h.run();
    let id = open_shelf(&h).expect("the new shelf opens");
    assert_eq!(h.state().library_state().renaming(), Some(id));
    // Its whole name is selected, so typing replaces it.
    let field = h.get_by_role(Role::TextInput);
    assert!(field.is_focused());
    field.type_text("Poetry");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let shelf = library(&h).shelf(id).unwrap();
    assert_eq!(shelf.name.value, "Poetry");
    assert_eq!(h.state().library_state().renaming(), None);
    assert!(h.state().library_showing());
    h.get_by_role_and_label(Role::Button, "Library").click();
    h.run();
    assert_eq!(open_shelf(&h), None);
    h.get_by_label("Poetry").click();
    h.run();
    assert_eq!(open_shelf(&h), Some(id));
}

#[test]
fn escape_while_naming_keeps_the_old_name_and_the_page() {
    let mut h = reader(&[("books", "2"), ("library", "show")]);
    h.get_by_label("New shelf").click();
    h.run();
    let id = open_shelf(&h).unwrap();
    h.get_by_role(Role::TextInput).type_text("Junk");
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(library(&h).shelf(id).unwrap().name.value, "New shelf");
    assert_eq!(h.state().library_state().renaming(), None);
    assert!(h.state().library_showing(), "Escape only ended the naming");
    // The crumb's menu names it again; the header's button shares its name.
    let crumbs = h.get_all_by_role_and_label(Role::Button, "New shelf");
    let crumb = crumbs.min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    crumb.unwrap().click();
    h.run();
    h.get_by_label("Rename…").click();
    h.run();
    assert_eq!(h.state().library_state().renaming(), Some(id));
}

#[test]
fn a_name_already_beside_it_is_refused() {
    let mut h = reader(&[("books", "2"), ("library", "show")]);
    new_shelf(&mut h, "Poetry");
    let other = new_shelf(&mut h, "Other");
    shelve(&mut h, ShelfAction::Rename(other, "poetry".into()));
    assert_eq!(toast(&h), "There's already a shelf called “poetry” there.");
    assert_eq!(library(&h).shelf(other).unwrap().name.value, "Other");
}

/// Presses on `from`, moves in steps past egui's drag threshold, and stops.
fn carry(h: &mut Harness<'static, MareaderApp>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run();
    h.drag_at(from);
    h.run();
    for step in 1..=6 {
        h.hover_at(from.lerp(to, step as f32 / 6.0));
        h.run();
    }
}

/// Lets go at `at`, the pointer staying there.
fn release(h: &mut Harness<'static, MareaderApp>, at: egui::Pos2) {
    h.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

#[test]
fn dragging_a_book_onto_a_shelf_files_it_and_says_so() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let shelf = new_shelf(&mut h, "Poetry");
    let title = title_of(&h, 1);
    let book = h.get_by_label(&title).rect().center();
    let target = h.get_by_label("Poetry").rect().center();
    carry(&mut h, book, target);
    release(&mut h, target);
    assert_eq!(
        library(&h).shelves_of(book_id(&h, 1)).len(),
        1,
        "{}",
        toast(&h)
    );
    assert_eq!(toast(&h), format!("Added “{title}” to “Poetry”."));
    assert_eq!(open_shelf(&h), None, "a drop doesn't open the shelf");
    // Again: it's already there.
    carry(&mut h, book, target);
    release(&mut h, target);
    assert_eq!(toast(&h), format!("“{title}” is already on “Poetry”."));
    assert_eq!(library(&h).shelf_books(shelf).len(), 1);
}

#[test]
fn escape_drops_what_is_carried_and_keeps_the_page() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    new_shelf(&mut h, "Poetry");
    let title = title_of(&h, 2);
    let book = h.get_by_label(&title).rect().center();
    let target = h.get_by_label("Poetry").rect().center();
    carry(&mut h, book, target);
    h.key_press(egui::Key::Escape);
    h.run();
    release(&mut h, target);
    assert!(library(&h).shelved().is_empty(), "{}", toast(&h));
    assert!(
        h.state().library_showing(),
        "Escape ended the drag, not the page"
    );
}

#[test]
fn a_shelf_moves_books_between_shelves_or_adds_them_with_alt() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let a = new_shelf(&mut h, "A");
    let b = new_shelf(&mut h, "B");
    let book = book_id(&h, 0);
    let file = |to, from, keep| ShelfAction::File {
        book,
        to,
        from,
        keep,
    };
    shelve(&mut h, file(Some(a), None, false));
    shelve(&mut h, file(Some(b), Some(a), false));
    assert!(toast(&h).starts_with("Moved"), "{}", toast(&h));
    assert_eq!(
        library(&h).shelves_of(book).into_iter().collect::<Vec<_>>(),
        [b]
    );
    shelve(&mut h, file(Some(a), Some(b), true));
    assert_eq!(library(&h).shelves_of(book).len(), 2, "Alt keeps it on B");
    // Dropped on the top level's crumb, it leaves the open shelf.
    shelve(&mut h, file(None, Some(a), false));
    assert_eq!(
        library(&h).shelves_of(book).into_iter().collect::<Vec<_>>(),
        [b]
    );
    // A shelf can't go inside its own shelves.
    shelve(
        &mut h,
        ShelfAction::Move {
            shelf: b,
            to: Some(b),
        },
    );
    let inner = new_shelf(&mut h, "Inner");
    shelve(
        &mut h,
        ShelfAction::Move {
            shelf: inner,
            to: Some(b),
        },
    );
    shelve(
        &mut h,
        ShelfAction::Move {
            shelf: b,
            to: Some(inner),
        },
    );
    assert_eq!(toast(&h), "A shelf can't go inside itself.");
}

#[test]
fn home_can_leave_shelved_books_to_their_shelves() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let shelf = new_shelf(&mut h, "Poetry");
    let shelved = title_of(&h, 0);
    let file = ShelfAction::File {
        book: book_id(&h, 0),
        to: Some(shelf),
        from: None,
        keep: false,
    };
    shelve(&mut h, file);
    assert!(
        h.query_by_label(&shelved).is_some(),
        "everything, by default"
    );
    h.get_by_label("View options").click();
    h.run();
    h.get_by_label("Unshelved").click();
    h.run();
    assert_eq!(h.state().settings().library.home, Home::Unshelved);
    assert!(h.query_by_label(&shelved).is_none());
    assert!(h.query_by_label(&title_of(&h, 1)).is_some());
}

#[test]
fn removing_a_shelf_asks_then_moves_its_shelves_up() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let outer = new_shelf(&mut h, "Outer");
    shelve(&mut h, ShelfAction::GoTo(Some(outer)));
    shelve(&mut h, ShelfAction::New);
    let inner = open_shelf(&h).unwrap();
    let file = ShelfAction::File {
        book: book_id(&h, 0),
        to: Some(outer),
        from: None,
        keep: false,
    };
    shelve(&mut h, file);
    shelve(&mut h, ShelfAction::GoTo(Some(outer)));
    shelve(&mut h, ShelfAction::AskRemove(outer));
    assert!(
        library(&h).shelf(outer).is_some(),
        "nothing goes before the answer"
    );
    h.get_by_label("Remove").click();
    h.run();
    assert!(library(&h).shelf(outer).is_none());
    assert_eq!(open_shelf(&h), None, "back up to where it was");
    let top: Vec<ShelfId> = library(&h).children(None).iter().map(|s| s.id).collect();
    assert_eq!(top, [inner]);
    assert_eq!(library(&h).len(), 4, "its books stay");
    assert_eq!(toast(&h), "Removed “Outer”. Its books stay in the library.");
}

#[test]
fn a_long_trail_folds_its_middle_and_leads_back_up() {
    let mut h = reader(&[("books", "1"), ("library", "show")]);
    for n in 1..=5 {
        shelve(&mut h, ShelfAction::New);
        let id = open_shelf(&h).unwrap();
        shelve(&mut h, ShelfAction::Rename(id, format!("Level {n}")));
    }
    let trail = h.state().library_state().trail();
    assert_eq!(trail.len(), 6);
    assert!(h.query_by_label("…").is_some());
    // The top, then the last two above the open shelf: four crumbs.
    let kept = &trail[trail.len() - 3].name;
    assert!(h.query_by_role_and_label(Role::Button, kept).is_some());
    h.get_by_label("…").click();
    h.run();
    let second = trail[1].id;
    h.get_by_label(&trail[1].name).click();
    h.run();
    assert_eq!(open_shelf(&h), second);
    h.get_by_role_and_label(Role::Button, "Library").click();
    h.run();
    assert_eq!(open_shelf(&h), None);
}

#[test]
fn a_removed_open_shelf_gives_way_to_its_parent() {
    let mut h = reader(&[("books", "2"), ("library", "show")]);
    let outer = new_shelf(&mut h, "Outer");
    shelve(&mut h, ShelfAction::GoTo(Some(outer)));
    shelve(&mut h, ShelfAction::New);
    let inner = open_shelf(&h).unwrap();
    shelve(&mut h, ShelfAction::Remove(inner));
    assert_eq!(open_shelf(&h), Some(outer));
    assert_eq!(h.state().library_state().renaming(), None);
}

fn picks(h: &Harness<'static, MareaderApp>) -> Vec<BookId> {
    h.state().library_state().picks().books()
}

fn book(h: &mut Harness<'static, MareaderApp>, action: BookAction) {
    act(h, Action::Book(action));
}

#[test]
fn ctrl_click_starts_a_selection_that_plain_clicks_then_extend() {
    let mut h = reader(&[("books", "6"), ("library", "show")]);
    let (one, two) = (title_of(&h, 1), title_of(&h, 2));
    h.get_by_label(&one)
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    assert_eq!(picks(&h), [book_id(&h, 1)]);
    assert!(
        h.query_by_label("1 book selected").is_some(),
        "the bar shows"
    );
    // While picking, a plain click picks too instead of opening.
    h.get_by_label(&two).click();
    h.run();
    assert_eq!(picks(&h).len(), 2);
    assert!(h.state().library_showing(), "nothing opened");
    h.get_by_label(&two).click();
    h.run();
    assert_eq!(picks(&h), [book_id(&h, 1)]);
    // Escape lets go of the selection before it leaves the page.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(picks(&h).is_empty());
    assert!(h.state().library_showing());
}

#[test]
fn shift_click_picks_the_run_between_and_ctrl_a_picks_all() {
    let mut h = reader(&[("books", "6"), ("library", "show"), ("layout", "list")]);
    // In the order shown: top to bottom.
    let mut titles: Vec<String> = library(&h).books().map(|b| b.title.value.clone()).collect();
    titles.sort_by(|a, b| top(&h, a).total_cmp(&top(&h, b)));
    h.get_by_label(&titles[1])
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    h.get_by_label(&titles[4])
        .click_modifiers(egui::Modifiers::SHIFT);
    h.run();
    assert_eq!(picks(&h).len(), 4, "{titles:?}");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    assert_eq!(picks(&h).len(), 6);
    h.get_by_label("Clear selection").click();
    h.run();
    assert!(picks(&h).is_empty());
}

#[test]
fn picks_go_on_a_new_shelf_together_and_come_off_it() {
    let mut h = reader(&[("books", "5"), ("library", "show")]);
    let (a, b) = (book_id(&h, 1), book_id(&h, 2));
    let both = vec![Pick::Book(a), Pick::Book(b)];
    act(&mut h, Action::Pick(PickAction::Only(both)));
    h.get_by_label("New shelf from these").click();
    h.run();
    let shelf = open_shelf(&h).expect("the new shelf opens");
    assert_eq!(
        h.state().library_state().renaming(),
        Some(shelf),
        "named next"
    );
    assert_eq!(library(&h).shelf_books(shelf).len(), 2);
    assert!(picks(&h).is_empty());
    act(&mut h, Action::Pick(PickAction::Only(vec![Pick::Book(a)])));
    book(&mut h, BookAction::TakeOff(vec![a]));
    assert_eq!(library(&h).shelf_books(shelf).len(), 1);
    assert!(toast(&h).starts_with("Took “"), "{}", toast(&h));
}

#[test]
fn a_dragged_selection_moves_onto_a_shelf_together() {
    let mut h = reader(&[("books", "5"), ("library", "show")]);
    let shelf = new_shelf(&mut h, "Poetry");
    let (one, two) = (title_of(&h, 1), title_of(&h, 2));
    h.get_by_label(&one)
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    h.get_by_label(&two)
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    let from = h.get_by_label(&one).rect().center();
    let target = h.get_by_label("Poetry").rect().center();
    carry(&mut h, from, target);
    release(&mut h, target);
    assert_eq!(library(&h).shelf_books(shelf).len(), 2, "{}", toast(&h));
    assert_eq!(toast(&h), "Added 2 books to “Poetry”.");
    assert!(picks(&h).is_empty(), "a drop lets go");
}

#[test]
fn a_card_menu_offers_what_a_book_can_do() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let title = title_of(&h, 1);
    h.get_by_label(&title).click_secondary();
    h.run();
    for entry in ["Open", "Select", "Rename…", "Read separately", "Remove…"] {
        assert!(h.query_by_label(entry).is_some(), "{entry}");
    }
    h.get_by_label("Rename…").click();
    h.run();
    // The old name starts focused and selected: typing replaces it.
    assert!(h.get_by_role(Role::TextInput).is_focused());
    h.get_by_role(Role::TextInput).type_text("Moby Dick");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let renamed = library(&h).book(book_id(&h, 1)).unwrap();
    assert_eq!(renamed.title.value, "Moby Dick");
    assert!(h.state().library_state().sheet().is_none());
}

#[test]
fn removing_a_folder_book_hides_it_until_restored_from_folders() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Shelf");
    let raven = put(&root, "raven.epub", &raven_bytes());
    put(&root, "notes.md", NOTES);
    let mut h = reader(&[("library", "show")]);
    act(&mut h, Action::AddFolder(root.clone()));
    let id = library(&h).book_at(&raven).unwrap();
    let (books, shelves) = (vec![id], Vec::new());
    book(&mut h, BookAction::AskRemove { books, shelves });
    let says = "Its file stays in its folder; restore it from Folders.";
    assert!(
        h.query_by_label(says).is_some(),
        "the receipt says what stays"
    );
    h.get_by_label("Remove").click();
    h.run();
    assert!(library(&h).book(id).is_none());
    assert!(raven.is_file(), "the file is never touched");
    assert!(
        toast(&h).ends_with("Restore from Folders."),
        "{}",
        toast(&h)
    );
    act(&mut h, Action::Rescan(None));
    assert!(library(&h).book(id).is_none(), "a rescan leaves it out");
    act(&mut h, Action::ShowFolders);
    h.get_by_label("Removed · 1").click();
    h.run();
    h.get_by_label("Restore").click();
    h.run();
    assert!(library(&h).book(id).is_some_and(|b| !b.missing()));
    assert_eq!(toast(&h), "Restored “The Raven”.");
}

#[test]
fn read_separately_keeps_a_second_place_in_the_same_file() {
    let path = raven();
    let mut h = reader(&[("epub", path.to_str().unwrap())]);
    go_to(&mut h, 2);
    h.run_steps(3);
    let id = library(&h).book_at(&path).unwrap();
    book(&mut h, BookAction::Twin(id));
    let twin = library(&h).books().find(|b| b.twin_of == Some(id));
    let twin = twin
        .map(|b| b.id)
        .unwrap_or_else(|| panic!("{}", state(&h)));
    assert_eq!(library(&h).book(twin).unwrap().title.value, "The Raven (2)");
    h.state_mut().queue(Command::OpenBook(twin));
    h.run();
    h.run_steps(3);
    assert_eq!(page(&h), 0, "a twin starts at its own place");
    go_to(&mut h, 1);
    h.run_steps(3);
    assert_eq!(library(&h).position(twin).map(|p| p.page), Some(1));
    assert_eq!(library(&h).position(id).map(|p| p.page), Some(2));
    h.state_mut().queue(Command::OpenBook(id));
    h.run();
    h.run_steps(3);
    assert_eq!(page(&h), 2, "the original kept its place");
}

#[test]
fn imports_are_copies_the_library_keeps_and_deletes_with_the_book() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    let outside = put(dir.path(), "Raven.epub", &raven_bytes());
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("library", "show").ok();
    app.attach_library(root.clone());
    let mut h = harness(app);
    let shelf = new_shelf(&mut h, "Poems");
    let to = Some(shelf);
    book(
        &mut h,
        BookAction::Import {
            paths: vec![outside.clone()],
            to,
        },
    );
    assert_eq!(toast(&h), "Imported “The Raven” to “Poems”.");
    let b = library(&h).books().next().unwrap().clone();
    let copy = b.path().unwrap().to_path_buf();
    assert_eq!(b.locations[0].origin, Origin::Store);
    assert!(copy.starts_with(root.join("files").canonicalize().unwrap()));
    assert_eq!(library(&h).shelf_books(shelf).len(), 1);
    // The same content again is the same book; nothing is copied.
    let notes = put(dir.path(), "notes.docx", b"not a book");
    book(
        &mut h,
        BookAction::Import {
            paths: vec![outside.clone(), notes],
            to: None,
        },
    );
    let said = "“The Raven” was already in your library. “notes.docx” isn't a book Mareader reads.";
    assert_eq!(toast(&h), said);
    assert_eq!(library(&h).len(), 1);
    let (books, shelves) = (vec![b.id], Vec::new());
    book(&mut h, BookAction::AskRemove { books, shelves });
    assert!(
        h.query_by_label("The copy Mareader keeps is deleted.")
            .is_some()
    );
    h.get_by_label("Remove").click();
    h.run();
    h.run();
    assert!(!copy.exists(), "the library's copy goes");
    assert!(outside.is_file(), "the file imported from stays");
}
