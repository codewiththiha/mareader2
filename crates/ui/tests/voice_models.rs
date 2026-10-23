//! Voice models in settings: downloaded, a tab of their own; removed, gone.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use mareader_speech::{Model, ModelFile, ModelVoice};
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::settings_window::Tab;

/// Never checked here: only sizes say what is on disk.
const NO_SUM: &str = "0000000000000000000000000000000000000000000000000000000000000000";

const fn file(path: &'static str, bytes: u64) -> ModelFile {
    ModelFile {
        path,
        bytes,
        sha256: NO_SUM,
        hosts: None,
    }
}

/// A model of a few bytes, from a host that refuses every call.
const TINY: &[Model] = &[Model {
    id: "tiny",
    name: "Tiny",
    about: "A model for tests",
    license: "MIT",
    hosts: &["http://127.0.0.1:9/"],
    files: &[file("config.json", 4)],
    voices: &[
        ModelVoice {
            id: "ada",
            name: "Ada",
            language: "en-US",
            feminine: true,
            file: file("voices/ada.pt", 2),
        },
        ModelVoice {
            id: "bo",
            name: "Bo",
            language: "en-GB",
            feminine: false,
            file: file("voices/bo.pt", 3),
        },
    ],
    first_voice: "ada",
}];

/// The app over `dir`, the Read aloud tab open.
fn settings(dir: &std::path::Path) -> Harness<'static, MareaderApp> {
    let dir = dir.to_owned();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 760.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(move |cc| {
            let mut app = MareaderApp::default();
            app.set_option("set", "read_aloud.engine:\"silent\"").ok();
            app.attach_voice_models_from(&cc.egui_ctx, dir, TINY);
            app
        });
    h.state_mut().queue(Command::OpenSettings(Tab::ReadAloud));
    h.run_steps(4);
    h
}

/// Puts the model's own files on disk, as a finished download leaves them.
fn downloaded(dir: &std::path::Path) {
    let folder = dir.join("tiny");
    std::fs::create_dir_all(folder.join("voices")).unwrap();
    std::fs::write(folder.join("config.json"), b"{}{}").unwrap();
    std::fs::write(folder.join("voices/ada.pt"), b"ad").unwrap();
}

fn tab(h: &Harness<'static, MareaderApp>) -> Tab {
    h.state().settings_window().tab()
}

#[test]
fn a_downloaded_model_has_its_own_tab_until_removed() {
    let dir = tempfile::tempdir().unwrap();
    downloaded(dir.path());
    let mut h = settings(dir.path());
    assert!(h.query_by_label("Download Tiny").is_none());
    h.get_by_label("Voices…").click();
    h.run_steps(3);
    assert!(matches!(tab(&h), Tab::Model(m) if m.id == "tiny"));
    let _ = h.get_by_label("All voices");
    let kept = h.get_by_role_and_label(egui::accesskit::Role::Slider, "Speech kept");
    assert_eq!(kept.accesskit_node().numeric_value(), Some(64.0));
    // The first voice came with it; the other is a download away.
    let _ = h.get_by_label("Download Bo");
    assert!(h.query_by_label("Remove Ada").is_none());
    let _ = h.get_by_label("Use Ada");
    h.get_by_label("Remove Tiny").click();
    h.run_steps(4);
    assert_eq!(tab(&h), Tab::ReadAloud);
    assert!(!dir.path().join("tiny").exists());
    let _ = h.get_by_label("Download Tiny");
}

#[test]
fn a_voice_picked_in_the_model_tab_is_kept_for_its_engine() {
    let dir = tempfile::tempdir().unwrap();
    downloaded(dir.path());
    std::fs::write(dir.path().join("tiny/voices/bo.pt"), b"bo!").unwrap();
    let mut h = settings(dir.path());
    let model = TINY.first().unwrap();
    h.state_mut()
        .queue(Command::OpenSettings(Tab::Model(model)));
    h.run_steps(3);
    assert!(h.query_by_label("Download Bo").is_none());
    h.get_by_label("Use Bo").click();
    h.run_steps(2);
    let voices = &h.state().settings().read_aloud.voices;
    assert_eq!(voices.get("tiny").map(String::as_str), Some("bo"));
    h.get_by_label("Remove Bo").click();
    h.run_steps(3);
    assert!(!dir.path().join("tiny/voices/bo.pt").exists());
    let _ = h.get_by_label("Download Bo");
}

#[test]
fn a_download_that_cannot_reach_its_host_offers_another_try() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = settings(dir.path());
    h.get_by_label("Download Tiny").click();
    for _ in 0..600 {
        h.run_steps(1);
        if h.query_by_label("Try again Tiny").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let _ = h.get_by_label("Try again Tiny");
    let _ = h.get_by_label("Couldn't download");
}
