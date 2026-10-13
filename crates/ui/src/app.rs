//! The app state and its eframe loop.

use std::time::Duration;

use eframe::Storage;
use egui::{Context, ViewportCommand};
use mareader_appearance::{Appearance, BaseMode, builtin_presets};
use mareader_settings::Settings;
use mareader_workspace::{Bounds, MAX_PANES, PaneId, PaneTree, Side, SplitAxis, TreeError};

use crate::backdrop::Backdrop;
use crate::command::{Command, Commands, WindowCommand};
use crate::documents::{Documents, OpenRequest};
use crate::files::Files;
use crate::gpu::{Glass, Gpu, Quad};
use crate::outline_panel::{self, OutlineState};
use crate::page_bar::{self, BarLook, PageBar};
use crate::panes::{self, PaneView};
use crate::pdf_view::ViewContext;
use crate::platform::{NativeLights, Platform};
use crate::settings_window::{SettingsWindow, Tab, WindowContext};
use crate::sidebar::{Panel, RailContext, Sidebar};
use crate::thumbnails::ThumbState;
use crate::titlebar::{self, BarContext, TitleBar};
use crate::toast::{Toast, Toasts};
use crate::{keymap, motion, resize, theme};

pub const SETTINGS_KEY: &str = "settings";

/// Settings reach disk this long after a change, at most.
pub const SAVE_EVERY: Duration = Duration::from_millis(500);

pub struct MareaderApp {
    settings: Settings,
    platform: Platform,
    titlebar: TitleBar,
    tree: PaneTree,
    active: PaneId,
    sidebar: Sidebar,
    settings_window: SettingsWindow,
    toasts: Toasts,
    lights: NativeLights,
    documents: Documents,
    files: Files,
    /// The sidebar panels' scroll-following, for the focused pane.
    outline: OutlineState,
    thumbs: ThumbState,
    /// Each pane's page bar.
    page_bars: std::collections::BTreeMap<PaneId, PageBar>,
    /// Last frame's pane rects, to aim a file drop.
    pane_rects: Vec<(PaneId, egui::Rect)>,
    styled: Option<(Appearance, bool)>,
    fonts: FontState,
    /// Commands from outside a view: tests, OS drops, later the keymap.
    queued: Commands,
    #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
    live: theme::live::LiveTokens,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FontState {
    Missing,
    Installed,
    Ready,
}

impl Default for MareaderApp {
    fn default() -> Self {
        Self::with_settings(Settings::default())
    }
}

impl MareaderApp {
    pub fn with_settings(mut settings: Settings) -> Self {
        settings.sanitize();
        Self {
            titlebar: TitleBar::new(settings.chrome.titlebar_pinned),
            settings,
            platform: Platform::current(),
            tree: PaneTree::new(),
            active: PaneId(1),
            sidebar: Sidebar::default(),
            settings_window: SettingsWindow::default(),
            toasts: Toasts::default(),
            lights: NativeLights::default(),
            documents: Documents::default(),
            files: Files::default(),
            outline: OutlineState::default(),
            thumbs: ThumbState::default(),
            page_bars: std::collections::BTreeMap::new(),
            pane_rects: Vec::new(),
            styled: None,
            fonts: FontState::Missing,
            queued: Commands::new(),
            #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
            live: theme::live::LiveTokens::from_env(),
        }
    }

    pub fn from_creation(cc: &eframe::CreationContext<'_>) -> Self {
        Self::from_storage(cc.storage)
    }

