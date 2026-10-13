//! The page pass and the glass, rendered by a real wgpu device.

use egui_kittest::Harness;
use image::RgbaImage;
use mareader_ui::MareaderApp;
use mareader_ui::theme::TITLE_BAR_H;

type Out = Result<(), String>;

const SIZE: egui::Vec2 = egui::vec2(900.0, 700.0);

fn reader(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .wgpu()
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn scroll(h: &mut Harness<'static, MareaderApp>, points: f32) {
    h.hover_at((SIZE / 2.0).to_pos2());
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -points),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run();
    // Pages scrolled in are drawn next frame.
    h.run_steps(2);
}

fn luma(image: &RgbaImage, x: u32, y: u32) -> i32 {
    let [r, g, b, _] = image.get_pixel(x, y).0;
    (i32::from(r) * 2126 + i32::from(g) * 7152 + i32::from(b) * 722) / 10_000
}

#[test]
fn pages_reach_the_screen_through_the_page_pass() -> Out {
    let mut h = reader(&[("pdf", "sample")]);
    assert!(h.state().documents().gpu().is_some(), "a device was found");
    let (_, pages) = h.state().documents().stats();
    assert!(pages > 0, "pages were uploaded");
    // Ours, not egui's: only fonts and icons live in its manager.
    let egui_textures = h.ctx.tex_manager().read().num_allocated();
    assert!(egui_textures < pages + 8, "{egui_textures} egui textures");
    let image = h.render()?;
    let (w, h) = image.dimensions();
    let ink = (h / 4..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| luma(&image, x, y) < 60)
        .count();
    assert!(ink > 500, "{ink} inked pixels");
    Ok(())
}

#[test]
fn a_pinned_bar_blurs_the_page_scrolled_under_it() -> Out {
    let options = [("pdf", "sample"), ("pinned", "true")];
    let mut h = reader(&options);
    let clear = h.render()?;
    scroll(&mut h, 200.0);
    let under = h.render()?;
    let (w, bar) = (clear.width(), TITLE_BAR_H as u32);
    // Icons and the title match in both; only the backdrop differs.
    let diff = |x: u32, y: u32| luma(&clear, x, y) - luma(&under, x, y);
    let mut shows = 0;
    let mut sharpest = 0;
    for y in 0..bar - 1 {
        for x in 0..w - 1 {
            let d = diff(x, y);
            shows += usize::from(d > 6);
            let step = (diff(x + 1, y) - d).abs().max((diff(x, y + 1) - d).abs());
            sharpest = sharpest.max(step);
        }
    }
    assert!(shows > 200, "the heading shows through: {shows} pixels");
    // Blurred it steps by about 1; merely shrunk, by about 13.
    assert!(sharpest < 6, "the backdrop is blurred: step {sharpest}");
    Ok(())
}

/// Rows of a band right of `left`: brightness spread and sharpest step.
fn band(image: &RgbaImage, rows: std::ops::Range<u32>, left: u32) -> (i32, i32) {
    let (mut lo, mut hi, mut step) = (i32::MAX, i32::MIN, 0);
    for y in rows {
        for x in left..image.width() - 1 {
            let l = luma(image, x, y);
            (lo, hi) = (lo.min(l), hi.max(l));
            step = step.max((luma(image, x + 1, y) - l).abs());
        }
    }
    (hi - lo, step)
}

#[test]
fn the_page_bar_is_glass_and_both_bars_turn_solid_without_it() -> Out {
    let mut h = reader(&[("pdf", "sample"), ("pinned", "true")]);
    // Brings a line of page 2 up under the bar's top edge.
    let line = {
        let docs = h.state().documents();
        let pane = h.state().active_pane();
        let reading = docs.reading(pane).ok_or("no document")?;
        let layer = reading.document.text_layer(1).map_err(|e| e.to_string())?;
        // Page 2 lies a page and a gap below page 1, off screen.
        let mut sheet = *docs.sheets(pane).first().ok_or("no page")?;
        let pitch = sheet.rect.height() + mareader_ui::layout::GAP;
        sheet.rect = sheet.rect.translate(egui::vec2(0.0, pitch));
        let text: String = layer.text.chars().iter().map(|c| c.ch).collect();
        let at = text.find("Sphinx").ok_or("no line")?;
        let line = layer.text.line_at(at);
        let first = layer.text.chars().get(line.start).ok_or("empty")?;
        sheet.to_screen(first.bounds)
    };
    scroll(&mut h, line.center().y - (SIZE.y - 35.0));
    // Pointing at the pane's foot raises its bar.
    h.hover_at(egui::pos2(SIZE.x / 2.0, SIZE.y - 3.0));
    h.run_steps(30);
    let glass = h.render()?;
    // Near the bar's top edge, past the slider's knob on page 1.
    let (top, bottom) = (SIZE.y as u32 - 38, SIZE.y as u32 - 32);
    let (spread, step) = band(&glass, top..bottom, 260);
    assert!(spread > 4, "page and margin show through: {spread}");
    assert!(step < 6, "and blurred: step {step}");
    h.state_mut().set_option("glass", "false")?;
    h.run_steps(4);
    let solid = h.render()?;
    assert_eq!(
        band(&solid, top..bottom, 260).0,
        0,
        "the page bar is one colour"
    );
    let title = band(&solid, 2..8, 0);
    assert_eq!(title.0, 0, "so is the title bar's edge");
    Ok(())
}
