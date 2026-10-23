//! Voice models in settings: downloaded, a tab of their own; removed, gone.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use mareader_speech::{Build, Gender, Knob, Model, ModelFile, ModelVoice};
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
        from: None,
    }
}

/// A model of a few bytes, from a host that refuses every call.
const TINY: &[Model] = &[
    Model {
        id: "tiny",
        name: "Tiny",
        about: "A model for tests",
        license: "MIT",
        hosts: &["http://127.0.0.1:9/"],
        files: &[file("config.json", 4)],
        builds: &[
            Build {
                id: "big",
                name: "Big",
                about: "Every weight",
                file: file("weights.bin", 5),
                extra: None,
            },
            Build {
                id: "lite",
                name: "Lite",
                about: "Fewer bytes",
                file: file("lite.bin", 3),
                extra: None,
            },
        ],
        voices: &[
            ModelVoice {
                id: "ada",
                name: "Ada",
                language: "en-US",
                gender: Gender::Female,
                file: Some(file("voices/ada.pt", 2)),
                extra: None,
                shared: &[],
                note: "",
            },
            ModelVoice {
                id: "bo",
                name: "Bo",
                language: "en-GB",
                gender: Gender::Male,
                file: Some(file("voices/bo.pt", 3)),
                extra: None,
                shared: &[],
                note: "",
            },
        ],
        first_voice: "ada",
        knobs: &[],
    },
    SOLO,
    DUO,
];

/// A model whose voice is its weights: no builds, a setting of its own.
const SOLO: Model = Model {
    id: "solo",
    name: "Solo",
    about: "A model with no builds",
    license: "MIT",
    hosts: &["http://127.0.0.1:9/"],
    files: &[file("words.bin", 4)],
    builds: &[],
    voices: &[ModelVoice {
        id: "cy",
        name: "Cy",
        language: "en-US",
        gender: Gender::Neutral,
        file: Some(file("cy.onnx", 2)),
        extra: Some(file("cy.onnx.json", 2)),
        shared: &[],
        note: "Slower to make",
    }],
    first_voice: "cy",
    knobs: &[Knob {
        id: "expression",
        name: "Expression",
        about: "How much the tone moves",
        range: (0.0, 1.5),
    }],
};

/// The voices both of Duo's builds bring.
const PACK: ModelFile = file("voices.npz", 3);

/// A voice inside every build, with no file of its own.
const fn inside(id: &'static str, name: &'static str) -> ModelVoice {
    ModelVoice {
        id,
        name,
        language: "en-US",
        gender: Gender::Neutral,
        file: None,
        extra: None,
        shared: &[],
        note: "",
    }
}

/// A model whose builds bring its voices, sharing one file.
const DUO: Model = Model {
    id: "duo",
    name: "Duo",
    about: "A model whose builds hold its voices",
    license: "MIT",
    hosts: &["http://127.0.0.1:9/"],
    files: &[file("words.bin", 4)],
    builds: &[
        Build {
            id: "large",
            name: "Large",
            about: "More weights",
            file: file("large.onnx", 6),
            extra: Some(PACK),
        },
        Build {
            id: "small",
            name: "Small",
            about: "Fewer weights",
            file: file("small.onnx", 2),
            extra: Some(PACK),
        },
    ],
    voices: &[inside("eve", "Eve"), inside("max", "Max")],
    first_voice: "eve",
    knobs: &[],
};

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

