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
        until: None,
    };
    start(&mut h, from);
    assert_eq!(lit(&h).map(|(s, _)| s), Some(at));
}

#[test]
fn a_selection_is_read_alone_and_again_from_its_start() {
    let mut h = reader(2);
    let (at, until) = (Spot { page: 0, char: 0 }, Spot { page: 0, char: 120 });
    let from = ReadFrom {
        anchor: None,
        at: Some(at),
        until: Some(until),
    };
    start(&mut h, from);
    let first = lit(&h);
    assert_eq!(first.map(|(s, _)| s), Some(at));
    // Each sentence lit ends by the selection's end, then reading stops.
    for _ in 0..600 {
        h.run_steps(1);
        let Some((_, end)) = lit(&h) else {
            break;
        };
        assert!(end <= until, "{end:?} is past {until:?}");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    h.run_steps(1);
    let _ = h.get_by_label("Selection read");
    // Play again reads the selection again, not the book.
    h.get_by_label("Play").click();
    step_until(&mut h, |h| lit(h).is_some());
    assert_eq!(lit(&h), first);
}

#[test]
fn the_selection_bar_reads_just_the_selection() {
    let mut h = reader(1);
    let pane = h.state().active_pane();
    let text = "The quick brown fox jumps over the lazy dog.".to_owned();
    h.state_mut().queue(Command::SelectText {
        pane,
        page: 0,
        text,
    });
    h.run();
    h.get_by_label("Read aloud").click();
    step_until(&mut h, |h| lit(h).is_some());
    assert!(h.state().documents().selection(pane).is_none());
    let (start, _) = lit(&h).expect("the selection is lit");
    assert!(
        start.char > 100,
        "{start:?} is the selection's, not the page's"
    );
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

#[test]
fn a_book_set_in_new_type_reads_on_from_the_same_sentence() {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("epub", "sample:3").ok();
    app.set_option("set", "read_aloud.engine:\"silent\"").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    let pane = h.state().active_pane();
    let from = ReadFrom {
        anchor: None,
        at: Some(Spot { page: 1, char: 0 }),
        until: None,
    };
    start(&mut h, from);
    let first = lit(&h);
    step_until(&mut h, |h| lit(h) != first);
    h.get_by_label("Pause").click();
    h.run_steps(2);
    let said = |h: &Harness<'static, MareaderApp>| h.state().read_aloud().said().map(|(_, a)| a);
    let before = said(&h).expect("the sentence has a place");
    let doc =
        |h: &Harness<'static, MareaderApp>| h.state().documents().reading(pane).map(|r| r.doc);
    let was = doc(&h);
    // Bigger type sets the book anew, as another document.
    h.state_mut().set_option("text_size", "28").ok();
    step_until(&mut h, |h| doc(h) != was && lit(h).is_some());
    assert_ne!(doc(&h), was, "set anew");
    assert!(h.state().read_aloud().active(), "reading goes on");
    assert_eq!(said(&h), Some(before), "the same sentence");
    let _ = h.get_by_label("Play");
    // Lit, and shown, where the new pages have it.
    let reading = h.state().documents().reading(pane).expect("a book");
    let page = reading.document.locate(before);
    assert!(page.is_some(), "the new pages have it");
    assert_eq!(lit(&h).map(|(s, _)| s.page), page);
    let shown = h.state().documents().visible(pane).unwrap_or(0..0);
    assert!(
        page.is_some_and(|p| shown.contains(&p)),
        "{page:?} in {shown:?}"
    );
    // Playing goes on past it.
    h.get_by_label("Play").click();
    let now = lit(&h);
    step_until(&mut h, |h| lit(h) > now);
    assert!(lit(&h) > now, "playing goes on");
}

/// The first character of `word` on page 0, and where it sits on screen.
fn word(h: &Harness<'static, MareaderApp>, word: &str) -> (Spot, egui::Rect) {
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let layer = docs.reading(pane).map(|r| r.document.text_layer(0));
    let layer = layer.and_then(Result::ok).expect("page 0's text");
    let sheet = *docs
        .sheets(pane)
        .iter()
        .find(|s| s.page == 0)
        .expect("page 0");
    let letters: Vec<char> = layer.text.chars().iter().map(|c| c.ch).collect();
    let want: Vec<char> = word.chars().collect();
    let char = (letters.windows(want.len()))
        .position(|w| w == want.as_slice())
        .expect("the word is on the page");
    let bounds = layer.text.chars().get(char).expect("its letter").bounds;
    (Spot { page: 0, char }, sheet.to_screen(bounds))
}

fn pointer(h: &mut Harness<'static, MareaderApp>, pos: egui::Pos2, pressed: Option<bool>) {
    h.event(egui::Event::PointerMoved(pos));
    if let Some(pressed) = pressed {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(1);
}

fn marker(h: &Harness<'static, MareaderApp>) -> Option<Spot> {
    let pane = h.state().active_pane();
    h.state().documents().read_marker(pane)
}

#[test]
fn the_page_menu_pins_the_start_and_reads_once_it_rests() {
    let mut h = reader(1);
    let (quick, at) = word(&h, "quick");
    // Right-click inside the word; the pin goes to its start.
    let inside = at.center() + egui::vec2(at.width() * 2.0, 0.0);
    h.event(egui::Event::PointerMoved(inside));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: inside,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(3);
    h.get_by_label("Read aloud from “quick”").click();
    h.run_steps(3);
    assert_eq!(marker(&h), Some(quick), "pinned at the word's start");
    let _ = h.get_by_label("Reading starts here");
    h.run_steps(40);
    assert!(lit(&h).is_none(), "nothing reads before it rests");
    step_until(&mut h, |h| lit(h).is_some());
    assert_eq!(lit(&h).map(|(s, _)| s), Some(quick));
    assert_eq!(marker(&h), None, "the pin gives way to the reading");
    h.run_steps(30);
    assert!(h.query_by_label("Reading starts here").is_none());
}

#[test]
fn the_pin_is_dragged_to_a_word_and_reads_from_there_after_resting() {
    let mut h = reader(1);
    let pane = h.state().active_pane();
    let (quick, from) = word(&h, "quick");
    let (lazy, to) = word(&h, "lazy");
    h.state_mut()
        .queue(Command::PlaceReadMarker { pane, spot: quick });
    h.run_steps(20);
    let pin = h.get_by_label("Reading starts here").rect();
    let grab = egui::pos2(pin.center().x, pin.min.y + 14.0);
    pointer(&mut h, grab, Some(true));
    // Carried by the knob: the word it ends beside is the one taken.
    let left = |r: egui::Rect| egui::pos2(r.left(), r.center().y);
    let drop = left(to) + (grab - left(from));
    for k in 1..=12 {
        let p = grab + (drop - grab) * (k as f32 / 12.0);
        pointer(&mut h, p, None);
    }
    h.run_steps(5);
    assert_eq!(marker(&h), Some(lazy), "it snaps to the word's start");
    h.run_steps(100);
    assert!(lit(&h).is_none(), "nothing reads while it is held");
    pointer(&mut h, drop, Some(false));
    h.run_steps(30);
    assert!(lit(&h).is_none(), "it rests a moment first");
    step_until(&mut h, |h| lit(h).is_some());
    assert_eq!(lit(&h).map(|(s, _)| s), Some(lazy));
}

#[test]
fn escape_sends_the_pin_away_unread() {
    let mut h = reader(1);
    let pane = h.state().active_pane();
    let (quick, _) = word(&h, "quick");
    h.state_mut()
        .queue(Command::PlaceReadMarker { pane, spot: quick });
    h.run_steps(5);
    assert_eq!(marker(&h), Some(quick));
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!(marker(&h), None);
    h.run_steps(120);
    assert!(lit(&h).is_none(), "nothing reads");
    assert!(h.query_by_label("Reading starts here").is_none());
}

#[test]
fn the_right_clicked_word_is_lit_where_reading_would_start() {
    let mut h = reader(1);
    let pane = h.state().active_pane();
    let (quick, _) = word(&h, "quick");
    // The right edge of its last letter is still the word, not the next.
    let k = {
        let docs = h.state().documents();
        let layer = docs.reading(pane).map(|r| r.document.text_layer(0));
        let layer = layer.and_then(Result::ok).expect("page 0's text");
        let sheet = *docs
            .sheets(pane)
            .iter()
            .find(|s| s.page == 0)
            .expect("page 0");
        let bounds = layer
            .text
            .chars()
            .get(quick.char + 4)
            .expect("its k")
            .bounds;
        sheet.to_screen(bounds)
    };
    let at = egui::pos2(k.right() - 1.0, k.center().y);
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(3);
    let lit_word = h
        .state()
        .page_ai()
        .washed_word(pane)
        .expect("a word is lit");
    assert_eq!(lit_word.page, 0);
    assert!(
        (quick.char..quick.char + 5).contains(&lit_word.char),
        "{lit_word:?} is in “quick” at {quick:?}"
    );
    h.get_by_label("Read aloud from “quick”").click();
    h.run_steps(3);
    assert_eq!(marker(&h), Some(quick), "the pin takes the lit word");
}

fn right_click(h: &mut Harness<'static, MareaderApp>, at: egui::Pos2) {
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(12);
}

#[test]
fn the_menu_keeps_off_its_word_and_rides_along_with_it() {
    let mut h = reader(2);
    let pane = h.state().active_pane();
    let (_, at) = word(&h, "quick");
    right_click(&mut h, at.center());
    let menu = h.state().page_ai().menu_rect().expect("the menu shows");
    let (_, lit) = word(&h, "quick");
    let whole = lit.expand2(egui::vec2(lit.width() * 4.0, 0.0));
    assert!(!menu.intersects(lit), "{menu:?} covers the word at {lit:?}");
    assert!(menu.top() > whole.bottom() || menu.bottom() < whole.top());
    // Scrolled a little, the menu moves just as its word does.
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -30.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(20);
    let (_, moved) = word(&h, "quick");
    let dy = moved.top() - lit.top();
    assert!(dy < -5.0, "the page scrolled: {dy}");
    let now = h.state().page_ai().menu_rect().expect("the menu stays");
    assert!(
        (now.top() - menu.top() - dy).abs() < 1.5,
        "{now:?} after {menu:?}, {dy}"
    );
    assert_eq!(h.state().page_ai().washed(pane), Some(&[0][..]));
}

#[test]
fn off_the_text_it_reads_from_the_page_s_start() {
    let mut h = reader(1);
    let pane = h.state().active_pane();
    let sheet = *h.state().documents().sheets(pane).first().expect("a page");
    right_click(&mut h, sheet.shown.min + egui::vec2(14.0, 14.0));
    h.get_by_label("Read aloud from page start").click();
    h.run_steps(3);
    let first = marker(&h).expect("a pin");
    assert_eq!(first, Spot { page: 0, char: 0 }, "the page's first word");
}

#[test]
fn a_pin_off_screen_is_brought_into_view_before_it_reads() {
    let mut h = reader(3);
    let pane = h.state().active_pane();
    let spot = Spot { page: 2, char: 0 };
    h.state_mut().queue(Command::PlaceReadMarker { pane, spot });
    h.run_steps(40);
    assert!(lit(&h).is_none(), "it waits its rest first");
    let shown = |h: &Harness<'static, MareaderApp>| {
        let sheets = h.state().documents().sheets(pane);
        sheets
            .iter()
            .any(|s| s.page == 2 && s.shown.height() > 40.0)
    };
    assert!(shown(&h), "its page came into view");
    assert!(h.query_by_label("Reading starts here").is_some());
    step_until(&mut h, |h| lit(h).is_some());
    assert_eq!(lit(&h).map(|(s, _)| s), Some(spot));
}
