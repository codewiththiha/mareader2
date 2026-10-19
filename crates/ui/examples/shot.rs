//! `shot OUT.png [key=value ...]`: renders the app headlessly to look at.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use mareader_cefr::{Part, State};
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::layout::{Arrow, ViewMode};
use mareader_ui::vocab::Action;
use mareader_ui::zoom::{Fit, Step, ZoomCommand};

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let out = args.next().ok_or("usage: shot OUT.png [key=value ...]")?;
    let mut app = MareaderApp::default();
    let mut hover = None;
    let mut size = egui::vec2(1200.0, 800.0);
    let mut zooms = Vec::new();
    let mut queued = Vec::new();
    let mut scroll = 0.0;
    let mut drag = None;
    let mut carry = None;
    let mut words = false;
    let mut searched = false;
    let mut tabs = 0;
    let mut right = None;
    let mut wait = 0;
    for kv in args {
        let (k, v) = kv
            .split_once('=')
            .ok_or(format!("expected key=value, got {kv}"))?;
        match k {
            "hover" => {
                let (x, y) = v.split_once(',').ok_or("hover=x,y")?;
                hover = Some(egui::pos2(
                    x.parse().map_err(|_| "hover x")?,
                    y.parse().map_err(|_| "hover y")?,
                ));
            }
            // A press, move and release once open: `drag=x1,y1,x2,y2`.
            "drag" => {
                let n: Vec<f32> = v.split(',').filter_map(|n| n.parse().ok()).collect();
                let [x1, y1, x2, y2] = n.as_slice() else {
                    return Err("drag=x1,y1,x2,y2".into());
                };
                drag = Some((egui::pos2(*x1, *y1), egui::pos2(*x2, *y2)));
            }
            // A drag held, not let go: `carry=x1,y1,x2,y2`.
            "carry" => {
                let n: Vec<f32> = v.split(',').filter_map(|n| n.parse().ok()).collect();
                let [x1, y1, x2, y2] = n.as_slice() else {
                    return Err("carry=x1,y1,x2,y2".into());
                };
                carry = Some((egui::pos2(*x1, *y1), egui::pos2(*x2, *y2)));
            }
            // Applied once the document is open: `zoom=in,in,page`.
            "zoom" => {
                for z in v.split(',') {
                    zooms.push(match z {
                        "in" => ZoomCommand::Step(Step::In),
                        "out" => ZoomCommand::Step(Step::Out),
                        "width" => ZoomCommand::Fit(Fit::Width),
                        "page" => ZoomCommand::Fit(Fit::Page),
                        _ => return Err(format!("zoom: in, out, width or page, not {z}")),
                    });
                }
            }
            // Points to wheel the document down once open.
            "scroll" => scroll = v.parse().map_err(|_| "scroll=points")?,
            // Also once open: `view=spread arrows=right,right`.
            "view" => queued.push(Command::View(match v {
                "single" => ViewMode::Single,
                "spread" => ViewMode::Spread,
                "vertical" => ViewMode::Vertical,
                "horizontal" => ViewMode::Horizontal,
                _ => {
                    return Err(format!(
                        "view: single, spread, vertical or horizontal, not {v}"
                    ));
                }
            })),
            "arrows" => {
                for a in v.split(',') {
                    queued.push(Command::Arrow(match a {
                        "left" => Arrow::Left,
                        "right" => Arrow::Right,
                        "up" => Arrow::Up,
                        "down" => Arrow::Down,
                        _ => return Err(format!("arrows: left, right, up or down, not {a}")),
                    }));
                }
            }
            // 1-based, like the page entry: `goto=12`.
            "goto" => {
                let page: usize = v.parse().map_err(|_| "goto=page")?;
                queued.push(Command::GoTo {
                    pane: app.active_pane(),
                    page: page.saturating_sub(1),
                });
            }
            // The find bar, once open: `find=true`.
            "find" if v == "true" => queued.push(Command::ShowFind),
            // Searched, on the first hit from the page read: `find=fox`.
            "find" => {
                app.set_option(k, v)?;
                queued.push(Command::ShowFind);
                searched = true;
            }
            // The real word levels, fetched into a temporary folder.
            "cefr" if v == "download" => {
                let dir = std::env::temp_dir().join("mareader-shot-cefr");
                app.set_option(k, &dir.to_string_lossy())?;
                words = true;
            }
            // The test packs, converted: `dictionaries=fixtures`.
            "dictionaries" if v == "fixtures" => {
                let dir = fixture_packs()?;
                app.set_option(k, &dir.to_string_lossy())?;
            }
            // Tab presses once open, so the focus ring shows: `tab=3`.
            "tab" => tabs = v.parse().map_err(|_| "tab=count")?,
            // A right-click once open, so its menu shows: `right=x,y`.
            "right" => {
                let (x, y) = v.split_once(',').ok_or("right=x,y")?;
                right = Some(egui::pos2(
                    x.parse().map_err(|_| "right x")?,
                    y.parse().map_err(|_| "right y")?,
                ));
            }
            // Frames to run before the shot, at 60 a second: `wait=90`.
            "wait" => wait = v.parse().map_err(|_| "wait=frames")?,
            "size" => {
                let (w, h) = v.split_once('x').ok_or("size=WxH")?;
                size = egui::vec2(
                    w.parse().map_err(|_| "width")?,
                    h.parse().map_err(|_| "height")?,
                );
            }
            _ => app.set_option(k, v)?,
        }
    }
    let mut harness = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .with_max_steps(64)
        .build_eframe(|_| app);
    harness.run_steps(4);
    if words {
        harness.state_mut().queue(Command::Vocab(Action::Download));
        wait_for_words(&mut harness)?;
    }
    for c in queued {
        harness.state_mut().queue(c);
        harness.run_steps(8);
    }
    // The hit is revealed, gliding there when it's pages away.
    if searched {
        harness.run_steps(60);
    }
    for z in zooms {
        harness.state_mut().queue(Command::Zoom(z));
        harness.run_steps(16);
    }
    if scroll != 0.0 {
        harness.hover_at((size / 2.0).to_pos2());
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -scroll),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(40);
    }
    let held = carry.map(|c| (c, false));
    if let Some(((from, to), release)) = drag.map(|d| (d, true)).or(held) {
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        harness.hover_at(from);
        harness.event(press(from, true));
        harness.run_steps(2);
        for k in 1..=8 {
            harness.hover_at(from + (to - from) * (k as f32 / 8.0));
            harness.run_steps(1);
        }
        if release {
            harness.event(press(to, false));
        }
        // A held carry rests, so the grid's rests take, opening included.
        harness.run_steps(if release { 4 } else { 50 });
    }
    if let Some(pos) = right {
        harness.hover_at(pos);
        harness.run_steps(1);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run_steps(1);
        }
        harness.run_steps(4);
    }
    harness.run_steps(wait);
    for _ in 0..tabs {
        harness.key_press(egui::Key::Tab);
        harness.run_steps(4);
    }
    // Long enough for a tooltip to appear.
    if let Some(p) = hover {
        harness.hover_at(p);
        harness.run_steps(45);
    }
    let image = harness.render()?;
    image.save(&out).map_err(|e| e.to_string())?;
    println!("{out}");
    Ok(())
}

/// The Burmese and French test packs, built into a temporary folder.
fn fixture_packs() -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join("mareader-shot-packs");
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dict/tests/fixtures");
    for (file, id) in [
        ("en-my.parquet", "mcfnlp-en-my"),
        ("en-fr.parquet", "wiktionary-en-fr"),
    ] {
        let pack = (mareader_dict::PACKS.iter())
            .find(|p| p.id == id)
            .ok_or(format!("no pack {id}"))?;
        let db = mareader_dict::db_file(&dir, pack);
        std::fs::create_dir_all(db.parent().unwrap_or(&dir)).map_err(|e| e.to_string())?;
        mareader_dict::build(&fixtures.join(file), &db).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

/// Steps until the word levels are in, for two minutes at most.
fn wait_for_words(harness: &mut Harness<'_, MareaderApp>) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        harness.run_steps(1);
        match harness.state().vocab().state(Part::Dataset) {
            State::Ready => break,
            State::Failed(why) => return Err(format!("word levels: {why}")),
            _ if Instant::now() > until => return Err("word levels: timed out".into()),
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    // Pages ask for their levels, then repaint shaded.
    harness.run_steps(12);
    Ok(())
}