    /// Unreadable settings repair section by section; the notes go to stderr.
    pub fn from_storage(storage: Option<&dyn Storage>) -> Self {
        let Some(json) = storage.and_then(|s| s.get_string(SETTINGS_KEY)) else {
            return Self::default();
        };
        let loaded = mareader_settings::load(&json);
        for note in &loaded.notes {
            eprintln!("mareader: {note}");
        }
        let repaired = !loaded.notes.is_empty();
        let mut app = Self::with_settings(loaded.settings);
        if repaired {
            let note = "Some settings couldn't be read and were reset to defaults.";
            app.toasts.push(Toast::error(note));
        }
        app
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn toast(&self) -> Option<&Toast> {
        self.toasts.current()
    }

    /// Applied at the end of the next frame, like a view's commands.
    pub fn queue(&mut self, command: Command) {
        self.queued.push(command);
    }

    /// Restyles egui when the look changed; true if it did.
    fn restyle(&mut self, ctx: &Context) -> bool {
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        if self.live.poll(ctx) {
            self.styled = None;
        }
        let want = (self.settings.appearance, self.settings.motion.enabled);
        if self.styled == Some(want) {
            return false;
        }
        #[allow(unused_mut)]
        let mut tokens = theme::Tokens::new(&self.settings.appearance);
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        self.live.overlay(&mut tokens);
        theme::apply(ctx, &tokens);
        motion::set_enabled(ctx, self.settings.motion.enabled);
        self.styled = Some(want);
        true
    }

    pub fn settings_window(&self) -> &SettingsWindow {
        &self.settings_window
    }

    pub fn window_context(&self) -> WindowContext {
        WindowContext {
            split: self.tree.len() > 1,
            reflowable: false,
        }
    }

    pub fn sidebar(&self) -> &Sidebar {
        &self.sidebar
    }

    /// Where the macOS traffic lights go; the desktop binary moves them.
    pub fn native_lights(&self) -> NativeLights {
        self.lights
    }

    pub fn titlebar(&self) -> &TitleBar {
        &self.titlebar
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.tree.panes()
    }

    pub fn active_pane(&self) -> PaneId {
        self.active
    }

    pub fn documents(&self) -> &Documents {
        &self.documents
    }

    /// The bar's title: the active pane's document, else the app.
    pub fn title(&self) -> &str {
        self.documents.title(self.active).unwrap_or("Mareader")
    }

    /// Headless knobs for `examples/shot.rs` and tests.
    pub fn set_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "theme" => {
                let base = BaseMode::ALL
                    .into_iter()
                    .find(|b| b.label().eq_ignore_ascii_case(value));
                self.settings.appearance.base = base.ok_or("theme: light, dark or dim")?;
            }
            "preset" => {
                let p = builtin_presets().into_iter().find(|p| p.id == value);
                self.settings.appearance = p
                    .ok_or("preset: sepia, green, night, parchment, cinema")?
                    .appearance;
            }
            "platform" => {
                self.platform = Platform::parse(value).ok_or("platform: mac, windows or gnome")?
            }
            "toast" => self.toasts.push(match value.strip_prefix("error:") {
                Some(message) => Toast::error(message),
                None => Toast::info(value),
            }),
            "pinned" => self.titlebar.pinned = value == "true",
            "workers" => self.documents.set_workers(
                value
                    .parse()
                    .map_err(|_| "workers: a count, 0 runs inline")?,
            ),
            "pdf" => self.queue(Command::Open(match value.strip_prefix("sample") {
                Some(pages) => {
                    let pages = pages.trim_start_matches(':').parse().unwrap_or(120);
                    let pdf = mareader_pdf::sample::pdf(pages, "A Mareader Sample");
                    OpenRequest::bytes(pdf, "sample.pdf")
                }
                None => OpenRequest::path(value.into()),
            })),
            "chromeless" => self.titlebar.chromeless = value == "true",
            "animations" => self.settings.motion.enabled = value == "true",
            "float" => self.settings.chrome.sidebar_overlay = value == "true",
            "glass" => self.settings.chrome.glass = value == "true",
            "sidebar" => match Panel::parse(value) {
                Some(p) => self.sidebar.open(Some(p)),
                None if value == "closed" => self.sidebar.close(),
                None => return Err("sidebar: thumbnails, outline, library or closed".into()),
            },
            "open" => match value {
                "appearance" => self.queue(Command::ShowAppearance),
                "settings" => self.settings_window.open(None),
                "menu" => self.queue(Command::ShowViewMenu),
                _ => return Err("open: appearance, settings or menu".into()),
            },
            "settings" => {
                let tab = Tab::parse(value).ok_or("settings: a tab name, e.g. theme")?;
                self.settings_window.open(Some(tab));
            }
            "panes" => {
                let n: usize = value.parse().map_err(|_| "panes: 1 to 4")?;
                let area = Bounds::new(0.0, 0.0, 1600.0, 1000.0);
                while self.tree.len() < n.min(mareader_workspace::MAX_PANES) {
                    let last = self.tree.panes().last().copied().unwrap_or(self.active);
                    let axis = if self.tree.len() % 2 == 1 {
                        SplitAxis::Horizontal
                    } else {
                        SplitAxis::Vertical
                    };
                    self.tree
                        .split(last, axis, Side::After, area)
                        .map_err(|e| e.to_string())?;
                }
            }
            _ => return Err(format!("unknown option {key}")),
        }
        Ok(())
    }

