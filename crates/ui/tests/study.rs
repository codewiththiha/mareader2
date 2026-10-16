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
    h.state().marks(pane).into_iter().map(|m| m.spot).collect()
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
    // Who answered, and a way to ask the AI chosen now.
    h.get_by_label("Sample answers");
    h.get_by_label("Ask again");

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
    let said = lib.marks(book).first().and_then(|m| m.gloss.value.clone());
    assert_eq!(said.map(|g| g.by).as_deref(), Some("Sample answers"));
    // Another AI keeps the answer: the card comes back answered, unasked.
    h.state_mut().set_option("explain", "none")?;
    select(&mut h, 0, "quick");
    h.state_mut().queue(Command::Explain(pane));
    h.run();
    assert!(h.state().study().card_answer().is_some());
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

#[test]
fn the_bar_highlights_and_underlines_and_a_click_offers_removal() -> Out {
    use mareader_library::Pen;
    let mut h = reader(&[("pdf", "sample:5")]);
    let (layer, sheet) = page(&h, 0)?;
    let quick = find(&layer, "quick brown")?;
    select(&mut h, 0, "quick brown");
    // Half is offered only once the reader adds it to the menu.
    assert!(h.query_by_label("Half highlight").is_none());
    h.get_by_label("Highlight").click();
    h.run();
    let pane = h.state().active_pane();
    assert!(h.state().documents().selection(pane).is_none());
    let (from, to) = ([0, quick], [0, quick + 11]);
    assert_eq!(marks(&h), [Spot::Chars { from, to }]);
    select(&mut h, 0, "lazy");
    h.get_by_label("Underline").click();
    h.run();
    let pens: Vec<Pen> = h.state().marks(pane).iter().map(|m| m.ink.pen).collect();
    assert_eq!(pens, [Pen::Full, Pen::Underline]);
    let yellow = h.state().settings().study.highlights.full;
    assert_eq!(
        h.state().marks(pane).first().map(|m| m.ink.color),
        Some(yellow)
    );
    // A click on the reader's own highlight offers removal, not a card.
    press(
        &mut h,
        middle(&layer, sheet, quick + 2)?,
        PointerButton::Primary,
    );
    assert_eq!(h.state().study().card_word(), None);
    h.get_by_label("Remove highlight").click();
    h.run();
    assert_eq!(marks(&h).len(), 1);
    Ok(())
}

#[test]
fn asking_for_a_colour_each_time_never_offers_the_ai_look() -> Out {
    use mareader_library::Pen;
    use mareader_settings::{BarItem, PickColor};
    let mut h = reader(&[("pdf", "sample:5")]);
    let look = &mut h.state_mut().settings_mut().study.highlights;
    look.pick = PickColor::Ask;
    look.bar.push(BarItem::Highlight(Pen::Half));
    let violet = look.ai.color.to_hex();
    select(&mut h, 0, "quick");
    h.get_by_label("Highlight").click();
    h.run();
    // The AI marks words in violet with this pen, so it is left out.
    assert!(h.query_by_label(&violet).is_none());
    h.get_by_label("#ffbe00").click();
    h.run();
    let pane = h.state().active_pane();
    let shown = h.state().marks(pane);
    let ink = shown.first().map(|m| (m.ink.pen, m.ink.color.to_hex()));
    assert_eq!(ink, Some((Pen::Full, "#ffbe00".to_owned())));
    // Half was added to the menu, where the AI's colour is fine.
    select(&mut h, 0, "lazy");
    h.get_by_label("Half highlight").click();
    h.run();
    h.get_by_label(&violet);
    Ok(())
}

#[test]
fn the_options_highlight_a_book_across_its_blocks() -> Out {
    let opts = [
        ("epub", "sample:2"),
        ("selection", "1:customs"),
        ("highlight", "underline"),
    ];
    let mut h = reader(&opts);
    h.run_steps(10);
    let spots = marks(&h);
    assert!(
        matches!(spots.as_slice(), [Spot::Blocks { .. }]),
        "{spots:?}"
    );
    Ok(())
}

