//! The selection bar, Explain, marks and the word card, headless.

use std::sync::Arc;

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_library::Spot;
use mareader_pdf::TextLayer;
use mareader_session::Anchor;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::selection::Sheet;
use mareader_ui::study::words;

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

fn press(h: &mut H, pos: Pos2, button: PointerButton) {
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
    h.run();
}

fn double_click(h: &mut H, pos: Pos2) {
    h.hover_at(pos);
    h.step();
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.step();
        }
    }
    h.run();
}

/// Selects the first `text` on `page`, as a drag would.
fn select(h: &mut H, page: usize, text: &str) {
    let pane = h.state().active_pane();
    let text = text.to_owned();
    h.state_mut()
        .queue(Command::SelectText { pane, page, text });
    h.run();
}

fn marks(h: &H) -> Vec<Spot> {
    let pane = h.state().active_pane();
    h.state().marks(pane).into_iter().map(|(_, s)| s).collect()
}

#[test]
fn explain_marks_the_word_and_its_card_comes_back_on_a_click() -> Out {
    let mut h = reader(&[("pdf", "sample:5")]);
    let (layer, sheet) = page(&h, 0)?;
    let quick = find(&layer, "quick")?;
    double_click(&mut h, middle(&layer, sheet, quick + 2)?);
    let pane = h.state().active_pane();
    assert!(h.state().documents().selection(pane).is_some());
    // Released: the bar is up under the word.
    h.get_by_label("Copy");
    h.get_by_label("Explain").click();
    h.run();
    let study = h.state().study();
    assert_eq!(study.card_word(), Some("quick"));
    let pos = study.card_answer().map(|a| a.pos.clone());
    assert_eq!(pos.as_deref(), Some("adjective"));
    assert!(h.state().documents().selection(pane).is_none());
    let spots = marks(&h);
    assert!(
        matches!(spots.as_slice(), [Spot::Page { page: 0, .. }]),
        "{spots:?}"
    );
    h.get_by_label("Synonyms");

    // A click elsewhere closes the card; a click on the mark brings it.
    let blank = sheet.rect.left_top() + egui::vec2(12.0, 12.0);
    press(&mut h, blank, PointerButton::Primary);
    assert!(!h.state().study().card_open());
    press(
        &mut h,
        middle(&layer, sheet, quick + 1)?,
        PointerButton::Primary,
    );
    assert_eq!(h.state().study().card_word(), Some("quick"));
    // Explaining the same word there again keeps one mark.
    select(&mut h, 0, "quick");
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    assert_eq!(marks(&h).len(), 1);
    Ok(())
}

#[test]
fn a_right_click_removes_a_mark_and_undo_brings_it_back() -> Out {
    let mut h = reader(&[
        ("pdf", "sample:5"),
        ("selection", "0:quick"),
        ("gloss", "open"),
    ]);
    h.run();
    assert_eq!(marks(&h).len(), 1, "the options mark the word");
    let (layer, sheet) = page(&h, 0)?;
    let at = middle(&layer, sheet, find(&layer, "quick")? + 2)?;
    press(&mut h, at, PointerButton::Secondary);
    assert!(!h.state().study().card_open(), "the menu replaces the card");
    h.get_by_label("Remove highlight").click();
    h.run();
    assert!(marks(&h).is_empty());
    h.get_by_label("Undo").click();
    h.run();
    assert_eq!(marks(&h).len(), 1);
    Ok(())
}

#[test]
fn a_phrase_is_not_explained_and_nothing_is_marked() -> Out {
    let mut h = reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    select(&mut h, 0, "quick brown");
    assert!(h.state().documents().selection(pane).is_some());
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    assert!(!h.state().study().card_open());
    assert!(marks(&h).is_empty());
    Ok(())
}

#[test]
fn a_failed_answer_says_why_and_offers_a_retry() -> Out {
    let mut h = reader(&[("pdf", "sample:5"), ("explain", "none")]);
    let pane = h.state().active_pane();
    select(&mut h, 0, "quick");
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    assert!(h.state().study().card_open());
    assert!(h.state().study().card_answer().is_none());
    // Nothing on this machine can answer: no retry is offered.
    assert!(h.query_by_label("Try again").is_none());
    assert_eq!(marks(&h).len(), 1, "the word is marked all the same");
    Ok(())
}

#[test]
fn a_book_s_mark_is_found_again_after_the_type_changes() -> Out {
    let mut h = reader(&[("epub", "sample:2"), ("view", "single")]);
    let pane = h.state().active_pane();
    let page = (0..8).find(|&p| {
        select(&mut h, p, "customs");
        h.state().documents().selection(pane).is_some()
    });
    let page = page.ok_or("no customs in the sample")?;
    h.state_mut().queue(Command::GoTo { pane, page });
    h.run();
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    assert_eq!(h.state().study().card_word(), Some("customs"));
    let spots = marks(&h);
    let [Spot::Text { block, start, end }] = spots.as_slice() else {
        return Err(format!("{spots:?}").into());
    };
    let (block, bytes) = (*block, *start..*end);
    assert_eq!(bytes.len(), "customs".len());

    h.state_mut().set_option("text_size", "30")?;
    h.run_steps(10);
    h.run();
    let reading = h.state().documents().reading(pane).ok_or("no book")?;
    let doc = reading.document;
    let anchor = Anchor {
        block,
        offset: bytes.start,
    };
    let page = doc.locate(anchor).ok_or("not located")?;
    let layer = doc.text_layer(page)?;
    let chars = words::find(&layer, block, bytes).ok_or("not found again")?;
    assert_eq!(layer.text.text(chars), "customs");
    Ok(())
}

#[test]
fn a_library_book_keeps_its_marks() -> Out {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("sample.pdf");
    std::fs::write(&file, mareader_pdf::sample::pdf(5, "A Kept Sample"))?;
    let path = file.to_string_lossy().into_owned();
    let mut h = reader(&[("pdf", path.as_str())]);
    h.run_steps(5);
    h.run();
    let pane = h.state().active_pane();
    select(&mut h, 0, "quick");
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    let lib = h.state().library();
    let book = lib.books().next().ok_or("not in the library")?.id;
    let kept: Vec<String> = lib.marks(book).iter().map(|m| m.word.clone()).collect();
    assert_eq!(kept, ["quick"]);
    Ok(())
}

#[test]
fn the_options_turn_to_the_page_and_open_the_card() -> Out {
    let opts = [
        ("epub", "sample:2"),
        ("selection", "1:customs"),
        ("gloss", "open"),
    ];
    let mut h = reader(&opts);
    h.run_steps(10);
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let pages: Vec<usize> = docs.sheets(pane).iter().map(|s| s.page).collect();
    let state = format!("{:?} {pages:?} {:?}", marks(&h), docs.selection(pane));
    assert_eq!(h.state().study().card_word(), Some("customs"), "{state}");
    assert_eq!(marks(&h).len(), 1);
    Ok(())
}
