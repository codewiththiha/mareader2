//! The page pass and the glass, rendered by a real wgpu device.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use image::RgbaImage;
use mareader_appearance::FilmGrain;
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
    // The title's glyph edges step sharply over any backdrop.
    let ink = |x: u32, y: u32| {
        [(x, y), (x + 1, y), (x, y + 1)]
            .iter()
            .any(|&(x, y)| luma(&clear, x, y) < 128)
    };
    for y in 0..bar - 1 {
        for x in 0..w - 1 {
            let d = diff(x, y);
            shows += usize::from(d > 6);
            if ink(x, y) {
                continue;
            }
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

#[test]
fn page_shadow_darkens_the_paper_under_a_page_until_switched_off() -> Out {
    // A margin leaves paper beside the page for its shadow.
    let on = [("pdf", "sample:3"), ("margin", "32")];
    let off = [("pdf", "sample:3"), ("margin", "32"), ("shadow", "false")];
    let mut shades = Vec::new();
    for options in [&on[..], &off[..]] {
        let mut h = reader(options);
        let pane = h.state().active_pane();
        let page = h.state().documents().sheets(pane).first().map(|s| s.rect);
        let page = page.ok_or("no page drawn")?;
        let image = h.render()?;
        // Just right of the page's edge, halfway down.
        let (x, y) = (page.right() as u32 + 2, page.center().y as u32);
        shades.push(luma(&image, x, y) - luma(&image, x + 20, y));
    }
    assert!(shades[0] < -2, "shadowed: {shades:?}");
    assert_eq!(shades[1], 0, "no shadow: {shades:?}");
    Ok(())
}

/// A margin pixel left of the text, on the first page.
const MARGIN_PX: (f32, f32) = (40.0, 300.0);

#[test]
fn the_page_pass_themes_pages_as_the_cpu_reference_does() -> Out {
    let looks = [("theme", "light"), ("theme", "dark"), ("theme", "dim")];
    let presets = [("preset", "sepia"), ("preset", "night")];
    for (key, value) in looks.into_iter().chain(presets) {
        let mut h = reader(&[("pdf", "sample:3"), (key, value)]);
        let image = h.render()?;
        let ppp = image.width() as f32 / SIZE.x;
        let (x, y) = (MARGIN_PX.0 * ppp, MARGIN_PX.1 * ppp);
        let got = image.get_pixel(x as u32, y as u32).0;
        let tone = h.state().settings().appearance.page_tone();
        let want = tone.blank().map(|c| (c * 255.0).round() as i32);
        let off = (0..3).map(|i| (i32::from(got[i]) - want[i]).abs()).max();
        assert!(off <= Some(3), "{value}: got {got:?}, want {want:?}");
    }
    Ok(())
}

#[test]
fn dark_pages_show_light_ink_on_dark_paper() -> Out {
    let mut h = reader(&[("pdf", "sample:3"), ("theme", "dark")]);
    let image = h.render()?;
    let (w, h) = image.dimensions();
    let paper = luma(&image, w / 20, h * 3 / 7);
    assert!(paper < 60, "the page went dark: {paper}");
    let ink = (h / 4..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| luma(&image, x, y) > paper + 120)
        .count();
    assert!(ink > 500, "{ink} light ink pixels");
    Ok(())
}

/// Darker runs down the margin column: each line's centre and darkest pixel.
fn margin_lines(image: &RgbaImage, ppp: f32, blank: i32) -> Vec<(f32, [u8; 4])> {
    let x = (MARGIN_PX.0 * ppp) as u32;
    let mut lines: Vec<(Vec<u32>, [u8; 4])> = Vec::new();
    let mut last = None;
    // Clear of the gutter's shadow on top and the progress strip below.
    let bottom = image.height().saturating_sub((12.0 * ppp) as u32);
    for y in (24.0 * ppp) as u32..bottom {
        if luma(image, x, y) >= blank - 4 {
            continue;
        }
        let px = image.get_pixel(x, y).0;
        match lines.last_mut() {
            Some((rows, darkest)) if last == Some(y - 1) => {
                rows.push(y);
                let [r, g, b, _] = *darkest;
                let old = (i32::from(r) * 2126 + i32::from(g) * 7152 + i32::from(b) * 722) / 10_000;
                if luma(image, x, y) < old {
                    *darkest = px;
                }
            }
            _ => lines.push((vec![y], px)),
        }
        last = Some(y);
    }
    let centre = |rows: &[u32]| rows.iter().sum::<u32>() as f32 / rows.len().max(1) as f32;
    lines.iter().map(|(rows, d)| (centre(rows), *d)).collect()
}

fn median_gap(lines: &[(f32, [u8; 4])]) -> f32 {
    let mut gaps: Vec<f32> = lines.windows(2).map(|w| w[1].0 - w[0].0).collect();
    gaps.sort_by(f32::total_cmp);
    gaps.get(gaps.len() / 2).copied().unwrap_or(0.0)
}

#[test]
fn lined_paper_rules_the_page_in_page_units() -> Out {
    let mut h = reader(&[("pdf", "sample:3"), ("texture", "lined")]);
    let image = h.render()?;
    let ppp = image.width() as f32 / SIZE.x;
    let tone = h.state().settings().appearance.page_tone();
    let p = tone.pattern;
    let to_u8 = |c: [f32; 3]| c.map(|c| (c * 255.0).round() as i32);
    let blank = to_u8(tone.blank());
    let lines = margin_lines(&image, ppp, blank[1]);
    assert!(lines.len() >= 8, "{} lines", lines.len());
    let gap = median_gap(&lines);
    for w in lines.windows(2) {
        let d = w[1].0 - w[0].0;
        assert!((d - gap).abs() <= 1.5, "uneven rules: {d} against {gap}");
    }
    // A fully covered pixel is the reference's stroke over the paper.
    let want = to_u8(p.over(tone.blank(), p.stroke));
    let off = |px: [u8; 4]| (0..3).map(|i| (i32::from(px[i]) - want[i]).abs()).max();
    let best = lines.iter().map(|l| off(l.1)).min().flatten();
    assert!(best <= Some(3), "darkest rule {best:?} from {want:?}");
    // Twice the scale, twice the pitch: the rules follow the page.
    let mut wide = reader(&[
        ("pdf", "sample:3"),
        ("texture", "lined"),
        ("texture_scale", "200"),
    ]);
    let image = wide.render()?;
    let wider = median_gap(&margin_lines(&image, ppp, blank[1]));
    assert!(
        (wider - 2.0 * gap).abs() <= 2.0,
        "{wider} against twice {gap}"
    );
    Ok(())
}

#[test]
fn real_paper_grains_the_page_around_the_reference() -> Out {
    let mut h = reader(&[("pdf", "sample:3"), ("texture", "paper")]);
    let image = h.render()?;
    let ppp = image.width() as f32 / SIZE.x;
    let p = h.state().settings().appearance.page_tone();
    let mean = p.pattern.over(p.blank(), p.pattern.grain_layer(0.5, 0.5));
    let want = (mean[1] * 255.0).round() as f64;
    let (x0, y0) = ((20.0 * ppp) as u32, (200.0 * ppp) as u32);
    let block: Vec<f64> = (y0..y0 + 60)
        .flat_map(|y| (x0..x0 + 40).map(move |x| (x, y)))
        .map(|(x, y)| f64::from(image.get_pixel(x, y).0[1]))
        .collect();
    let n = block.len() as f64;
    let avg = block.iter().sum::<f64>() / n;
    let spread = (block.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / n).sqrt();
    assert!(
        (avg - want).abs() <= 3.0,
        "grain mean {avg}, reference {want}"
    );
    assert!(spread > 0.5, "grain varies: {spread}");
    Ok(())
}

#[test]
fn rules_run_on_across_touching_pages() -> Out {
    // 792 pt is no whole number of rules, so per-page rules would jar.
    let opts = [("pdf", "sample:3"), ("texture", "lined"), ("gap", "false")];
    let mut h = reader(&opts);
    // Page one is about 1164 points tall: bring its seam on screen.
    scroll(&mut h, 900.0);
    let image = h.render()?;
    let ppp = image.width() as f32 / SIZE.x;
    let tone = h.state().settings().appearance.page_tone();
    let blank = (tone.blank()[1] * 255.0).round() as i32;
    let lines = margin_lines(&image, ppp, blank);
    assert!(lines.len() >= 12, "{} lines", lines.len());
    let gap = median_gap(&lines);
    for w in lines.windows(2) {
        let d = w[1].0 - w[0].0;
        assert!(
            (d - gap).abs() <= 1.5,
            "a jarred rule at {}: {d} against {gap}",
            w[1].0
        );
    }
    Ok(())
}

/// A pixel of `plain`, the frame without grain, grained by the reference.
fn grained_by_reference(plain: &RgbaImage, grain: FilmGrain, ppp: f32, x: u32, y: u32) -> [u8; 3] {
    let [r, g, b, _] = plain.get_pixel(x, y).0;
    let base = [r, g, b].map(|c| f32::from(c) / 255.0);
    let p = [(x as f32 + 0.5) / ppp, (y as f32 + 0.5) / ppp];
    grain.over(base, p).map(|c| (c * 255.0).round() as u8)
}

#[test]
fn film_grain_multiplies_light_and_screens_dark_as_the_reference() -> Out {
    for theme in ["light", "dark"] {
        let mut h = reader(&[("theme", theme)]);
        let plain = h.render()?;
        h.state_mut().set_option("grain", "static")?;
        h.state_mut().set_option("grain_intensity", "100")?;
        h.run_steps(2);
        let grained = h.render()?;
        let look = h.state().settings().appearance;
        let grain = FilmGrain::of(&look).ok_or("grain is on")?;
        assert_eq!(grain.screen, theme == "dark");
        let ppp = plain.width() as f32 / SIZE.x;
        // Plain paper below the empty state's prompt.
        let (x0, y0) = ((780.0 * ppp) as u32, (560.0 * ppp) as u32);
        let (mut worst, mut values) = (0, Vec::new());
        for y in y0..y0 + 40 {
            for x in x0..x0 + 40 {
                let want = grained_by_reference(&plain, grain, ppp, x, y);
                let [r, g, b, _] = grained.get_pixel(x, y).0;
                for (got, want) in [r, g, b].into_iter().zip(want) {
                    worst = worst.max((i32::from(got) - i32::from(want)).abs());
                }
                values.push(f64::from(g));
            }
        }
        let n = values.len() as f64;
        let avg = values.iter().sum::<f64>() / n;
        let spread = (values.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / n).sqrt();
        assert!(worst <= 3, "{theme}: off the reference by {worst}");
        assert!(spread > 3.0, "{theme}: the grain varies, spread {spread}");
        let before = f64::from(plain.get_pixel(x0, y0).0[1]);
        let darker = avg < before;
        assert_eq!(darker, theme == "light", "{theme}: {before} became {avg}");
        h.state_mut().set_option("grain", "off")?;
        h.run_steps(2);
        let off = h.render()?;
        let same = (y0..y0 + 40)
            .all(|y| (x0..x0 + 40).all(|x| off.get_pixel(x, y) == plain.get_pixel(x, y)));
        assert!(same, "{theme}: grain off leaves the paper untouched");
    }
    Ok(())
}

/// How much the green channel varies over a card's plain paper.
fn card_spread(image: &RgbaImage, card: egui::Rect, ppp: f32) -> f64 {
    // Left of the lines, clear of the corner dot.
    let w = card.width();
    let x = (card.left() + w * 0.05) * ppp..(card.left() + w * 0.16) * ppp;
    let y = (card.top() + w * 0.2) * ppp..(card.top() + w * 0.55) * ppp;
    let mut values = Vec::new();
    for py in y.start as u32..y.end as u32 {
        for px in x.start as u32..x.end as u32 {
            values.push(f64::from(image.get_pixel(px, py).0[1]));
        }
    }
    let n = values.len().max(1) as f64;
    let avg = values.iter().sum::<f64>() / n;
    (values.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / n).sqrt()
}

#[test]
fn preset_cards_carry_their_own_texture_and_grain() -> Out {
    let loud = [
        ("texture", "grid"),
        ("grain", "static"),
        ("grain_intensity", "100"),
        ("save_preset", "Loud"),
        ("preset", "sepia"),
        ("open", "appearance"),
    ];
    let mut h = reader(&loud);
    h.run_steps(4);
    let image = h.render()?;
    let ppp = image.width() as f32 / SIZE.x;
    let loud = card_spread(&image, h.get_by_label("Loud").rect(), ppp);
    let plain = card_spread(&image, h.get_by_label("Sepia").rect(), ppp);
    assert!(plain < 1.5, "Sepia's card is flat paper: {plain}");
    assert!(loud > 4.0, "Loud's card shows its grain: {loud}");
    Ok(())
}

#[test]
fn split_panes_fill_with_their_own_paper() -> Out {
    let mut h = reader(&[("panes", "2"), ("independent", "true")]);
    h.run_steps(2);
    let image = h.render()?;
    let ppp = image.width() as f32 / SIZE.x;
    let app = h.state();
    let mut papers = Vec::new();
    // Below each pane's empty state, clear of outlines and dividers.
    for (pane, x) in app.panes().into_iter().zip([0.25, 0.75]) {
        let want = mareader_ui::theme::Tokens::new(&app.look(pane)).paper;
        let at = ((SIZE.x * x * ppp) as u32, (SIZE.y * 0.9 * ppp) as u32);
        let [r, g, b, _] = image.get_pixel(at.0, at.1).0;
        let off = [(r, want.r()), (g, want.g()), (b, want.b())]
            .map(|(got, want)| (i32::from(got) - i32::from(want)).abs());
        assert!(
            off.iter().all(|d| *d <= 2),
            "{pane:?}: {r} {g} {b} vs {want:?}"
        );
        papers.push(want);
    }
    assert_ne!(papers.first(), papers.last(), "each pane its own paper");
    Ok(())
}