    fn apply(&mut self, ctx: &Context, command: Command) {
        match command {
            Command::ToggleSidebar => self.sidebar.toggle(),
            Command::OpenSidebar(panel) => self.sidebar.open(panel),
            Command::CloseSidebar => self.sidebar.close(),
            Command::SetSidebarOverlay(on) => self.settings.chrome.sidebar_overlay = on,
            Command::Split(axis) => {
                if let Some(id) = self.split(ctx, self.active, axis) {
                    self.active = id;
                }
            }
            Command::FocusPane(id) => self.active = id,
            Command::ClosePane(id) => {
                if self.tree.close(id).is_ok() {
                    self.documents.close_pane(id);
                    if self.active == id {
                        self.active = self.tree.panes().first().copied().unwrap_or(PaneId(1));
                    }
                }
            }
            Command::ShowOpenDialog { beside } => self.files.pick(ctx, beside),
            Command::Zoom(zoom) => {
                let now = ctx.input(|i| i.time);
                self.documents.zoom(self.active, zoom, now);
            }
            Command::View(mode) => {
                let now = ctx.input(|i| i.time);
                let fit = self.settings.layout.auto_scale;
                self.documents.set_mode(self.active, mode, fit, now);
            }
            Command::Arrow(arrow) => self.documents.arrow(self.active, arrow),
            Command::GoTo { pane, page } => self.documents.go_to(pane, page),
            Command::Turn { pane, forward } => self.documents.turn(pane, forward),
            Command::Open(request) => {
                let mut pane = request.pane.unwrap_or(self.active);
                // Open beside: a new pane to the right, as in mareader.
                if request.beside {
                    match self.split(ctx, pane, SplitAxis::Horizontal) {
                        Some(new) => pane = new,
                        None => return,
                    }
                }
                if self.tree.panes().contains(&pane) {
                    self.active = pane;
                    if let Some(note) = self.documents.open(ctx, pane, request) {
                        self.toasts.push(note);
                    }
                }
            }
            Command::SetRatio(path, ratio) => {
                self.tree.set_ratio(&path, ratio);
            }
            Command::SetAppearance(mut a) => {
                a.sanitize();
                self.settings.appearance = a;
            }
            Command::SetTitlebarPinned(p) => {
                self.titlebar.pinned = p;
                self.settings.chrome.titlebar_pinned = p;
            }
            Command::Window(w) => ctx.send_viewport_cmd(match w {
                WindowCommand::Minimize => ViewportCommand::Minimized(true),
                WindowCommand::ToggleMaximize => {
                    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ViewportCommand::Maximized(!maximized)
                }
                WindowCommand::Close => ViewportCommand::Close,
                WindowCommand::StartDrag => ViewportCommand::StartDrag,
                WindowCommand::ToggleFullscreen => {
                    let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    ViewportCommand::Fullscreen(!full)
                }
            }),
            Command::Notify(toast) => self.toasts.push(toast),
            Command::DismissToast => self.toasts.dismiss(ctx.input(|i| i.time)),
            Command::ShowAppearance => {
                self.titlebar.reveal();
                egui::Popup::open_id(ctx, egui::Id::new(crate::titlebar::APPEARANCE_POPOVER));
            }
            Command::ShowViewMenu => {
                self.titlebar.reveal();
                egui::Popup::open_id(ctx, egui::Id::new(crate::view_menu::VIEW_MENU));
            }
            Command::ShowSettings => self.settings_window.open(None),
            Command::OpenSettings(tab) => self.settings_window.open(Some(tab)),
            Command::CloseSettings => self.settings_window.close(),
            Command::SetSettings(s) => {
                let mut s = *s;
                s.sanitize();
                self.titlebar.pinned = s.chrome.titlebar_pinned;
                self.settings = s;
            }
            Command::ShowLibrary | Command::ShowSearch => {}
        }
    }
}

impl MareaderApp {
    /// Splits `pane`; a refusal is explained in a toast.
    fn split(&mut self, ctx: &Context, pane: PaneId, axis: SplitAxis) -> Option<PaneId> {
        let r = ctx.content_rect();
        let area = Bounds::new(r.left(), r.top(), r.width(), r.height());
        match self.tree.split(pane, axis, Side::After, area) {
            Ok(id) => Some(id),
            Err(TreeError::Full) => {
                let note = format!("A workspace holds {MAX_PANES} panes at most.");
                self.toasts.push(Toast::info(note));
                None
            }
            Err(_) => {
                let note = "This pane is too small to split.";
                self.toasts.push(Toast::info(note));
                None
            }
        }
    }

