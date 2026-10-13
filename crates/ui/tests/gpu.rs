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
