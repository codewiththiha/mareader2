//! The app state and its eframe loop.

use eframe::Storage;
use egui::{Context, ViewportCommand};
use mareader_appearance::{Appearance, BaseMode, builtin_presets};
use mareader_workspace::{Bounds, PaneId, PaneTree, Side, SplitAxis};
use serde::{Deserialize, Serialize};

use crate::panes::{self, PaneAction, PaneLook};
use crate::platform::Platform;
use crate::titlebar::{BarAction, BarContext, TitleBar};
use crate::widgets::Caption;
use crate::{motion, resize, theme};

/// mareader's settings key, so a migration can read the same name.
pub const SETTINGS_KEY: &str = "mareader.settings.v1";

/// The persisted slice that exists so far; fields join as features land.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    pub titlebar_pinned: bool,
    pub animations: bool,
}

pub struct MareaderApp {
    settings: Settings,
    platform: Platform,
    titlebar: TitleBar,
    tree: PaneTree,
    active: PaneId,
    look: PaneLook,
    sidebar_open: bool,
    styled: Option<(Appearance, bool)>,
    fonts: FontState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FontState {
    Missing,
    Installed,
    Ready,
}

impl Default for MareaderApp {
    fn default() -> Self {
        Self::with_settings(Settings {
            animations: true,
            ..Settings::default()
        })
    }
}

impl MareaderApp {
    pub fn with_settings(mut settings: Settings) -> Self {
        settings.appearance.sanitize();
        Self {
            titlebar: TitleBar::new(settings.titlebar_pinned),
            settings,
            platform: Platform::current(),
            tree: PaneTree::new(),
            active: PaneId(1),
            look: PaneLook::default(),
            sidebar_open: false,
            styled: None,
            fonts: FontState::Missing,
        }
    }

    pub fn from_creation(cc: &eframe::CreationContext<'_>) -> Self {
        let saved = cc
            .storage
            .and_then(|s| s.get_string(SETTINGS_KEY))
            .and_then(|json| serde_json::from_str::<Settings>(&json).ok());
        saved.map_or_else(Self::default, Self::with_settings)
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
            "pinned" => self.titlebar.pinned = value == "true",
            "chromeless" => self.titlebar.chromeless = value == "true",
            "animations" => self.settings.animations = value == "true",
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

    fn split(&mut self, axis: SplitAxis, ctx: &Context) {
        let r = ctx.content_rect();
        let area = Bounds::new(r.left(), r.top(), r.width(), r.height());
        if let Ok(id) = self.tree.split(self.active, axis, Side::After, area) {
            self.active = id;
        }
    }

    fn apply_bar(&mut self, ctx: &Context, actions: Vec<BarAction>, maximized: bool) {
        for a in actions {
            match a {
                BarAction::ToggleSidebar => self.sidebar_open = !self.sidebar_open,
                BarAction::SplitRight => self.split(SplitAxis::Horizontal, ctx),
                BarAction::SplitDown => self.split(SplitAxis::Vertical, ctx),
                BarAction::PinChanged(p) => self.settings.titlebar_pinned = p,
                BarAction::Window(Caption::Minimize) => {
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(true))
                }
                BarAction::Window(Caption::ToggleMaximize) => {
                    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                }
                BarAction::Window(Caption::Close) => ctx.send_viewport_cmd(ViewportCommand::Close),
                BarAction::Library
                | BarAction::Search
                | BarAction::Appearance
                | BarAction::Settings => {}
            }
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
        let want = (self.settings.appearance, self.settings.animations);
        if self.styled != Some(want) {
            theme::apply(ctx, &self.settings.appearance);
            motion::set_enabled(ctx, self.settings.animations);
            self.styled = Some(want);
        }
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
        let t = theme::Tokens::get(&ctx);
        let area = ctx.content_rect();
        ui.painter().rect_filled(area, 0.0, t.paper);
        let under_bar = self.titlebar.pinned && !self.titlebar.chromeless;
        for action in panes::show(ui, &mut self.tree, self.active, self.look, area, under_bar) {
            match action {
                PaneAction::Focus(id) => self.active = id,
                PaneAction::Close(id) => {
                    if self.tree.close(id).is_ok() && self.active == id {
                        self.active = self.tree.panes().first().copied().unwrap_or(PaneId(1));
                    }
                }
            }
        }
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let bar = BarContext {
            title: "Mareader",
            platform: self.platform,
            sidebar_open: self.sidebar_open,
            left_inset: 0.0,
            maximized,
        };
        let actions = self.titlebar.show(&ctx, &bar);
        self.apply_bar(&ctx, actions, maximized);
    }

    fn save(&mut self, storage: &mut dyn Storage) {
        if let Ok(json) = serde_json::to_string(&self.settings) {
            storage.set_string(SETTINGS_KEY, json);
        }
    }
}