    /// Drops open in the pane under the pointer; returns the hover target.
    fn drops(&self, ctx: &Context) -> Option<PaneId> {
        let pointer = ctx.pointer_latest_pos();
        let under = pointer
            .and_then(|p| self.pane_rects.iter().find(|(_, r)| r.contains(p)))
            .map_or(self.active, |(id, _)| *id);
        let (hovering, dropped) = ctx.input(|i| {
            let first = i.raw.dropped_files.first().cloned();
            (!i.raw.hovered_files.is_empty(), first)
        });
        if let Some(file) = dropped {
            self.files.dropped(ctx, &file, under);
        }
        hovering.then_some(under)
    }

    /// Every document pane's counter, progress line and bottom bar.
    fn page_bars(&mut self, ctx: &Context, backdrop: &Backdrop, out: &mut Commands) {
        let rects = &self.pane_rects;
        self.page_bars
            .retain(|id, _| rects.iter().any(|(p, _)| p == id));
        // The title bar's glass takes slot 0.
        for (i, (id, rect)) in rects.iter().enumerate() {
            if let Some(reading) = self.documents.reading(*id) {
                let look = BarLook {
                    layout: &self.settings.layout,
                    backdrop,
                    slot: i + 1,
                };
                let bar = self.page_bars.entry(*id).or_default();
                page_bar::show(ctx, *rect, &reading, &look, bar, out);
            }
        }
    }

    /// What the bars show behind them: pane fills, then this frame's pages.
    fn backdrop(&mut self, ctx: &Context) -> Backdrop {
        let drawn = self.documents.take_drawn();
        if !self.settings.chrome.glass {
            return Backdrop::Solid;
        }
        let Some(gpu) = self.documents.gpu() else {
            return Backdrop::Tint;
        };
        let t = theme::Tokens::get(ctx);
        let fills = self.pane_rects.iter().map(|(_, r)| Quad::fill(*r, t.paper));
        let mut quads: Vec<Quad> = fills.collect();
        quads.extend(drawn);
        Backdrop::Glass(Glass {
            pass: gpu.pass.clone(),
            canvas: t.surface,
            quads,
        })
    }

    /// The rail hosts the lights while drawn; then the bar, unless overlay.
    fn place_lights(&self, ctx: &Context, overlay: bool) -> NativeLights {
        let bar_hosts = self.sidebar.offset().is_none() && !overlay;
        // Handing over from a rail, they fade in rather than pop.
        let secs = motion::secs(ctx, titlebar::FADE);
        let handoff = ctx.animate_bool_with_time(egui::Id::new("lights-bar"), bar_hosts, secs);
        match self.sidebar.offset() {
            Some(shift) => NativeLights {
                shift,
                opacity: 1.0,
            },
            None if bar_hosts => NativeLights {
                shift: 0.0,
                opacity: self.titlebar.opacity().min(handoff),
            },
            None => NativeLights::default(),
        }
    }
}

/// Three dots where AppKit would put its buttons, for checking off a Mac.
fn stand_in_lights(ctx: &Context, lights: NativeLights) {
    if lights.opacity <= 0.0 {
        return;
    }
    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("stand-in-lights"));
    let painter = ctx.layer_painter(layer);
    let top = ctx.content_rect().left_top();
    let colours = [(255, 95, 87), (254, 188, 46), (40, 200, 64)];
    for (i, (r, g, b)) in colours.into_iter().enumerate() {
        let x = 27.0 + lights.shift + 20.0 * i as f32;
        let fill = egui::Color32::from_rgb(r, g, b).gamma_multiply(lights.opacity);
        painter.circle_filled(top + egui::vec2(x, theme::TITLE_BAR_H / 2.0), 6.0, fill);
    }
}

