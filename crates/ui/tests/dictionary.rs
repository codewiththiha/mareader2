//! Look up, the Dictionary page and Settings over fixture packs.
#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_dict::{PACKS, State, build, db_file};
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::dict::page::LABEL;
use mareader_ui::route::Route;
use mareader_ui::settings_window::Tab;

type H = Harness<'static, MareaderApp>;

/// A folder holding the Burmese and French packs, converted.
fn shelf() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dict/tests/fixtures");
    for (file, id) in [
        ("en-my.parquet", "mcfnlp-en-my"),
        ("en-fr.parquet", "wiktionary-en-fr"),
    ] {
        let pack = PACKS.iter().find(|p| p.id == id).expect("a pack on offer");
        build(&fixtures.join(file), &db_file(dir.path(), pack)).expect("a fixture converts");
    }
    dir
}

fn reader(dir: &Path) -> H {
    let mut app = MareaderApp::default();
    let path = dir.to_string_lossy().into_owned();
    for (k, v) in [
        ("workers", "0"),
        ("animations", "false"),
        ("dictionaries", path.as_str()),
        ("pdf", "sample:5"),
    ] {
        app.set_option(k, v).expect("a known option");
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn select(h: &mut H, text: &str) {
    let pane = h.state().active_pane();
    let text = text.to_owned();
    h.state_mut().queue(Command::SelectText {
        pane,
        page: 0,
        text,
    });
    h.run();
}

#[test]
fn look_up_answers_from_the_packs_in_the_default_language() {
    let dir = shelf();
    let mut h = reader(dir.path());
    select(&mut h, "quick");
    h.get_by_label("Look up").click();
    h.run();
    let pane = h.state().active_pane();
    assert!(h.state().documents().selection(pane).is_none());
    let dict = h.state().dict();
    assert_eq!(dict.card_word(), Some("quick"));
    // No default yet: the first language other than English.
    assert_eq!(dict.card_answers(), Some(vec!["မြန်သော".to_owned()]));
    h.get_by_label("English → Burmese");
    // The card points at its words from beside them, never over them.
    let (card, at) = h.state().dict().card_place().expect("a drawn card");
    assert!(!card.intersects(at), "{card:?} covers {at:?}");
    assert!(card.top() >= at.bottom() || card.bottom() <= at.top());
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().dict().card_word(), None);

    let mut s = h.state().settings().clone();
    s.dictionary.default_lang = Some("fr".into());
    h.state_mut().queue(Command::SetSettings(Box::new(s)));
    select(&mut h, "quick brown");
    h.get_by_label("Look up").click();
    h.run();
    let dict = h.state().dict();
    assert_eq!(dict.card_word(), Some("quick brown"));
    assert_eq!(dict.card_answers(), Some(Vec::new()));
    h.get_by_label("No entry for “quick brown”.");
}

#[test]
fn without_packs_the_card_leads_to_settings() {
    let empty = tempfile::tempdir().expect("a temporary folder");
    let mut h = reader(empty.path());
    select(&mut h, "fox");
    h.get_by_label("Look up").click();
    h.run();
    h.get_by_label("No dictionary is downloaded.");
    h.get_by_label("Get dictionaries…").click();
    h.run();
    h.get_by_label("Language packs");
    h.get_by_label("Download English → Burmese");
}

#[test]
fn the_tab_lists_packs_and_removes_one() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut().queue(Command::OpenSettings(Tab::Dictionary));
    h.run();
    h.get_by_label("Download English → Japanese");
    h.get_by_label("Remove English → French").click();
    h.run();
    let fr = PACKS
        .iter()
        .find(|p| p.id == "wiktionary-en-fr")
        .expect("French");
    assert_eq!(*h.state().dict().state(fr), State::Absent);
    h.get_by_label("Download English → French");
    h.get_by_label("Default language");
}

#[test]
fn the_panel_searches_every_pack_as_typed() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut().queue(Command::ShowDictionary);
    h.run();
    assert_eq!(h.state().route(), Route::Dictionary);
    // Focused first, as a click does, then typed into.
    h.get_by_label(LABEL).click();
    h.run();
    h.get_by_label(LABEL).type_text("ight");
    // The debounce passes, then the reply lands.
    h.run_steps(30);
    // Each answer shows in its row, and the chosen one in full.
    assert!(h.query_all_by_label_contains("lumière").count() >= 1);
    assert!(h.query_all_by_label_contains("အလင်း").count() >= 1);
}

#[test]
fn the_shortcut_opens_the_page_and_escape_steps_back() {
    let dir = shelf();
    let mut h = reader(dir.path());
    let keys = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
    h.key_press_modifiers(keys, egui::Key::D);
    h.run();
    assert_eq!(h.state().route(), Route::Dictionary);
    // Cmd+D's bookmark never rode along with the Shift.
    h.get_by_label("Back to reading");
    h.get_by_label(LABEL).type_text("quick");
    h.run_steps(30);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().dict().page_text(), "", "Escape clears first");
    assert_eq!(h.state().route(), Route::Dictionary);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().route(), Route::Reader);
}

#[test]
fn from_the_library_the_page_returns_to_the_library() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    h.get_by_label("Dictionary").click();
    h.run();
    assert_eq!(h.state().route(), Route::Dictionary);
    h.get_by_label("Back to the library").click();
    h.run();
    assert_eq!(h.state().route(), Route::Library);
    // The page's Library button goes there from the reader's side too.
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    h.state_mut().queue(Command::ShowDictionary);
    h.run();
    h.get_by_label("Library").click();
    h.run();
    assert_eq!(h.state().route(), Route::Library);
}

#[test]
fn arrows_pick_a_result_and_enter_keeps_the_search() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut()
        .set_option("dict_page", "ight")
        .expect("a known option");
    h.run_steps(30);
    h.key_press(egui::Key::ArrowDown);
    h.key_press(egui::Key::Enter);
    h.run();
    let recent = &h.state().settings().dictionary.recent;
    assert_eq!(recent, &["ight".to_owned()]);
    // Cleared, the page offers it again; a click searches it.
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_label("Recent");
    h.get_by_label("ight").click();
    h.run();
    assert_eq!(h.state().dict().page_text(), "ight");
}

#[test]
fn an_answer_can_be_looked_up_in_turn() {
    let dir = shelf();
    let mut h = reader(dir.path());
    let mut s = h.state().settings().clone();
    s.dictionary.langs = vec!["wiktionary-en-fr".into()];
    h.state_mut().queue(Command::SetSettings(Box::new(s)));
    h.state_mut()
        .set_option("dict_page", "light")
        .expect("a known option");
    h.run_steps(30);
    h.get_by_label_contains("Look up “").click();
    h.run();
    let text = h.state().dict().page_text().to_owned();
    assert!(!text.is_empty() && text != "light", "{text}");
}
