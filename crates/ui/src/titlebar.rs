//! The custom title bar; behaviour and metrics are in `docs/look.md`.

use egui::{Align, Align2, Context, CornerRadius, Id, Layout, Order, Rect, Sense, UiBuilder, pos2};

use crate::icons::Icon;
use crate::motion;
use crate::platform::Platform;
use crate::theme::{self, TITLE_BAR_H, Tokens};
use crate::widgets::{self, Caption};

/// mareader's `DEFAULT_HOVER_DELAY`: the grace before the bar hides.
pub const HIDE_GRACE: f64 = 0.4;
/// The opacity fade, mareader's `transition-opacity duration-200`.
pub const FADE: f32 = 0.2;
/// `gap-2` on the row; `gap-1` in the trailing cluster.
const ROW_GAP: f32 = 8.0;
const TRAIL_GAP: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarAction {
    ToggleSidebar,
    Library,
    SplitRight,
    SplitDown,
    Search,
    Appearance,
    Settings,
    PinChanged(bool),
    Window(Caption),
}

/// What the bar needs from the app this frame.
pub struct BarContext<'a> {
    pub title: &'a str,
    pub platform: Platform,
    pub sidebar_open: bool,
    /// The band never sits over a docked sidebar; this is its width.
    pub left_inset: f32,
    pub maximized: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TitleBar {
    pub pinned: bool,
    /// Never shows, hover or pin (a distraction-free reading mode).
    pub chromeless: bool,
    shown: bool,
    left_at: Option<f64>,
}

impl TitleBar {
    pub fn new(pinned: bool) -> Self {
        Self {
            pinned,
            ..Self::default()
        }
    }

    /// Visible or about to be: pinned, or revealed by hover.
    pub fn visible(&self) -> bool {
        !self.chromeless && (self.pinned || self.shown)
    }

    fn band(ctx: &Context, left_inset: f32) -> Rect {
        let screen = ctx.content_rect();
        Rect::from_min_max(
            pos2(screen.left() + left_inset, screen.top()),
            pos2(screen.right(), TITLE_BAR_H),
        )
    }

    fn update_hover(&mut self, ctx: &Context, band: Rect) {
        let now = ctx.input(|i| i.time);
        let inside = ctx.pointer_hover_pos().is_some_and(|p| band.contains(p));
        let held = ctx.any_popup_open() || (self.shown && ctx.input(|i| i.pointer.any_down()));
        if inside {
            self.shown = true;
            self.left_at = None;
        } else if self.shown && !held {
            let since = *self.left_at.get_or_insert(now);
            let waited = now - since;
            if waited >= HIDE_GRACE {
                self.shown = false;
                self.left_at = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(HIDE_GRACE - waited));
            }
        }
    }

    pub fn show(&mut self, ctx: &Context, bar: &BarContext<'_>) -> Vec<BarAction> {
        let mut actions = Vec::new();
        if self.chromeless {
            return actions;
        }
        let band = Self::band(ctx, bar.left_inset);
        self.update_hover(ctx, band);
        let visible = self.visible();
        let t =
            ctx.animate_bool_with_time(Id::new("titlebar-fade"), visible, motion::secs(ctx, FADE));
        if t <= 0.0 {
            return actions;
        }
        egui::Area::new(Id::new("titlebar"))
            .order(Order::Foreground)
            .fixed_pos(band.min)
            .constrain(false)
            .interactable(visible)
            .show(ctx, |ui| {
                ui.set_opacity(t);
                let tok = Tokens::get(ctx);
                let (rect, drag) = ui.allocate_exact_size(band.size(), Sense::click_and_drag());
                ui.painter()
                    .rect_filled(rect, CornerRadius::ZERO, tok.glass);
                if visible {
                    if drag.double_clicked() {
                        actions.push(BarAction::Window(Caption::ToggleMaximize));
                    } else if drag.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                }
                self.contents(ui, rect, bar, visible, &mut actions);
            });
        actions
    }

    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        bar: &BarContext<'_>,
        live: bool,
        out: &mut Vec<BarAction>,
    ) {
        let row = Rect::from_min_max(rect.min, pos2(rect.right() - 8.0, rect.bottom()));
        let lead = Rect::from_min_max(
            pos2(rect.left() + bar.platform.leading_gutter(), row.top()),
            row.max,
        );
        let mut push = |a: BarAction, clicked: bool| {
            if clicked && live {
                out.push(a);
            }
        };
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(lead)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = ROW_GAP;
                let r = widgets::ghost_button(ui, Icon::Sidebar, "Toggle sidebar");
                push(BarAction::ToggleSidebar, r.clicked());
                let r = widgets::ghost_button(ui, Icon::Library, "Library");
                push(BarAction::Library, r.clicked());
            },
        );
        let tok = Tokens::get(ui.ctx());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            bar.title,
            theme::medium(13.0),
            tok.ink,
        );
        let mut trail_right = row.right();
        let caption = match bar.platform {
            Platform::Windows => {
                let r = Rect::from_min_max(pos2(rect.right() - 3.0 * 46.0, rect.top()), rect.max);
                trail_right = r.left() - ROW_GAP;
                let flush = UiBuilder::new()
                    .max_rect(r)
                    .layout(Layout::left_to_right(Align::Center));
                ui.scope_builder(flush, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    widgets::windows_captions(ui, rect.height(), bar.maximized)
                })
                .inner
            }
            Platform::Gnome => {
                let right = row.right() - 10.0;
                let r = Rect::from_min_max(
                    pos2(right - (3.0 * 24.0 + 2.0 * 8.0), row.top()),
                    pos2(right, row.bottom()),
                );
                trail_right = r.left() - ROW_GAP;
                let circles = UiBuilder::new()
                    .max_rect(r)
                    .layout(Layout::left_to_right(Align::Center));
                ui.scope_builder(circles, |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    widgets::gnome_captions(ui, bar.maximized)
                })
                .inner
            }
            Platform::Mac => None,
        };
        if let Some(c) = caption {
            push(BarAction::Window(c), true);
        }
        let trail = Rect::from_min_max(row.min, pos2(trail_right, row.bottom()));
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(trail)
                .layout(Layout::right_to_left(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = TRAIL_GAP;
                if widgets::icon_button(ui, Icon::Pin, "Pin titlebar open", self.pinned).clicked()
                    && live
                {
                    self.pinned = !self.pinned;
                    push(BarAction::PinChanged(self.pinned), true);
                }
                let r = widgets::icon_button(ui, Icon::Settings, "Settings", false);
                push(BarAction::Settings, r.clicked());
                let r = widgets::icon_button(ui, Icon::Appearance, "Appearance", false);
                push(BarAction::Appearance, r.clicked());
                let r = widgets::icon_button(ui, Icon::Search, "Search", false);
                push(BarAction::Search, r.clicked());
                let r = widgets::icon_button(ui, Icon::SplitDown, "Split down", false);
                push(BarAction::SplitDown, r.clicked());
                let r = widgets::icon_button(ui, Icon::SplitRight, "Split right", false);
                push(BarAction::SplitRight, r.clicked());
            },
        );
    }
}