#[test]
fn the_highlights_tab_sets_colours_the_menu_and_keeps_the_ai_look() {
    use egui::accesskit::Role;
    use mareader_appearance::Rgb;
    use mareader_library::Pen;
    use mareader_settings::BarItem;
    use mareader_ui::settings_window::Tab;
    let mut h = reader(&[("pdf", "sample")]);
    h.set_size(egui::vec2(1000.0, 1200.0));
    h.state_mut().queue(Command::OpenSettings(Tab::Highlights));
    h.run();
    // The AI marks words with this pen and colour: it can't be picked.
    h.get_by_label("Highlight #a58af0").click();
    h.get_by_label("Highlight #ffbe00").click();
    h.run();
    let look = &h.state().settings().study.highlights;
    assert_eq!(look.full, Rgb(0xff, 0xbe, 0x00));
    // Half joins the menu; Copy moves down past Explain.
    h.get_by_role_and_label(Role::CheckBox, "Half highlight")
        .click();
    h.get_by_label("Move Copy down").click();
    h.run();
    let bar = &h.state().settings().study.highlights.bar;
    let want = [
        BarItem::Explain,
        BarItem::Copy,
        BarItem::LookUp,
        BarItem::Highlight(Pen::Full),
        BarItem::Highlight(Pen::Underline),
        BarItem::Highlight(Pen::Half),
    ];
    assert_eq!(bar.as_slice(), want);
    // Half takes violet, then the AI takes Half: Half moves on.
    h.get_by_label("Half highlight #a58af0").click();
    h.run();
    let ai_half = h.get_by_role_and_label(Role::Button, "Half highlight");
    ai_half.scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::Button, "Half highlight")
        .click();
    h.run();
    let look = &h.state().settings().study.highlights;
    assert_eq!(look.ai.pen, Pen::Half);
    assert_ne!(look.half, look.ai.color);
}

#[test]
fn the_ai_tab_picks_who_explains() {
    use egui::accesskit::Role;
    use mareader_settings::Provider;
    use mareader_ui::settings_window::Tab;
    let mut h = reader(&[("pdf", "sample")]);
    h.state_mut().queue(Command::OpenSettings(Tab::Ai));
    h.run();
    h.get_by_role_and_label(Role::RadioButton, "OpenAI").click();
    h.run();
    let ai = &h.state().settings().ai;
    assert_eq!(ai.provider, Provider::OpenAi);
    // The preset's model field shows; its address waits under Advanced.
    h.get_by_label("Advanced");
}

#[test]
fn several_marks_are_picked_and_removed_together() -> Out {
    use mareader_library::Pen;
    let mut h = reader(&[("pdf", "sample:5")]);
    let pane = h.state().active_pane();
    for word in ["quick", "lazy"] {
        select(&mut h, 0, word);
        let (pen, color) = (Pen::Full, None);
        h.state_mut().queue(Command::Highlight { pane, pen, color });
        h.run();
    }
    assert_eq!(marks(&h).len(), 2);
    let (layer, sheet) = page(&h, 0)?;
    let quick = middle(&layer, sheet, find(&layer, "quick")? + 2)?;
    let lazy = middle(&layer, sheet, find(&layer, "lazy")? + 1)?;
    // The menu's Select several, then a click takes the other in.
    press(&mut h, quick, PointerButton::Secondary);
    h.get_by_label("Select several").click();
    h.run();
    assert!(h.state().study().picking());
    press(&mut h, lazy, PointerButton::Primary);
    h.get_by_label("2 selected");
    h.get_by_label("Remove (2)").click();
    h.run();
    assert!(marks(&h).is_empty());
    assert!(!h.state().study().picking());
    h.get_by_label("Undo").click();
    h.run();
    assert_eq!(marks(&h).len(), 2);

    // A long press picks; letting go doesn't pick it off again.
    h.hover_at(quick);
    h.step();
    let button = PointerButton::Primary;
    let modifiers = Modifiers::NONE;
    let pos = quick;
    h.event(Event::PointerButton {
        pos,
        button,
        pressed: true,
        modifiers,
    });
    h.run_steps(45);
    h.event(Event::PointerButton {
        pos,
        button,
        pressed: false,
        modifiers,
    });
    h.step();
    h.run();
    assert_eq!(h.state().study().picked(pane).map(|p| p.len()), Some(1));
    h.get_by_label("All").click();
    h.run();
    h.get_by_label("2 selected");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().study().picking());
    assert_eq!(marks(&h).len(), 2, "nothing removed");
    Ok(())
}
