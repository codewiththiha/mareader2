//! Reading aloud a book with no narration: a voice, a sentence at a time.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_speech::Spot;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::read_aloud::{ReadFrom, Span};

/// A sample PDF, read by the engine that says nothing.
fn reader(pages: usize) -> Harness<'static, MareaderApp> {
    reader_sized(pages, egui::vec2(900.0, 700.0))
}

fn reader_sized(pages: usize, size: egui::Vec2) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("pdf", &format!("sample:{pages}")).ok();
    app.set_option("set", "read_aloud.engine:\"silent\"").ok();
    let mut h = Harness::builder()
        .with_size(size)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

/// Starts reading, waiting for the pages' thread to light a sentence.
fn start(h: &mut Harness<'static, MareaderApp>, from: ReadFrom) {
    let pane = h.state().active_pane();
    h.state_mut().queue(Command::ReadAloud { pane, from });
    step_until(h, |h| lit(h).is_some());
}

/// Steps until `done`, giving the pages' thread time between frames.
fn step_until(
    h: &mut Harness<'static, MareaderApp>,
    done: impl Fn(&Harness<'static, MareaderApp>) -> bool,
) {
    for _ in 0..600 {
        h.run_steps(1);
        if done(h) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// Where the sentence lit now starts and ends.
fn lit(h: &Harness<'static, MareaderApp>) -> Option<(Spot, Spot)> {
    let pane = h.state().active_pane();
    match h.state().read_aloud().lit(pane)?.span {
        Span::Chars { start, end } => Some((start, end)),
        Span::Anchors { .. } => None,
    }
}

#[test]
fn sentences_are_lit_one_after_another_and_pages_follow() {
    let mut h = reader(2);
    start(&mut h, ReadFrom::default());
    let first = lit(&h).expect("the first sentence is lit");
    assert_eq!(first.0.page, 0);
    assert!(first.0 < first.1);
    let _ = h.get_by_label("Pause");
    let _ = h.get_by_label("Next sentence");
    step_until(&mut h, |h| lit(h) != Some(first));
    let second = lit(&h).expect("the next sentence is lit");
    assert!(second.0 >= first.1, "{second:?} comes after {first:?}");
    // Read on to the second page; the pane turns with it.
    step_until(&mut h, |h| lit(h).is_none_or(|(s, _)| s.page > 0));
    assert_eq!(lit(&h).map(|(s, _)| s.page), Some(1));
    h.run_steps(5);
    let pane = h.state().active_pane();
    assert_eq!(h.state().documents().current(pane), Some(1));
}

#[test]
fn pause_holds_the_sentence_and_previous_steps_back() {
    let mut h = reader(1);
    start(&mut h, ReadFrom::default());
    let first = lit(&h);
    step_until(&mut h, |h| lit(h) != first);
    h.get_by_label("Pause").click();
    // The voice may move on in the frame the click lands.
    h.run_steps(2);
    let _ = h.get_by_label("Play");
    let before = lit(&h);
    h.run_steps(6);
    assert_eq!(lit(&h), before, "paused, the same sentence stays lit");
    h.get_by_label("Previous sentence").click();
    h.run_steps(1);
    let back = lit(&h);
    assert!(back < before, "{back:?} is before {before:?}");
    h.get_by_label("Play").click();
    step_until(&mut h, |h| lit(h) > back);
    assert!(lit(&h) > back, "playing goes on");
}

#[test]
fn reading_starts_at_the_character_asked_for() {
    let mut h = reader(2);
    let at = Spot { page: 1, char: 0 };
    let from = ReadFrom {
        anchor: None,
        at: Some(at),
    };
    start(&mut h, from);
    assert_eq!(lit(&h).map(|(s, _)| s), Some(at));
}

#[test]
fn the_reading_ends_and_says_so() {
    let mut h = reader(1);
    start(&mut h, ReadFrom::default());
    step_until(&mut h, |h| lit(h).is_none());
    h.run_steps(1);
    let _ = h.get_by_label("The end");
    h.get_by_label("Stop reading aloud").click();
    h.run_steps(2);
    assert!(!h.state().read_aloud().active());
}

#[test]
fn a_sentence_below_the_fold_is_brought_into_view() {
    let mut h = reader_sized(1, egui::vec2(900.0, 360.0));
    start(&mut h, ReadFrom::default());
    step_until(&mut h, |h| lit(h).is_none_or(|(s, _)| s.char > 250));
    let (start, end) = lit(&h).expect("a sentence far down is lit");
    h.get_by_label("Pause").click();
    h.run_steps(40);
    let pane = h.state().active_pane();
    let rects = h.state().pane_rects().to_vec();
    let (_, shown) = rects
        .into_iter()
        .find(|(p, _)| *p == pane)
        .expect("the pane");
    let chars = start.char..end.char.max(start.char + 1);
    let at = h
        .state()
        .documents()
        .hit_box(pane, 0, chars)
        .expect("drawn");
    assert!(shown.contains_rect(at), "{at:?} shows within {shown:?}");
}

/// Scrolls the control named `label` into view, then clicks it.
fn press(h: &mut Harness<'static, MareaderApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run_steps(30);
    h.get_by_label(label).click();
}

/// The reader with settings open on the Read aloud tab.
fn voice_settings(pages: usize) -> Harness<'static, MareaderApp> {
    let mut h = reader(pages);
    let tab = mareader_ui::settings_window::Tab::ReadAloud;
    h.state_mut().queue(Command::OpenSettings(tab));
    h.run_steps(4);
    h
}

#[test]
fn the_tab_lists_the_engine_voices_and_picks_one() {
    let mut h = voice_settings(1);
    assert!(h.state().read_aloud().booth.is_open());
    // The list starts folded, its pick named on the fold.
    assert!(h.query_by_label("System default").is_none());
    press(&mut h, "Voices");
    h.run_steps(2);
    // English first: the French voice waits behind the language picker.
    let _ = h.get_by_label("System default");
    assert!(h.query_by_label("Hush").is_none());
    press(&mut h, "Quiet");
    h.run_steps(2);
    let picked = h.state().settings().read_aloud.voice().map(str::to_owned);
    assert_eq!(picked.as_deref(), Some("quiet-en"));
    press(&mut h, "System default");
    h.run_steps(2);
    assert_eq!(h.state().settings().read_aloud.voice(), None);
    // The list folds away, its pick still named.
    press(&mut h, "Voices");
    h.run_steps(2);
    assert!(h.query_by_label("Quiet").is_none());
    press(&mut h, "Voices");
    h.run_steps(2);
    let _ = h.get_by_label("Quiet");
}

#[test]
fn each_voice_says_a_line_in_itself() {
    let mut h = voice_settings(1);
    press(&mut h, "Voices");
    h.run_steps(2);
    press(&mut h, "Preview Quiet");
    h.run_steps(1);
    let booth = &h.state().read_aloud().booth;
    assert_eq!(booth.samples(), 1);
    assert_eq!(booth.trying_voice(), Some("quiet-en"));
    // Its picked voice stays as it was.
    assert_eq!(h.state().settings().read_aloud.voice(), None);
    // Once the line ends, its button offers it again.
    step_until(&mut h, |h| !h.state().read_aloud().booth.trying());
    h.run_steps(1);
    let _ = h.get_by_label("Preview Quiet");
}

#[test]
fn a_preview_says_a_line_and_the_book_waits() {
    let mut h = voice_settings(1);
    start(&mut h, ReadFrom::default());
    assert!(h.state().read_aloud().playing());
    press(&mut h, "Preview");
    h.run_steps(1);
    assert_eq!(h.state().read_aloud().booth.samples(), 1);
    assert!(!h.state().read_aloud().playing(), "the book pauses");
    // The silent voice ends its line within a few looks.
    step_until(&mut h, |h| !h.state().read_aloud().booth.trying());
    let _ = h.get_by_label("Preview");
    h.state_mut().queue(Command::CloseSettings);
    h.run_steps(2);
    assert!(
        !h.state().read_aloud().booth.is_open(),
        "closing lets it go"
    );
}

#[test]
fn the_shortcut_starts_reading_then_pauses_it() {
    let mut h = reader(1);
    let keys = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
    h.key_press_modifiers(keys, egui::Key::U);
    step_until(&mut h, |h| lit(h).is_some());
    assert!(h.state().read_aloud().playing());
    h.key_press_modifiers(keys, egui::Key::U);
    h.run_steps(1);
    assert!(!h.state().read_aloud().playing(), "the same keys pause");
    assert!(lit(&h).is_some(), "paused, it stays lit");
}
