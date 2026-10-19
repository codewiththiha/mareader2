//! The page AI menu and kept results, headless.

use std::sync::Arc;

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_pdf::TextLayer;
use mareader_settings::PageStart;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::page_ai::menu::FIND;
use mareader_ui::selection::Sheet;

type H = Harness<'static, MareaderApp>;
type Out = Result<(), Box<dyn std::error::Error>>;

fn reader(options: &[(&str, &str)]) -> H {
    let mut app = MareaderApp::default();
    for (k, v) in [
        ("workers", "0"),
        ("explain", "canned"),
        ("animations", "false"),
    ] {
        app.set_option(k, v).ok();
    }
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

/// A reader with no model at all, even on a Mac.
fn bare() -> H {
    let mut h = reader(&[("pdf", "sample:5")]);
    let mut settings = h.state().settings().clone();
    settings.ai.presets.clear();
    h.state_mut()
        .queue(Command::SetSettings(Box::new(settings)));
    h.run();
    h
}

/// Page `page`'s text and where it sits on screen.
fn page(h: &H, page: usize) -> Result<(Arc<TextLayer>, Sheet), String> {
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let reading = docs.reading(pane).ok_or("no document")?;
    let layer = reading
        .document
        .text_layer(page)
        .map_err(|e| e.to_string())?;
    let sheets = docs.sheets(pane);
    let sheet = *sheets
        .iter()
        .find(|s| s.page == page)
        .ok_or("not on screen")?;
    Ok((Arc::new(layer), sheet))
}

fn find(layer: &TextLayer, needle: &str) -> Result<usize, String> {
    let chars: String = layer.text.chars().iter().map(|c| c.ch).collect();
    let at = chars
        .find(needle)
        .ok_or(format!("{needle} not on the page"))?;
    Ok(chars.get(..at).unwrap_or_default().chars().count())
}

/// Character `i`'s middle on screen.
fn middle(layer: &TextLayer, sheet: Sheet, i: usize) -> Result<Pos2, String> {
    let c = layer.text.chars().get(i).ok_or("no such character")?;
    Ok(sheet.to_screen(c.bounds).center())
}

/// A blank spot near the page's top left corner.
fn margin(sheet: Sheet) -> Pos2 {
    sheet.shown.min + egui::vec2(12.0, 12.0)
}

/// A press without running to rest, for menus that start work.
fn tap(h: &mut H, pos: Pos2, button: PointerButton) {
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
}

fn press(h: &mut H, pos: Pos2, button: PointerButton) {
    tap(h, pos, button);
    h.run();
}

#[test]
fn without_a_model_the_page_menu_offers_setting_one_up() -> Out {
    let mut h = bare();
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    assert!(h.query_by_label("Set up AI…").is_some());
    assert!(h.query_by_label("Summarize page").is_none());
    Ok(())
}

#[test]
fn with_a_model_the_page_menu_offers_the_three_tasks() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("provider", "ollama")]);
    let (layer, sheet) = page(&h, 0)?;
    let on_text = middle(&layer, sheet, find(&layer, "quick")? + 2)?;
    press(&mut h, on_text, PointerButton::Secondary);
    for row in ["Summarize page", "Translate page…", "Simplify page"] {
        assert!(h.query_by_label(row).is_some(), "{row} is offered");
    }
    Ok(())
}

#[test]
fn a_picked_language_is_remembered_for_next_time() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("provider", "ollama")]);
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Translate page…").click();
    h.run();
    h.get_by_label(FIND).type_text("burm");
    h.run();
    h.get_by_label("Burmese").click();
    h.run();
    h.get_by_label("This page").click();
    // The run starts at once; the busy bubble keeps the frames coming.
    h.run_steps(3);
    let translate = &h.state().settings().ai.tasks.translate;
    assert_eq!(translate.language.as_deref(), Some("my"));
    assert_eq!(translate.recent.first().map(String::as_str), Some("my"));
    Ok(())
}

#[test]
fn a_mark_or_a_selection_keeps_its_own_menu() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("provider", "ollama"),
        ("selection", "0:quick"),
        ("gloss", "open"),
    ]);
    h.run();
    let (layer, sheet) = page(&h, 0)?;
    let marked = middle(&layer, sheet, find(&layer, "quick")? + 2)?;
    press(&mut h, marked, PointerButton::Secondary);
    assert!(h.query_by_label("Remove highlight").is_some());
    assert!(h.query_by_label("Summarize page").is_none());
    Ok(())
}

