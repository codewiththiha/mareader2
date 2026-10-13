//! `shot OUT.png [key=value ...]`: renders the app headlessly to look at.

use egui_kittest::Harness;
use mareader_ui::MareaderApp;

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let out = args.next().ok_or("usage: shot OUT.png [key=value ...]")?;
    let mut app = MareaderApp::default();
    let mut hover = None;
    let mut size = egui::vec2(1200.0, 800.0);
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
        .with_max_steps(64)
        .build_eframe(|_| app);
    harness.run_steps(4);
    if let Some(p) = hover {
        harness.hover_at(p);
        harness.run_steps(30);
    }
    let image = harness.render()?;
    image.save(&out).map_err(|e| e.to_string())?;
    println!("{out}");
    Ok(())
}
