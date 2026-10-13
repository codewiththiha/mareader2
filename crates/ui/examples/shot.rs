//! `shot OUT.png [key=value ...]`: renders the app headlessly to look at.

use egui_kittest::Harness;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::layout::{Arrow, ViewMode};
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
    for c in queued {
        harness.state_mut().queue(c);
        harness.run_steps(8);
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
    if let Some(p) = hover {
        harness.hover_at(p);
        harness.run_steps(30);
    }
    let image = harness.render()?;
    image.save(&out).map_err(|e| e.to_string())?;
    println!("{out}");
    Ok(())
}
