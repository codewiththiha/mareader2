//! Look up, the Dictionary panel and Settings over fixture packs.
#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_dict::{PACKS, State, build, db_file};
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::settings_window::Tab;
use mareader_ui::sidebar::Panel;

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
    h.state_mut()
        .queue(Command::OpenSidebar(Some(Panel::Dictionary)));
    h.run();
    // Focused first, as a click does, then typed into.
    h.get_by_label(mareader_ui::dict::panel::LABEL).click();
    h.run();
    h.get_by_label(mareader_ui::dict::panel::LABEL)
        .type_text("ight");
    // The debounce passes, then the reply lands.
    h.run_steps(30);
    h.get_by_label("lumière");
    h.get_by_label("အလင်း");
}