/// Puts the model's first download on disk, as a finished one leaves it.
fn downloaded(dir: &std::path::Path) {
    let folder = dir.join("tiny");
    std::fs::create_dir_all(folder.join("voices")).unwrap();
    std::fs::write(folder.join("config.json"), b"{}{}").unwrap();
    std::fs::write(folder.join("weights.bin"), b"12345").unwrap();
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
    // A downloaded model brings the memory rows to Read aloud.
    let kept = h.get_by_role_and_label(egui::accesskit::Role::Slider, "Speech kept");
    assert_eq!(kept.accesskit_node().numeric_value(), Some(64.0));
    let _ = h.get_by_label("Nothing loaded");
    h.get_by_label("Voices…").click();
    h.run_steps(3);
    assert!(matches!(tab(&h), Tab::Model(m) if m.id == "tiny"));
    let _ = h.get_by_label("All voices");
    // The other build is a download away; one build here can't go alone.
    let _ = h.get_by_label("Download Tiny (Lite)");
    let _ = h.get_by_label("Use Big");
    assert!(h.query_by_label("Remove Tiny (Big)").is_none());
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

#[test]
fn a_second_build_is_picked_for_its_model_then_removed_alone() {
    let dir = tempfile::tempdir().unwrap();
    downloaded(dir.path());
    std::fs::write(dir.path().join("tiny/lite.bin"), b"abc").unwrap();
    let mut h = settings(dir.path());
    let model = TINY.first().unwrap();
    h.state_mut()
        .queue(Command::OpenSettings(Tab::Model(model)));
    h.run_steps(3);
    h.get_by_label("Use Lite").click();
    h.run_steps(2);
    let builds = &h.state().settings().read_aloud.builds;
    assert_eq!(builds.get("tiny").map(String::as_str), Some("lite"));
    h.get_by_label("Remove Tiny (Lite)").click();
    h.run_steps(3);
    assert!(!dir.path().join("tiny/lite.bin").exists());
    assert!(dir.path().join("tiny/weights.bin").exists());
    // The model stays, on the build left.
    assert!(matches!(tab(&h), Tab::Model(m) if m.id == "tiny"));
    let _ = h.get_by_label("Download Tiny (Lite)");
    let _ = h.get_by_label("Use Big");
}

#[test]
fn a_model_not_yet_downloaded_offers_a_build_to_start_with() {
    let dir = tempfile::tempdir().unwrap();
    let h = settings(dir.path());
    let _ = h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Tiny build");
    let _ = h.get_by_label("Download Tiny");
}

#[test]
fn a_model_without_builds_downloads_whole_and_its_sound_can_be_set() {
    let dir = tempfile::tempdir().unwrap();
    let h = settings(dir.path());
    assert!(
        (h.query_by_role_and_label(egui::accesskit::Role::ComboBox, "Solo build")).is_none(),
        "nothing to pick"
    );
    let _ = h.get_by_label("Download Solo");
    drop(h);
    let folder = dir.path().join("solo");
    std::fs::create_dir_all(&folder).unwrap();
    for (name, bytes) in [("words.bin", 4), ("cy.onnx", 2), ("cy.onnx.json", 2)] {
        std::fs::write(folder.join(name), vec![0; bytes]).unwrap();
    }
    let mut h = settings(dir.path());
    assert!(h.query_by_label("Download Solo").is_none());
    h.get_by_label("Voices…").click();
    h.run_steps(3);
    assert!(matches!(tab(&h), Tab::Model(m) if m.id == "solo"));
    let _ = h.get_by_label("Use Cy");
    let slider = h.get_by_role_and_label(egui::accesskit::Role::Slider, "Expression");
    assert_eq!(slider.accesskit_node().numeric_value(), Some(1.0));
    h.state_mut()
        .settings_mut()
        .read_aloud
        .knobs
        .insert("solo".into(), [("expression".into(), 0.5)].into());
    h.run_steps(2);
    let slider = h.get_by_role_and_label(egui::accesskit::Role::Slider, "Expression");
    assert_eq!(slider.accesskit_node().numeric_value(), Some(0.5));
    h.get_by_label("Reset").click();
    h.run_steps(2);
    assert!(
        h.state().settings().read_aloud.knobs.is_empty(),
        "back as made"
    );
}

#[test]
fn voices_inside_the_builds_need_no_download_and_builds_share_their_file() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("duo");
    std::fs::create_dir_all(&folder).unwrap();
    let files = [
        ("words.bin", 4),
        ("large.onnx", 6),
        ("small.onnx", 2),
        ("voices.npz", 3),
    ];
    for (name, bytes) in files {
        std::fs::write(folder.join(name), vec![0; bytes]).unwrap();
    }
    let mut h = settings(dir.path());
    let model = TINY.get(2).unwrap();
    h.state_mut()
        .queue(Command::OpenSettings(Tab::Model(model)));
    h.run_steps(3);
    assert!(h.query_by_label("All voices").is_none(), "nothing to fetch");
    assert!(h.query_by_label("Download Max").is_none());
    assert!(h.query_by_label("Remove Max").is_none());
    h.get_by_label("Use Max").click();
    h.run_steps(2);
    let voices = &h.state().settings().read_aloud.voices;
    assert_eq!(voices.get("duo").map(String::as_str), Some("max"));
    h.get_by_label("Remove Duo (Large)").click();
    h.run_steps(3);
    assert!(!folder.join("large.onnx").exists());
    assert!(
        folder.join("voices.npz").exists(),
        "the small build keeps it"
    );
    let _ = h.get_by_label("Use Max");
    let _ = h.get_by_label("Use Small");
}
