//! Opening and scrolling PDFs, headless, with jobs run between frames.

use eframe::{App as _, Storage as _};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_ui::app::LAST_DOCUMENT_KEY;
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
    h.state_mut().set_option("focus", "2").ok();
    h.state_mut().set_option("pdf", "sample:30").ok();
    h.run();
    assert!(textures(&h) > baseline, "pages were uploaded");
    h.state_mut().queue(Command::ClosePane(second));
    h.run();
    assert_eq!(h.state().active_pane(), first);
    // Nothing of it is left: no document, raster, text layer or job.
    let empty = mareader_ui::documents::Meter::default();
    assert_eq!(h.state().documents().meter(), empty);
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
fn an_epub_opens_titled_with_its_pages_drawn() {
    let h = reader(&[("epub", "sample:3")]);
    assert_eq!(h.state().title(), "A Mareader Book");
    let pane = h.state().active_pane();
    let pages = h.state().documents().page_count(pane);
    // Three chapters, each opening a page and running onto another.
    assert!(pages.is_some_and(|n| n >= 6), "{pages:?}");
    assert!(h.state().window_context().reflowable);
    let (docs, drawn) = h.state().documents().stats();
    assert_eq!((docs, drawn), (1, visible(&h).len() + 1));
}

#[test]
fn a_broken_epub_says_why() {
    let mut h = reader(&[]);
    let book = OpenRequest::bytes(b"PK\x03\x04 not really".to_vec(), "Dune.epub");
    h.state_mut().queue(Command::Open(book));
    h.run_steps(3);
    let toast = h.state().toast().map(|t| t.message.clone());
    assert!(
        toast.as_ref().is_some_and(|t| t.contains("EPUB")),
        "{toast:?}"
    );
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

fn zoom(h: &Harness<'static, MareaderApp>) -> mareader_ui::zoom::ZoomState {
    let pane = h.state().active_pane();
    let state = h.state().documents().zoom_state(pane);
    state.unwrap_or(mareader_ui::zoom::ZoomState {
        zoom: 0.0,
        fit: None,
    })
}

fn current(h: &Harness<'static, MareaderApp>) -> usize {
    let pane = h.state().active_pane();
    h.state().documents().current(pane).unwrap_or(usize::MAX)
}

/// Opens 60 pages and wheels down to somewhere in the middle.
fn reading_page_twenty() -> Harness<'static, MareaderApp> {
    let mut h = reader(&[("pdf", "sample:60")]);
    h.hover_at(egui::pos2(450.0, 400.0));
    for _ in 0..12 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -1000.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    h.run();
    h
}

#[test]
fn zoom_keys_step_the_ladder_and_keep_the_page_in_view() {
    use mareader_ui::zoom::{Fit, MAX};
    let mut h = reading_page_twenty();
    let before = zoom(&h);
    assert_eq!(before.fit, Some(Fit::Width));
    let page = current(&h);
    assert!(page > 5, "reading page {page}");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Equals);
    h.run();
    let after = zoom(&h);
    assert_eq!(after.fit, None);
    assert_eq!(
        Some(after.zoom),
        mareader_ui::zoom::next(before.zoom, mareader_ui::zoom::Step::In)
    );
    assert_eq!(current(&h), page, "the page under the centre stays");
    // Bare minus steps back down; Cmd+0 returns to fit width.
    h.key_press(egui::Key::Minus);
    h.run();
    assert!(zoom(&h).zoom < after.zoom);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num0);
    h.run();
    assert_eq!(zoom(&h), before);
    assert_eq!(current(&h), page);
    // Chained presses stop at the top of the ladder.
    for _ in 0..20 {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Plus);
    }
    h.run();
    assert_eq!(zoom(&h).zoom, MAX);
    assert_eq!(current(&h), page);
}

#[test]
fn a_pinch_zooms_about_the_pointer() {
    let mut h = reading_page_twenty();
    let before = zoom(&h).zoom;
    let page = current(&h);
    h.hover_at(egui::pos2(450.0, 400.0));
    h.event(egui::Event::Zoom(1.5));
    h.run();
    let after = zoom(&h);
    assert_eq!(after.fit, None);
    assert!((after.zoom - before * 1.5).abs() < 1e-3, "{after:?}");
    assert_eq!(current(&h), page);
}

