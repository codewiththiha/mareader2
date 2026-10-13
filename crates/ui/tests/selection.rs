//! Selecting text and following links, headless, on the sample PDF.

use std::sync::Arc;

use egui::{Event, Modifiers, OutputCommand, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use mareader_pdf::{TextLayer, sample};
use mareader_ui::MareaderApp;
use mareader_ui::selection::Sheet;

type H = Harness<'static, MareaderApp>;
type Out = Result<(), Box<dyn std::error::Error>>;

fn reader() -> H {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("pdf", "sample:40").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

/// The first page's text and where that page sits on screen.
fn page_one(h: &H) -> Result<(Arc<TextLayer>, Sheet), String> {
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let reading = docs.reading(pane).ok_or("no document")?;
    let layer = reading.document.text_layer(0).map_err(|e| e.to_string())?;
    let sheet = *docs.sheets(pane).first().ok_or("no page on screen")?;
    Ok((Arc::new(layer), sheet))
}

/// Character `i`'s left or right edge, half way up, on screen.
fn edge(layer: &TextLayer, sheet: Sheet, i: usize, right: bool) -> Result<Pos2, String> {
    let c = layer.text.chars().get(i).ok_or("no such character")?;
    let r = sheet.to_screen(c.bounds);
    let x = if right {
        r.right() - 1.0
    } else {
        r.left() + 1.0
    };
    Ok(pos2(x, r.center().y))
}

/// Where `needle` starts in the page's text, as a character index.
fn find(layer: &TextLayer, needle: &str) -> Result<usize, String> {
    let chars: String = layer.text.chars().iter().map(|c| c.ch).collect();
    let at = chars
        .find(needle)
        .ok_or(format!("{needle} not on the page"))?;
    Ok(chars.get(..at).unwrap_or_default().chars().count())
}

fn button(h: &mut H, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.step();
}

fn drag(h: &mut H, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.step();
    button(h, from, true);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.step();
    }
    button(h, to, false);
    h.run();
}

fn clicks(h: &mut H, at: Pos2, n: usize) {
    h.hover_at(at);
    h.step();
    for _ in 0..n {
        button(h, at, true);
        button(h, at, false);
    }
    h.run();
}

/// Copies, returning what reached the clipboard this frame.
fn copy(h: &mut H) -> Option<String> {
    h.event(Event::Copy);
    h.step();
    h.output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
}

fn selected(h: &H) -> bool {
    let pane = h.state().active_pane();
    h.state().documents().selection(pane).is_some()
}

#[test]
fn dragging_over_text_selects_it_and_copy_takes_exactly_that() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    assert!(
        h.state().documents().text_layers() > 0,
        "text read for the view"
    );
    let quick = find(&layer, "The quick")?;
    let fox = find(&layer, "fox")? + 2;
    drag(
        &mut h,
        edge(&layer, sheet, quick, false)?,
        edge(&layer, sheet, fox, true)?,
    );
    assert!(selected(&h));
    assert_eq!(copy(&mut h).as_deref(), Some("The quick brown fox"));
    // Dragged backwards, across lines, it's the same text in order.
    let sphinx = find(&layer, "Sphinx")?;
    drag(
        &mut h,
        edge(&layer, sheet, sphinx + 5, true)?,
        edge(&layer, sheet, fox - 2, false)?,
    );
    let text = copy(&mut h).ok_or("nothing copied")?;
    assert!(text.starts_with("fox jumps"), "{text}");
    assert!(text.ends_with("\nSphinx"), "{text}");
    Ok(())
}

#[test]
fn double_and_triple_clicks_take_a_word_and_a_line() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    let quartz = find(&layer, "quartz")? + 2;
    let at = edge(&layer, sheet, quartz, false)?;
    clicks(&mut h, at, 2);
    assert_eq!(copy(&mut h).as_deref(), Some("quartz"));
    h.run_steps(40);
    clicks(&mut h, at, 3);
    let line = copy(&mut h);
    assert_eq!(
        line.as_deref(),
        Some("Sphinx of black quartz, judge my vow.")
    );
    // Escape clears it; a plain click elsewhere would too.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!selected(&h));
    assert_eq!(copy(&mut h), None);
    Ok(())
}

#[test]
fn text_shows_a_text_cursor_and_links_a_hand() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    h.hover_at(edge(&layer, sheet, find(&layer, "Sphinx")?, false)?);
    h.run();
    assert_eq!(
        h.output().platform_output.cursor_icon,
        egui::CursorIcon::Text
    );
    let link = layer.links.first().ok_or("no link")?;
    h.hover_at(sheet.to_screen(link.bounds).center());
    h.run();
    let cursor = h.output().platform_output.cursor_icon;
    assert_eq!(cursor, egui::CursorIcon::PointingHand);
    Ok(())
}

#[test]
fn an_internal_link_turns_this_pane_and_a_web_link_opens_the_browser() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    let [chapter, web] = layer.links.as_slice() else {
        return Err("the sample page has two links".into());
    };
    let pane = h.state().active_pane();
    // The browser is asked in the frame of the click itself.
    let at = sheet.to_screen(web.bounds).center();
    h.hover_at(at);
    h.step();
    button(&mut h, at, true);
    button(&mut h, at, false);
    let opened = h
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            OutputCommand::OpenUrl(o) => Some(o.url.clone()),
            _ => None,
        });
    assert_eq!(opened.as_deref(), Some(sample::WEB_LINK));
    assert_eq!(h.state().documents().current(pane), Some(0));
    // "Next chapter, page 6".
    clicks(&mut h, sheet.to_screen(chapter.bounds).center(), 1);
    h.run_steps(10);
    assert_eq!(h.state().documents().current(pane), Some(5));
    Ok(())
}

#[test]
fn dragging_past_the_bottom_scrolls_and_selects_into_the_next_page() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    let from = edge(&layer, sheet, find(&layer, "Sphinx")?, false)?;
    h.hover_at(from);
    h.step();
    button(&mut h, from, true);
    // Held below the window, the column scrolls on by itself.
    let below = pos2(from.x, 760.0);
    for _ in 0..90 {
        h.hover_at(below);
        h.step();
    }
    button(&mut h, below, false);
    h.run();
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let pages = docs.selection(pane).ok_or("nothing selected")?.pages();
    assert!(pages.start == 0 && pages.end >= 2, "{pages:?}");
    let text = copy(&mut h).ok_or("nothing copied")?;
    assert!(text.starts_with("Sphinx of black quartz"), "{text}");
    assert!(text.contains("\nPage 2 of 40\n"), "{text}");
    Ok(())
}
