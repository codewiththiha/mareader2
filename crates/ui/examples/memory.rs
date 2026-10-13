//! `memory BOOK.pdf [ROUNDS]`: what reading costs, what closing returns.

// Linux only: it reads `/proc/self/status`. See `docs/memory.md`.

use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use mareader_ui::command::Command;
use mareader_ui::{MareaderApp, OpenRequest};
use mareader_workspace::{PaneId, SplitAxis};

type App = Harness<'static, MareaderApp>;

/// A `/proc/self/status` field, in MiB.
fn status(field: &str) -> Option<f64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = text.lines().find(|l| l.starts_with(field))?;
    let kib: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib / 1024.0)
}

/// Forgets the peak so far, so the next one is the next step's own.
fn reset_peak() {
    std::fs::write("/proc/self/clear_refs", "5").ok();
}

fn row(h: &mut App, step: &str) {
    h.render().ok();
    let m = h.state().documents().meter();
    let textures = h.ctx.tex_manager().read().num_allocated();
    let rss = status("VmRSS:").unwrap_or(0.0);
    let peak = status("VmHWM:").unwrap_or(0.0);
    let pages = m.page_bytes as f64 / (1024.0 * 1024.0);
    println!(
        "| {step} | {rss:.0} | {peak:.0} | {} | {} ({pages:.0} MiB) | {} | {} | {textures} |",
        m.documents, m.pages, m.texts, m.jobs,
    );
    reset_peak();
}

fn open(h: &mut App, pane: PaneId, path: &Path) {
    let request = OpenRequest {
        pane: Some(pane),
        ..OpenRequest::path(path.to_path_buf())
    };
    h.state_mut().queue(Command::Open(request));
    h.run();
}

/// Every page in turn, each landing at once and drawn.
fn read_through(h: &mut App, pane: PaneId) {
    let count = h.state().documents().page_count(pane).unwrap_or(0);
    for page in 0..count {
        h.state_mut().queue(Command::GoTo { pane, page });
        h.run();
        h.render().ok();
    }
}

fn main() -> Result<(), String> {
    let path: PathBuf = std::env::args()
        .nth(1)
        .ok_or("usage: memory BOOK.pdf [ROUNDS]")?
        .into();
    let mut app = MareaderApp::default();
    app.set_option("workers", "0")?;
    app.set_option("panes", "2")?;
    let mut s = app.settings().clone();
    // Jumps land at once: the read-through is about pages, not glides.
    s.motion.scroll_jumps = false;
    app.queue(Command::SetSettings(Box::new(s)));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .wgpu()
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    println!("| Step | RSS MiB | Peak MiB | Docs | Pages | Text layers | Jobs | egui textures |");
    println!("|---|---|---|---|---|---|---|---|");
    row(&mut h, "idle, two empty panes");
    let panes = h.state().panes();
    let (left, right) = (panes[0], panes[1]);
    open(&mut h, right, &path);
    row(&mut h, "opened in one pane");
    read_through(&mut h, right);
    row(&mut h, "every page read");
    h.state_mut().queue(Command::ClosePane(right));
    h.run();
    row(&mut h, "its pane closed");
    // A leak grows with each round; memory kept for reuse levels off.
    let rounds = std::env::args().nth(2).and_then(|n| n.parse().ok());
    for round in 2..=rounds.unwrap_or(4) {
        h.state_mut().queue(Command::Split(SplitAxis::Horizontal));
        h.run();
        let pane = h.state().active_pane();
        open(&mut h, pane, &path);
        read_through(&mut h, pane);
        h.state_mut().queue(Command::ClosePane(pane));
        h.run();
        row(&mut h, &format!("round {round}: read and closed"));
    }
    // The same book twice: the second pane should cost nearly nothing.
    h.state_mut().queue(Command::FocusPane(left));
    h.state_mut().queue(Command::Split(SplitAxis::Horizontal));
    h.run();
    let panes = h.state().panes();
    open(&mut h, panes[0], &path);
    row(&mut h, "opened again");
    open(&mut h, panes[1], &path);
    row(&mut h, "and in the second pane");
    Ok(())
}