#[test]
fn the_menu_fits_the_page() {
    use mareader_ui::zoom::Fit;
    let mut h = reader(&[("pdf", "sample:5")]);
    h.state_mut().queue(Command::ShowViewMenu);
    h.run();
    h.get_by_label("Fit page").click();
    h.run();
    let z = zoom(&h);
    assert_eq!(z.fit, Some(Fit::Page));
    // A letter page 700 high, less margins, fits by its height.
    let want = (700.0 - 32.0) / 792.0;
    assert!((z.zoom - want).abs() < 0.01, "{z:?} want {want}");
    let _ = h.get_by_label(&z.percent());
}

#[test]
fn a_resize_refits_and_keeps_the_page() {
    let mut h = reading_page_twenty();
    let page = current(&h);
    h.set_size(egui::vec2(600.0, 700.0));
    h.run();
    let z = zoom(&h);
    // No page margin by default: the page meets the pane's sides.
    assert!((z.zoom - 600.0 / 612.0).abs() < 0.01, "{z:?}");
    assert_eq!(current(&h), page);
}

#[derive(Default)]
struct Memory(std::collections::HashMap<String, String>);

impl eframe::Storage for Memory {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }

    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }

    fn flush(&mut self) {}
}

/// A sample PDF on disk, deleted when dropped.
struct File(std::path::PathBuf);

impl File {
    fn sample(pages: usize, tag: &str) -> Result<Self, String> {
        let name = format!("mareader-{tag}-{}.pdf", std::process::id());
        let path = std::env::temp_dir().join(name);
        let pdf = mareader_pdf::sample::pdf(pages, "Read Last Time");
        std::fs::write(&path, pdf).map_err(|e| e.to_string())?;
        Ok(Self(path))
    }
}

impl Drop for File {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}

fn launched(mut app: MareaderApp) -> Harness<'static, MareaderApp> {
    app.set_option("workers", "0").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

#[test]
fn the_book_read_at_exit_reopens_on_its_page() -> Result<(), String> {
    let file = File::sample(30, "resume")?;
    let mut app = MareaderApp::default();
    let request = OpenRequest {
        page: 12,
        ..OpenRequest::path(file.0.clone())
    };
    app.queue(Command::Open(request));
    let mut h = launched(app);
    let mut disk = Memory::default();
    h.state_mut().save(&mut disk);
    // The next launch, from what was saved.
    let mut again = MareaderApp::from_storage(Some(&disk));
    assert!(
        again.resume(),
        "saved {:?}",
        disk.get_string(LAST_DOCUMENT_KEY)
    );
    let h = launched(again);
    assert_eq!(h.state().title(), "Read Last Time");
    let pane = h.state().active_pane();
    assert_eq!(h.state().documents().current(pane), Some(12));
    // A file gone since then leaves the window empty, without a fuss.
    drop(file);
    let mut moved = MareaderApp::from_storage(Some(&disk));
    assert!(!moved.resume());
    Ok(())
}

#[test]
fn a_dropped_book_or_an_empty_window_leaves_nothing_to_reopen() {
    let mut h = reader(&[]);
    let mut disk = Memory::default();
    h.state_mut().save(&mut disk);
    assert_eq!(disk.get_string(LAST_DOCUMENT_KEY).as_deref(), Some("null"));
    // Bytes, such as a browser's drop, have no file to come back to.
    h.state_mut().queue(Command::Open(sample(3)));
    h.run();
    assert_eq!(h.state().last_document(), None);
    let mut app = MareaderApp::from_storage(Some(&disk));
    assert!(!app.resume());
}

#[test]
fn one_file_in_two_panes_is_read_once_and_drawn_once() -> Result<(), String> {
    let file = File::sample(20, "twice")?;
    let mut h = reader(&[("panes", "2")]);
    let panes = h.state().panes();
    for pane in &panes {
        let request = OpenRequest {
            pane: Some(*pane),
            ..OpenRequest::path(file.0.clone())
        };
        h.state_mut().queue(Command::Open(request));
        h.run();
    }
    let meter = h.state().documents().meter();
    assert_eq!(meter.documents, 1, "one parse");
    // Panes of one size want the same rasters, so they share them.
    let shown = h.state().documents().visible(panes[0]).unwrap_or(0..0);
    assert_eq!(meter.pages, shown.len() + 1, "one set of pages");
    h.state_mut().queue(Command::ClosePane(panes[1]));
    h.run();
    assert_eq!(h.state().documents().meter().documents, 1, "still read");
    Ok(())
}