impl eframe::App for MareaderApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if self.fonts == FontState::Missing {
            theme::install_fonts(ctx);
            egui_extras::install_image_loaders(ctx);
            // Zoom keys zoom the document, not the interface.
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
            self.fonts = FontState::Installed;
        }
        self.restyle(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.documents.gpu().is_none()
            && let Some(render) = frame.wgpu_render_state()
        {
            self.documents.set_gpu(Gpu::new(render));
        }
        // Fonts from set_fonts arrive next frame; named families panic before.
        if self.fonts != FontState::Ready {
            self.fonts = FontState::Ready;
            ctx.request_repaint();
            return;
        }
        resize::edges(&ctx, self.platform.frameless());
        let mut commands = std::mem::take(&mut self.queued);
        self.documents.receive(&ctx, &mut commands);
        self.files.collect(&mut commands);
        keymap::read(&ctx, self.platform, &mut commands);
        let drop_target = self.drops(&ctx);
        // An open popup takes Escape first.
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let modal = self.settings_window.is_open();
        let free = escape && !modal && !egui::Popup::is_any_open(&ctx);
        // A selection takes Escape before the sidebar does.
        if free && !self.documents.clear_selection(self.active) && self.sidebar.is_open() {
            commands.push(Command::CloseSidebar);
        }
        let overlay = self.settings.chrome.sidebar_overlay;
        let rail = RailContext {
            overlay,
            slide: self.settings.motion.sidebar_slide,
            platform: self.platform,
        };
        let (documents, active) = (&mut self.documents, self.active);
        let (outline, thumbs) = (&mut self.outline, &mut self.thumbs);
        // The rail's panels serve the focused pane's document.
        let mut body =
            |ui: &mut egui::Ui, panel: Panel, recentre: bool, out: &mut Commands| match panel {
                Panel::Outline => documents.reading(active).is_some_and(|r| {
                    outline_panel::show(ui, &r, outline, recentre, out);
                    true
                }),
                Panel::Thumbnails => documents.thumbnails(ui, active, thumbs, recentre, out),
                Panel::Library | Panel::Dictionary => false,
            };
        let inset = if overlay {
            self.sidebar
                .show_floating(&ctx, &rail, &mut body, &mut commands)
        } else {
            self.sidebar
                .show_docked(ui, &rail, &mut body, &mut commands)
        };
        let view = PaneView {
            active: self.active,
            look: self.settings.workspace.look,
            chrome_bottom: self.titlebar.band_bottom(&ctx),
            drop_target,
        };
        // A pinned bar covers the panes' top; documents start below it.
        let pinned_bottom = if self.titlebar.pinned {
            view.chrome_bottom
        } else {
            f32::NEG_INFINITY
        };
        let documents = &mut self.documents;
        egui::CentralPanel::no_frame().show(ui, |ui| {
            self.pane_rects = panes::show(ui, &self.tree, &view, &mut commands, |ui, id| {
                let cx = ViewContext {
                    active: id == view.active,
                    top_inset: (pinned_bottom - ui.max_rect().top()).max(0.0),
                };
                documents.show(ui, id, &cx)
            });
        });
        let backdrop = self.backdrop(&ctx);
        self.page_bars(&ctx, &backdrop, &mut commands);
        self.documents.end_frame();
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        let bar = BarContext {
            title: self.documents.title(self.active).unwrap_or("Mareader"),
            platform: self.platform,
            left_inset: inset,
            sidebar_toggle: !overlay && !self.sidebar.is_open(),
            maximized: ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
            fullscreen,
            gutter: !overlay && inset <= 0.0 && !fullscreen,
            appearance: &self.settings.appearance,
            zoom: self.documents.zoom_state(self.active),
            mode: self.documents.mode(self.active),
            backdrop: &backdrop,
        };
        self.titlebar.show(&ctx, &bar, &mut commands);
        self.lights = self.place_lights(&ctx, overlay);
        // Shots and the preview simulate macOS; draw where the lights go.
        if self.platform == Platform::Mac && !cfg!(target_os = "macos") {
            stand_in_lights(&ctx, self.lights);
        }
        let mut stage = ctx.content_rect();
        stage.min.x += inset;
        self.toasts.show(&ctx, stage, &mut commands);
        let window = self.window_context();
        self.settings_window
            .show(&ctx, &self.settings, &window, &mut commands);
        // Applied after drawing, so the next frame must show the result.
        if !commands.is_empty() {
            ctx.request_repaint();
            let before = self.settings.clone();
            for command in commands {
                self.apply(&ctx, command);
            }
            // eframe autosaves only on a frame, so book one after the interval.
            if self.settings != before {
                ctx.request_repaint_after(SAVE_EVERY + Duration::from_millis(50));
            }
        }
        // This frame drew with the old look; show the new one next frame.
        if self.restyle(&ctx) {
            ctx.request_repaint();
        }
    }

    /// Writes happen on eframe's persist thread, and only when changed.
    fn auto_save_interval(&self) -> Duration {
        SAVE_EVERY
    }

    fn save(&mut self, storage: &mut dyn Storage) {
        storage.set_string(SETTINGS_KEY, mareader_settings::to_json(&self.settings));
    }
}
