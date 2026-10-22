//! Mareader's desktop entry point: the window, then `mareader_ui` runs.
#![deny(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::Duration;

use mareader_ui::command::Command;
use mareader_ui::{MareaderApp, OpenRequest};

use inbox::Inbox;
use single_instance::Claim;

mod inbox;
mod open_files;
mod reduced_motion;
mod single_instance;
#[cfg(any(test, all(target_os = "macos", feature = "traffic-lights")))]
mod traffic_lights;

/// The app plus the window glue only a desktop binary can do.
struct Desktop {
    app: MareaderApp,
    inbox: Inbox,
}

/// Opens the newest of `paths`, as mareader does; false if none.
fn open_latest(app: &mut MareaderApp, paths: Vec<PathBuf>) -> bool {
    let Some(path) = paths.into_iter().last() else {
        return false;
    };
    app.queue(Command::Open(OpenRequest::path(path)));
    true
}

impl eframe::App for Desktop {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        self.app.clear_color(visuals)
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let mail = self.inbox.take();
        if mail.raise {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        open_latest(&mut self.app, mail.paths);
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
    // macOS shows the bundle's icon; elsewhere the window and taskbar take it.
    #[cfg(not(target_os = "macos"))]
    let v = match eframe::icon_data::from_png_bytes(include_bytes!("../icons/128x128@2x.png")) {
        Ok(icon) => v.with_icon(icon),
        Err(_) => v,
    };
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
    let files: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .filter(|a| !open_files::is_launch_noise(a))
        .map(PathBuf::from)
        .collect();
    let inbox = Inbox::default();
    if let Some(dir) = eframe::storage_dir("mareader")
        && single_instance::claim(&dir, &files, &inbox) == Claim::Forwarded
    {
        return Ok(());
    }
    // `mareader book.pdf` opens it; Finder's files join it before launch.
    inbox.post(files, false);
    #[cfg(all(target_os = "macos", feature = "open-files"))]
    open_files::install(inbox.clone());
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
            if let Some(dir) = eframe::storage_dir("mareader") {
                app.attach_library(dir.join("library"));
                app.attach_ai(&cc.egui_ctx, dir.join("ai"), dev_helper());
                app.attach_dictionaries(&cc.egui_ctx, dir.join("dictionaries"));
                app.attach_vocabulary(&cc.egui_ctx, dir.join("vocabulary"));
                app.attach_voice_models(&cc.egui_ctx, dir.join("voice-models"));
            }
            if let Some(dir) = cache_dir() {
                app.attach_cache(dir);
            }
            inbox.wake_with(cc.egui_ctx.clone());
            // A file asked for at launch, else what was read last time.
            if !open_latest(&mut app, inbox.take().paths) {
                app.resume();
            }
            reduced_motion::watch(app.reduced_motion(), cc.egui_ctx.clone());
            Ok(Box::new(Desktop { app, inbox }))
        }),
    )
}

/// Where rebuildable files such as covers go, as each platform expects.
fn cache_dir() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "windows") {
        return var("LOCALAPPDATA").map(|d| d.join("mareader").join("cache"));
    }
    if cfg!(target_os = "macos") {
        return var("HOME").map(|h| h.join("Library/Caches/mareader"));
    }
    let base = var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache")));
    base.map(|d| d.join("mareader"))
}

/// Debug builds also find the helper `tools/fm-helper.sh` builds.
fn dev_helper() -> Option<PathBuf> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/helper/FMBridge");
    cfg!(debug_assertions).then(|| PathBuf::from(path))
}
