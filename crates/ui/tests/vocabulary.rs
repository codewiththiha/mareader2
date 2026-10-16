//! Shading words above the reader's level, and the Vocabulary tab.
#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_cefr::{Part, State};
use mareader_dict::{PACKS, build as build_pack, db_file};
use mareader_settings::CefrLevel;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::settings_window::Tab;
use parquet::basic::{Compression, ZstdLevel};
use parquet::data_type::{ByteArray, ByteArrayType, FloatType};
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::parser::parse_message_type;

type H = Harness<'static, MareaderApp>;

/// "quick" is C2; the rest of its line is easy.
const ROWS: &[(&str, &str, f32)] = &[
    ("the", "DT", 1.0),
    ("quick", "JJ", 6.0),
    ("brown", "JJ", 1.0),
    ("fox", "NN", 2.0),
];

/// A parquet shaped like the real dataset.
fn dataset(path: &Path) {
    let message = "message schema {
        REQUIRED BYTE_ARRAY word (UTF8);
        OPTIONAL BYTE_ARRAY pos_tag (UTF8);
        REQUIRED FLOAT level;
        OPTIONAL BYTE_ARRAY lemma (UTF8);
    }";
    let schema = Arc::new(parse_message_type(message).unwrap());
    let zstd = Compression::ZSTD(ZstdLevel::default());
    let props = Arc::new(WriterProperties::builder().set_compression(zstd).build());
    let mut writer = SerializedFileWriter::new(File::create(path).unwrap(), schema, props).unwrap();
    let mut group = writer.next_row_group().unwrap();
    let words: Vec<ByteArray> = ROWS.iter().map(|r| r.0.into()).collect();
    let tags: Vec<ByteArray> = ROWS.iter().map(|r| r.1.into()).collect();
    let tagged = vec![1i16; ROWS.len()];
    let levels: Vec<f32> = ROWS.iter().map(|r| r.2).collect();
    let none = vec![0i16; ROWS.len()];
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<ByteArrayType>();
    typed.write_batch(&words, None, None).unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<ByteArrayType>();
    typed.write_batch(&tags, Some(&tagged), None).unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<FloatType>();
    typed.write_batch(&levels, None, None).unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<ByteArrayType>();
    typed.write_batch(&[], Some(&none), None).unwrap();
    col.close().unwrap();
    group.close().unwrap();
    writer.close().unwrap();
}

/// A folder with the word table converted, and one with the Burmese pack.
fn shelves() -> (tempfile::TempDir, tempfile::TempDir) {
    let words = tempfile::tempdir().unwrap();
    let parquet = words.path().join("source.parquet");
    dataset(&parquet);
    mareader_cefr::build(&parquet, &words.path().join("cefr.db")).unwrap();
    std::fs::remove_file(parquet).unwrap();
    let packs = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dict/tests/fixtures");
    let pack = PACKS.iter().find(|p| p.id == "mcfnlp-en-my").unwrap();
    build_pack(
        &fixtures.join("en-my.parquet"),
        &db_file(packs.path(), pack),
    )
    .unwrap();
    (words, packs)
}

fn reader(words: &Path, packs: Option<&Path>) -> H {
    let mut app = MareaderApp::default();
    let words = words.to_string_lossy().into_owned();
    let packs = packs.map(|p| p.to_string_lossy().into_owned());
    let mut options = vec![
        ("workers", "0"),
        ("explain", "canned"),
        ("animations", "false"),
        ("cefr", words.as_str()),
        ("pdf", "sample:5"),
    ];
    if let Some(packs) = &packs {
        options.push(("dictionaries", packs.as_str()));
    }
    for (k, v) in options {
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

/// The middle of the first `needle` on page 0, on screen.
fn spot(h: &H, needle: &str) -> Pos2 {
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let layer = docs.reading(pane).unwrap().document.text_layer(0).unwrap();
    let sheet = *docs.sheets(pane).iter().find(|s| s.page == 0).unwrap();
    let chars = layer.text.chars();
    let text: String = chars.iter().map(|c| c.ch).collect();
    let at = text.find(needle).unwrap();
    let i = text.get(..at).unwrap().chars().count() + needle.chars().count() / 2;
    sheet.to_screen(chars.get(i).unwrap().bounds).center()
}

/// A click on the middle of the first `needle` on page 0.
fn click(h: &mut H, needle: &str) {
    let pos = spot(h, needle);
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run();
}

#[test]
fn a_click_on_a_shaded_word_explains_it_and_marks_it() {
    let (words, _) = shelves();
    let mut h = reader(words.path(), None);
    assert_eq!(h.state().vocab().state(Part::Dataset), &State::Ready);
    // An easy word is not shaded: a click there opens nothing.
    click(&mut h, "brown");
    assert_eq!(h.state().study().card_word(), None);
    click(&mut h, "quick");
    assert_eq!(h.state().study().card_word(), Some("quick"));
    let pane = h.state().active_pane();
    assert_eq!(h.state().marks(pane).len(), 1);
    // Off: the same word is plain text again.
    h.key_press(egui::Key::Escape);
    h.run();
    let mut s = h.state().settings().clone();
    s.study.cefr.click_explain = false;
    h.state_mut().queue(Command::SetSettings(Box::new(s)));
    h.run();
    click(&mut h, "quick");
    assert_eq!(h.state().study().card_word(), None);
}

#[test]
fn resting_on_a_shaded_word_opens_its_dictionary_card() {
    let (words, packs) = shelves();
    let mut h = reader(words.path(), Some(packs.path()));
    h.hover_at(spot(&h, "quick"));
    h.run_steps(20);
    let dict = h.state().dict();
    assert_eq!(dict.card_word(), Some("quick"));
    assert!(dict.card_rested());
    assert_eq!(dict.card_answers(), Some(vec!["မြန်သော".to_owned()]));
    // Leaving the word and its card closes it.
    h.hover_at(Pos2::new(5.0, 790.0));
    h.run_steps(3);
    assert_eq!(h.state().dict().card_word(), None);
    // With resting off, nothing opens.
    let mut s = h.state().settings().clone();
    s.dictionary.hover = false;
    h.state_mut().queue(Command::SetSettings(Box::new(s)));
    h.run();
    h.hover_at(spot(&h, "quick"));
    h.run_steps(20);
    assert_eq!(h.state().dict().card_word(), None);
}

#[test]
fn the_tab_sets_the_level_and_removes_the_dataset() {
    use egui::accesskit::Role;
    let (words, _) = shelves();
    let mut h = reader(words.path(), None);
    h.set_size(egui::vec2(1000.0, 1200.0));
    h.state_mut().queue(Command::OpenSettings(Tab::Vocabulary));
    h.run();
    h.get_by_label("Reading level");
    h.get_by_label("4 words");
    h.get_by_role_and_label(Role::CheckBox, "Shade words above my level")
        .click();
    // A pen's own colour is kept from shading.
    h.get_by_label("Shading #00b2f4").click();
    h.run();
    let study = &h.state().settings().study;
    assert!(!study.cefr.enabled);
    assert_eq!(study.cefr.level, CefrLevel::default());
    assert_eq!(study.highlights.cefr.color.to_hex(), "#00b2f4");
    h.get_by_label("Remove word levels").click();
    h.run();
    assert_eq!(h.state().vocab().state(Part::Dataset), &State::Absent);
    h.get_by_label("Download word levels");
}
