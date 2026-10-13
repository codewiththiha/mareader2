//! Mareader's desktop entry point: the window, then `mareader_ui` runs.
#![deny(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;

use mareader_ui::command::Command;
use mareader_ui::{MareaderApp, OpenRequest};

mod reduced_motion;
#[cfg(any(test, all(target_os = "macos", feature = "traffic-lights")))]
mod traffic_lights;

/// The app plus the window glue only a desktop binary can do.
struct Desktop {
    app: MareaderApp,
}

impl eframe::App for Desktop {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
        #[cfg(all(target_os = "macos", feature = "traffic-lights"))]
        {
            let fullscreen = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
            traffic_lights::place(frame, self.app.native_lights(), fullscreen);
        }
    }

    fn auto_save_interval(&self) -> Duration {
        self.app.auto_save_interval()
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.app.save(storage);
    }
}

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
        Box::new(|cc| {
            let mut app = MareaderApp::from_creation(cc);
            // `mareader book.pdf` opens it; else what was read last time.
            if let Some(path) = std::env::args_os().nth(1) {
                app.queue(Command::Open(OpenRequest::path(path.into())));
            } else {
                app.resume();
            }
            reduced_motion::watch(app.reduced_motion(), cc.egui_ctx.clone());
            Ok(Box::new(Desktop { app }))
        }),
    )
}