#[test]
fn a_kept_result_covers_the_page_until_the_original_is_asked_for() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("provider", "ollama"),
        ("page_ai", "0:simplify"),
    ]);
    h.run_steps(4);
    h.run();
    let pane = h.state().active_pane();
    assert!(h.state().page_ai().shows(pane, 0), "the result shows");
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Show original page").click();
    h.run();
    assert!(!h.state().page_ai().shows(pane, 0));
    Ok(())
}

/// Starts Simplify in continuous mode from the page menu.
fn simplify_on(h: &mut H, remember: bool) -> Result<(), String> {
    let (_, sheet) = page(h, 0)?;
    press(h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Simplify page").click();
    h.run();
    if remember {
        h.get_by_label("Remember my choice").click();
        h.run();
    }
    h.get_by_label("Continuous").click();
    // Its first page is being written: the frames keep coming.
    h.run_steps(3);
    Ok(())
}

#[test]
fn simplify_asks_how_and_can_remember_continuous() -> Out {
    let mut h = reader(&[("pdf", "sample:40"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    simplify_on(&mut h, true)?;
    let start = h.state().settings().ai.tasks.simplify.start;
    assert_eq!(start, PageStart::Continuous);
    assert!(h.state().page_ai().following(pane).is_some());
    Ok(())
}

#[test]
fn a_task_set_to_this_page_runs_without_asking() -> Out {
    let mut h = reader(&[("pdf", "sample:40"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    let mut settings = h.state().settings().clone();
    settings.ai.tasks.simplify.start = PageStart::Page;
    h.state_mut()
        .queue(Command::SetSettings(Box::new(settings)));
    h.run();
    let (_, sheet) = page(&h, 0)?;
    press(&mut h, margin(sheet), PointerButton::Secondary);
    h.get_by_label("Simplify page").click();
    h.run_steps(3);
    assert!(h.query_by_label("Continuous").is_none(), "no second step");
    assert!(h.state().page_ai().following(pane).is_none());
    assert!(
        h.state().page_ai().shows(pane, 0),
        "the page is being written"
    );
    Ok(())
}

#[test]
fn the_pill_stops_continuous_mode() -> Out {
    let mut h = reader(&[("pdf", "sample:40"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    simplify_on(&mut h, false)?;
    h.get_by_label("Stop").click();
    h.run_steps(3);
    assert!(h.state().page_ai().following(pane).is_none());
    Ok(())
}

#[test]
fn going_far_away_pauses_then_asks_to_carry_on_there() -> Out {
    let mut h = reader(&[("pdf", "sample:40"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    simplify_on(&mut h, false)?;
    h.state_mut().queue(Command::GoTo { pane, page: 30 });
    // Kept still a moment far from page 0, it asks.
    h.run_steps(120);
    assert!(h.query_by_label("Continue").is_some(), "the pill asks");
    h.get_by_label("Continue").click();
    h.run_steps(3);
    assert!(h.query_by_label("Continue").is_none());
    assert!(h.state().page_ai().following(pane).is_some());
    assert!(h.state().page_ai().shows(pane, 30), "it carries on there");
    Ok(())
}

#[test]
fn staying_on_a_done_page_asks_the_next_pages() -> Out {
    let mut h = reader(&[("pdf", "sample:40"), ("provider", "ollama")]);
    let pane = h.state().active_pane();
    let mut settings = h.state().settings().clone();
    settings.ai.tasks.pages.dwell = 5;
    settings.ai.tasks.pages.at_once = 3;
    h.state_mut()
        .queue(Command::SetSettings(Box::new(settings)));
    h.run();
    simplify_on(&mut h, false)?;
    h.run_steps(30);
    let ai = h.state().page_ai();
    assert!(ai.shows(pane, 0), "the first page is done");
    assert!(!ai.shows(pane, 1), "nothing ahead before the stay");
    // Five seconds on the page, at 60 frames a second.
    h.run_steps(330);
    let ai = h.state().page_ai();
    for p in 1..=3 {
        assert!(ai.shows(pane, p), "page {p} is asked");
    }
    assert!(!ai.shows(pane, 4), "no more than pages at once");
    Ok(())
}
