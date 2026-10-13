//! The app state and its eframe loop.

use std::time::Duration;

use eframe::Storage;
use egui::{Context, ViewportCommand};
use mareader_appearance::{Appearance, BaseMode, builtin_presets};
use mareader_settings::Settings;
use mareader_workspace::{Bounds, MAX_PANES, PaneId, PaneTree, Side, SplitAxis, TreeError};

use crate::command::{Command, Commands, WindowCommand};
use crate::panes::{self, PaneView};
use crate::platform::Platform;
use crate::settings_window::{SettingsWindow, Tab, WindowContext};
use crate::sidebar::{Panel, RailContext, Sidebar};
use crate::titlebar::{BarContext, TitleBar};
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

    pub fn titlebar(&self) -> &TitleBar {
        &self.titlebar
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.tree.panes()
    }

    pub fn active_pane(&self) -> PaneId {
        self.active
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
            "chromeless" => self.titlebar.chromeless = value == "true",
            "animations" => self.settings.motion.enabled = value == "true",
            "float" => self.settings.chrome.sidebar_overlay = value == "true",
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
                let r = ctx.content_rect();
                let area = Bounds::new(r.left(), r.top(), r.width(), r.height());
                match self.tree.split(self.active, axis, Side::After, area) {
                    Ok(id) => self.active = id,
                    Err(TreeError::Full) => {
                        let note = format!("A workspace holds {MAX_PANES} panes at most.");
                        self.toasts.push(Toast::info(note));
                    }
                    Err(_) => self
                        .toasts
                        .push(Toast::info("This pane is too small to split.")),
                }
            }
            Command::FocusPane(id) => self.active = id,
            Command::ClosePane(id) => {
                if self.tree.close(id).is_ok() && self.active == id {
                    self.active = self.tree.panes().first().copied().unwrap_or(PaneId(1));
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

impl eframe::App for MareaderApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if self.fonts == FontState::Missing {
            theme::install_fonts(ctx);
            egui_extras::install_image_loaders(ctx);
            self.fonts = FontState::Installed;
        }
        self.restyle(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Fonts from set_fonts arrive next frame; named families panic before.
        if self.fonts != FontState::Ready {
            self.fonts = FontState::Ready;
            ctx.request_repaint();
            return;
        }
        resize::edges(&ctx, self.platform.frameless());
        let mut commands = std::mem::take(&mut self.queued);
        keymap::read(&ctx, self.platform, &mut commands);
        // An open popup takes Escape first.
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let modal = self.settings_window.is_open();
        if escape && self.sidebar.is_open() && !modal && !egui::Popup::is_any_open(&ctx) {
            commands.push(Command::CloseSidebar);
        }
        let overlay = self.settings.chrome.sidebar_overlay;
        let rail = RailContext {
            overlay,
            slide: self.settings.motion.sidebar_slide,
            platform: self.platform,
        };
        let inset = if overlay {
            self.sidebar.show_floating(&ctx, &rail, &mut commands)
        } else {
            self.sidebar.show_docked(ui, &rail, &mut commands)
        };
        let view = PaneView {
            active: self.active,
            look: self.settings.workspace.look,
            chrome_bottom: self.titlebar.band_bottom(&ctx),
        };
        egui::CentralPanel::no_frame().show(ui, |ui| {
            panes::show(ui, &self.tree, &view, &mut commands);
        });
        let bar = BarContext {
            title: "Mareader",
            platform: self.platform,
            left_inset: inset,
            sidebar_toggle: !overlay && !self.sidebar.is_open(),
            maximized: ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
            fullscreen: ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)),
            appearance: &self.settings.appearance,
        };
        self.titlebar.show(&ctx, &bar, &mut commands);
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
