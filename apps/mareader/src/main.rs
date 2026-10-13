//! Mareader's desktop entry point: the window, then `mareader_ui` runs.
#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mareader_ui::MareaderApp;

fn viewport() -> egui::ViewportBuilder {
    let v = egui::ViewportBuilder::default()
        .with_title("Mareader")
        .with_app_id("mareader")
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size([640.0, 480.0]);
    // macOS keeps its traffic lights; elsewhere the app draws captions.
    if cfg!(target_os = "macos") {
        v.with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
    } else {
        v.with_decorations(false)
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: viewport(),
        renderer: eframe::Renderer::Wgpu,
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "mareader",
        options,
        Box::new(|cc| Ok(Box::new(MareaderApp::from_creation(cc)))),
    )
}
