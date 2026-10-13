//! Opening and scrolling PDFs, headless, with jobs run between frames.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_ui::command::Command;
use mareader_ui::{MareaderApp, OpenRequest};

fn reader(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn textures(h: &Harness<'static, MareaderApp>) -> usize {
    h.ctx.tex_manager().read().num_allocated()
}

fn visible(h: &Harness<'static, MareaderApp>) -> std::ops::Range<usize> {
    let pane = h.state().active_pane();
    h.state().documents().visible(pane).unwrap_or(0..0)
}

#[test]
fn the_sample_opens_titled_with_its_first_page_drawn() {
    let h = reader(&[("pdf", "sample:500")]);
    assert_eq!(h.state().title(), "A Mareader Sample");
    let pane = h.state().active_pane();
    assert_eq!(h.state().documents().page_count(pane), Some(500));
    assert_eq!(visible(&h).start, 0);
    // What is on screen, plus one page of prefetch.
    let (docs, pages) = h.state().documents().stats();
    assert_eq!((docs, pages), (1, visible(&h).len() + 1));
}

#[test]
fn the_wheel_scrolls_and_only_nearby_pages_are_kept_drawn() {
    let mut h = reader(&[("pdf", "sample:10000")]);
    h.hover_at(egui::pos2(450.0, 400.0));
    for _ in 0..20 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -2000.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    h.run();
    let shown = visible(&h);
    assert!(shown.start > 20, "scrolled to {shown:?}");
    // Ten thousand pages, yet work and textures track the viewport.
    let (_, pages) = h.state().documents().stats();
    assert!(pages <= 40, "{pages} textures");
}

#[test]
fn closing_the_last_pane_on_a_book_frees_its_textures() {
    let mut h = reader(&[("panes", "2")]);
    let baseline = textures(&h);
    let panes = h.state().panes();
    let (first, second) = (panes[0], panes[1]);
    h.state_mut().queue(Command::FocusPane(second));
    h.state_mut().set_option("pdf", "sample:30").ok();
    h.run();
    assert!(textures(&h) > baseline, "pages were uploaded");
    h.state_mut().queue(Command::ClosePane(second));
    h.run();
    assert_eq!(h.state().active_pane(), first);
    assert_eq!(h.state().documents().stats(), (0, 0));
    assert_eq!(textures(&h), baseline);
}

#[test]
fn a_broken_file_says_why_and_leaves_the_pane_empty() {
    let mut h = reader(&[]);
    let junk = OpenRequest::bytes(b"%PDF-1.7 nonsense".to_vec(), "Notes.pdf");
    h.state_mut().queue(Command::Open(junk));
    h.run();
    let toast = h.state().toast().map(|t| t.message.clone());
    assert_eq!(
        toast.as_deref(),
        Some("Couldn't open Notes: the file isn't a readable PDF.")
    );
    assert_eq!(h.state().title(), "Mareader");
}

#[test]
fn an_epub_is_turned_away_kindly_for_now() {
    let mut h = reader(&[]);
    let book = OpenRequest::bytes(vec![0; 8], "Dune.epub");
    h.state_mut().queue(Command::Open(book));
    h.run_steps(3);
    let toast = h.state().toast().map(|t| t.message.clone());
    assert!(toast.is_some_and(|t| t.contains("EPUB")));
}

fn sample(pages: usize) -> OpenRequest {
    let pdf = mareader_pdf::sample::pdf(pages, "A Mareader Sample");
    OpenRequest::bytes(pdf, "sample.pdf")
}

#[test]
fn open_beside_splits_and_opens_in_the_new_pane() {
    let mut h = reader(&[]);
    let first = h.state().active_pane();
    let request = OpenRequest {
        beside: true,
        ..sample(5)
    };
    h.state_mut().queue(Command::Open(request));
    h.run();
    let panes = h.state().panes();
    assert_eq!(panes.len(), 2);
    let new = h.state().active_pane();
    assert_ne!(new, first);
    let docs = h.state().documents();
    assert_eq!(docs.page_count(new), Some(5));
    assert_eq!(docs.page_count(first), None, "the first pane stays empty");
}

/// A file as the desktop hands it over: a path, read by a worker.
#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

#[test]
fn a_dropped_file_opens_in_the_pane_under_the_pointer() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("mareader-drop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("Dropped Notes.pdf");
    let pdf = mareader_pdf::sample::pdf(3, "untitled");
    std::fs::write(&path, pdf).map_err(|e| e.to_string())?;
    let mut h = reader(&[("panes", "2")]);
    let right = h.state().panes()[1];
    h.hover_at(egui::pos2(700.0, 350.0));
    h.run_steps(2);
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(path)));
    h.run();
    assert_eq!(h.state().active_pane(), right);
    assert_eq!(h.state().title(), "Dropped Notes");
    std::fs::remove_dir_all(dir).map_err(|e| e.to_string())
}

#[test]
fn an_empty_pane_offers_the_open_dialog() {
    let h = reader(&[]);
    let _ = h.get_by_label("Open…");
}
