//! How pages sit: gap, margin, default fit, Auto Resize and the label.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_settings::StartupFit;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::zoom::Fit;

type H = Harness<'static, MareaderApp>;

fn reader(options: &[(&str, &str)]) -> H {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn sheets(h: &H) -> Vec<egui::Rect> {
    let pane = h.state().active_pane();
    let sheets = h.state().documents().sheets(pane);
    sheets.iter().map(|s| s.rect).collect()
}

fn zoom(h: &H) -> (f32, Option<Fit>) {
    let pane = h.state().active_pane();
    let z = h.state().documents().zoom_state(pane);
    z.map_or((0.0, None), |z| (z.zoom, z.fit))
}

fn go(h: &mut H, page: usize) {
    let pane = h.state().active_pane();
    h.state_mut().queue(Command::GoTo { pane, page });
    // Past the glide, then past Auto Resize's settle.
    h.run();
    h.run_steps(30);
    h.run();
}

#[test]
fn no_gap_lets_strip_pages_touch() {
    let mut h = reader(&[("pdf", "sample:5")]);
    // Zoomed well out, several pages share the view.
    for _ in 0..5 {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Minus);
    }
    h.run();
    let s = sheets(&h);
    assert!((s[1].top() - s[0].bottom() - 12.0).abs() < 1.0, "{s:?}");
    h.state_mut().settings_mut().layout.no_gap = true;
    h.run();
    let s = sheets(&h);
    assert!((s[1].top() - s[0].bottom()).abs() < 1.0, "{s:?}");
}

#[test]
fn the_page_margin_is_air_beside_a_fitted_page() {
    let mut h = reader(&[("pdf", "sample:5")]);
    let edge = sheets(&h)[0];
    h.state_mut().settings_mut().layout.page_margin = 32.0;
    h.run();
    let inset = sheets(&h)[0];
    assert!((edge.width() - inset.width() - 64.0).abs() < 1.0);
    assert!((inset.left() - edge.left() - 32.0).abs() < 1.0);
}

#[test]
fn documents_open_at_the_default_fit() {
    let mut h = reader(&[]);
    h.state_mut().settings_mut().layout.startup_fit = StartupFit::Page;
    h.state_mut().set_option("pdf", "sample:5").ok();
    h.run_steps(3);
    h.run();
    let (zoom, fit) = zoom(&h);
    assert_eq!(fit, Some(Fit::Page));
    // A letter page by its height: 700 high less the strip's ends.
    assert!(zoom < 1.0, "{zoom}");
}

#[test]
fn auto_resize_refits_to_a_page_of_another_size() {
    // Every seventh sample page lies on its side, 792 wide.
    let mut h = reader(&[("pdf", "sample:20")]);
    let (letter, fit) = zoom(&h);
    assert_eq!(fit, Some(Fit::Width));
    go(&mut h, 6);
    let (wide, _) = zoom(&h);
    let want = letter * 612.0 / 792.0;
    assert!((wide - want).abs() < 0.01, "{wide} want {want}");
    go(&mut h, 7);
    assert!((zoom(&h).0 - letter).abs() < 0.01);
    h.state_mut().settings_mut().layout.auto_resize = false;
    go(&mut h, 6);
    assert!((zoom(&h).0 - letter).abs() < 0.01, "off, the fit holds");
}

#[test]
fn the_floating_label_shows_while_the_bar_hides_and_room_allows() {
    let mut h = reader(&[("pdf", "sample:5"), ("label", "chapter")]);
    let chapter = "Chapter 1: Opening";
    // Fitted edge to edge, there is no room beside the page.
    assert!(h.query_by_label(chapter).is_none());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Minus);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Minus);
    h.run();
    let _ = h.get_by_label(chapter);
    h.state_mut().set_option("sidebar", "outline").ok();
    h.run();
    let shown = h.query_all_by_label(chapter).count();
    assert_eq!(shown, 1, "only the outline's entry, not the label");
}

#[test]
fn always_show_keeps_the_label_under_a_pinned_bar() {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("label", "chapter"),
        ("pinned", "true"),
    ]);
    assert!(h.query_by_label("Chapter 1: Opening").is_none());
    h.state_mut().settings_mut().layout.floating_label_persist = true;
    h.run();
    let _ = h.get_by_label("Chapter 1: Opening");
}

#[test]
fn the_margin_stepper_steps_by_four_and_reads_off_at_zero() {
    let mut h = reader(&[("pdf", "sample:5"), ("settings", "layout")]);
    let _ = h.get_by_label("Off");
    h.get_by_label("More page margin").scroll_to_me();
    h.run();
    h.get_by_label("More page margin").click();
    h.run();
    h.get_by_label("More page margin").click();
    h.run();
    assert_eq!(h.state().settings().layout.page_margin, 8.0);
    let _ = h.get_by_label("8 px");
}

#[test]
fn the_progress_line_follows_the_scroll() {
    let mut h = reader(&[("pdf", "sample:60")]);
    let pane = h.state().active_pane();
    let progress = |h: &H| {
        h.state()
            .documents()
            .reading(pane)
            .map_or(-1.0, |r| r.progress)
    };
    assert!(progress(&h) < 0.05, "{}", progress(&h));
    go(&mut h, 30);
    let mid = progress(&h);
    assert!((0.4..0.6).contains(&mid), "{mid}");
    h.state_mut().queue(Command::LastPage);
    h.run();
    assert!(progress(&h) > 0.99, "{}", progress(&h));
}

fn tiles(h: &H) -> usize {
    h.state().documents().meter().tiles
}

#[test]
fn a_deep_zoom_draws_tiles_once_the_view_rests() {
    let mut h = reader(&[("pdf", "sample:3")]);
    assert_eq!(tiles(&h), 0, "a fitted page is one texture");
    // At 500 %, a page is 3960 px high: past egui's 2048 texture side.
    for _ in 0..12 {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Equals);
    }
    h.run_steps(30);
    h.run();
    let rested = tiles(&h);
    assert!((1..=6).contains(&rested), "{rested} tiles on screen");
    // A pan asks for none until it stops.
    h.hover_at(egui::pos2(450.0, 400.0));
    for _ in 0..10 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -150.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
        assert!(tiles(&h) <= rested, "a tile arrived mid-pan");
    }
    h.run_steps(30);
    h.run();
    assert!(tiles(&h) > rested, "the new rows came once it rested");
    let bytes = h.state().documents().meter().page_bytes;
    assert!(bytes < mareader_ui::pages::CAP_BYTES, "{bytes}");
}
