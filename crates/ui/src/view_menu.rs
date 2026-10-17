//! The title bar's menu: crowded-out tools, full screen, the shortcut list.

use egui::{
    Align, Button, Id, Layout, Popup, PopupCloseBehavior, Rect, RectAlign, RichText, Sense, Ui,
    pos2, vec2,
};

use crate::command::{Command, Commands, WindowCommand};
use crate::documents::Placement;
use crate::icons::Icon;
use crate::keymap;
use crate::layout::ViewMode;
use crate::platform::Platform;
use crate::theme::Tokens;
use crate::widgets;
use crate::zoom::{Fit, Step, ZoomCommand, ZoomState};

pub const VIEW_MENU: &str = "view-menu";
const WIDTH: f32 = 272.0;
const KEYS_OPEN: &str = "view-menu-keys";

/// A title bar tool; folds into the menu when the bar runs out of room.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub icon: Icon,
    pub label: &'static str,
    pub command: Command,
}

pub struct MenuContext<'a> {
    /// Tools the bar had no room for, in bar order.
    pub folded: &'a [Tool],
    pub fullscreen: bool,
    pub platform: Platform,
    /// The active document's zoom and mode; no rows without one.
    pub zoom: Option<ZoomState>,
    pub mode: Option<ViewMode>,
    pub auto_scroll: Option<bool>,
    /// The title bar stays shown instead of waiting for a hover.
    pub pinned: bool,
}

/// The menu button and its popup.
pub fn button(ui: &mut Ui, cx: &MenuContext<'_>, out: &mut Commands) {
    let id = Id::new(VIEW_MENU);
    let open = Popup::is_id_open(ui.ctx(), id);
    let r = widgets::icon_button(ui, Icon::Menu, "View & tools", open);
    Popup::from_toggle_button_response(&r)
        .id(id)
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_END)
        .gap(6.0)
        .width(WIDTH)
        .show(|ui| contents(ui, cx, out));
}

fn contents(ui: &mut Ui, cx: &MenuContext<'_>, out: &mut Commands) {
    ui.set_width(WIDTH);
    ui.spacing_mut().item_spacing.y = 2.0;
    // Splitting is a drag from the Library panel, so Open… is the only row.
    let command = Command::ShowOpenDialog(Placement::Into);
    let keys = keymap::hint(cx.platform, &command);
    if widgets::menu_row(ui, Icon::Open, "Open…", keys.as_deref(), false).clicked() {
        out.push(command);
        ui.close();
    }
    ui.separator();
    // Layout, zoom and reading each under a name of their own.
    if let Some(mode) = cx.mode {
        heading(ui, "Layout");
        view_row(ui, mode, cx.platform, out);
    }
    if let Some(zoom) = cx.zoom {
        heading(ui, "Zoom");
        zoom_row(ui, zoom, cx.platform, out);
    }
    heading(ui, "Reading");
    if let (Some(on), Some(mode)) = (cx.auto_scroll, cx.mode) {
        auto_scroll_row(ui, on, !mode.paged(), cx.platform, out);
    }
    let full = Command::Window(WindowCommand::ToggleFullscreen);
    let (icon, label) = if cx.fullscreen {
        (Icon::ExitFullscreen, "Exit full screen")
    } else {
        (Icon::Fullscreen, "Full screen")
    };
    let keys = keymap::hint(cx.platform, &full);
    if widgets::menu_row(ui, icon, label, keys.as_deref(), false).clicked() {
        out.push(full);
        ui.close();
    }
    // The bar it sits on hides too, so it only ever turns it on.
    let keys = keymap::hint(cx.platform, &Command::Chromeless);
    let label = "Distraction-free";
    if widgets::menu_row(ui, Icon::Chromeless, label, keys.as_deref(), false).clicked() {
        out.push(Command::Chromeless);
        ui.close();
    }
    pin_row(ui, cx.pinned, out);
    ui.separator();
    for tool in cx.folded {
        let keys = keymap::hint(cx.platform, &tool.command);
        if widgets::menu_row(ui, tool.icon, tool.label, keys.as_deref(), false).clicked() {
            out.push(tool.command.clone());
            ui.close();
        }
    }
    let keys = keymap::hint(cx.platform, &Command::ShowSettings);
    if widgets::menu_row(ui, Icon::Settings, "Settings", keys.as_deref(), false).clicked() {
        out.push(Command::ShowSettings);
        ui.close();
    }
    shortcuts(ui, cx.platform);
    ui.separator();
    let t = Tokens::get(ui.ctx());
    let version = format!("Mareader {}", env!("CARGO_PKG_VERSION"));
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(version).size(t.text_small).color(t.muted));
    });
}

/// Keeps the title bar shown; both routes' menus offer it.
pub fn pin_row(ui: &mut Ui, pinned: bool, out: &mut Commands) {
    let label = "Keep title bar shown";
    if widgets::menu_row(ui, Icon::Pin, label, None, pinned).clicked() {
        out.push(Command::SetTitlebarPinned(!pinned));
    }
}

/// Auto-scroll's switch, dimmed in the paged modes, as in mareader.
fn auto_scroll_row(ui: &mut Ui, on: bool, can: bool, platform: Platform, out: &mut Commands) {
    let keys = keymap::hint(platform, &Command::AutoScroll);
    ui.add_enabled_ui(can, |ui| {
        let row = widgets::menu_row(ui, Icon::AutoScroll, "Auto-scroll", keys.as_deref(), on);
        if row.clicked() {
            out.push(Command::AutoScroll);
            ui.close();
        }
    });
}

/// A small muted name over a group of rows.
fn heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(text).size(t.text_small).color(t.muted));
    });
}

/// The four view modes, each named under its icon; the menu stays open.
fn view_row(ui: &mut Ui, current: ViewMode, platform: Platform, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), 46.0);
    let (band, _) = ui.allocate_exact_size(size, Sense::hover());
    let band = band.shrink2(vec2(4.0, 0.0));
    let slot = band.width() / ViewMode::ALL.len() as f32;
    for (i, mode) in ViewMode::ALL.into_iter().enumerate() {
        let icon = match mode {
            ViewMode::Single => Icon::SinglePage,
            ViewMode::Spread => Icon::Spread,
            ViewMode::Vertical => Icon::ScrollVertical,
            ViewMode::Horizontal => Icon::ScrollHorizontal,
        };
        let cell = Rect::from_min_size(
            pos2(band.left() + slot * i as f32, band.top()),
            vec2(slot, band.height()),
        )
        .shrink(1.0);
        let on = mode == current;
        let label = mode.label();
        let r = ui.interact(cell, ui.id().with(("view-mode", label)), Sense::click());
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
        let command = Command::View(mode);
        let keys = keymap::hint(platform, &command);
        widgets::tip(&r, label, keys.as_deref());
        let fill = if on {
            t.accent_soft
        } else if r.hovered() {
            t.hover
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(cell, t.radius, fill);
        let ink = if on { t.accent } else { t.ink };
        let mark = Rect::from_center_size(cell.center() - vec2(0.0, 8.0), egui::Vec2::splat(16.0));
        icon.paint(ui, mark, ink);
        let at = cell.center() + vec2(0.0, 11.0);
        let font = egui::FontId::proportional(11.0);
        ui.painter()
            .text(at, egui::Align2::CENTER_CENTER, mode.short(), font, ink);
        if r.clicked() {
            out.push(command);
        }
    }
}

/// Zoom steps, then the fits; the menu stays open so presses chain.
fn zoom_row(ui: &mut Ui, zoom: ZoomState, platform: Platform, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(4.0);
        let steps = [
            (Step::Out, Icon::ZoomOut, "Zoom out"),
            (Step::In, Icon::ZoomIn, "Zoom in"),
        ];
        for (i, (step, icon, label)) in steps.into_iter().enumerate() {
            if i == 1 {
                let percent = RichText::new(zoom.percent()).size(t.text_small);
                ui.add_sized([52.0, 28.0], egui::Label::new(percent));
            }
            let command = Command::Zoom(ZoomCommand::Step(step));
            let keys = keymap::hint(platform, &command);
            let r = ui.add_enabled_ui(zoom.can_step(step), |ui| {
                widgets::icon_button_keyed(ui, icon, label, false, keys.as_deref())
            });
            if r.inner.clicked() {
                out.push(command);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(4.0);
            let fits = [
                (Fit::Page, Icon::FitPage, "Fit page"),
                (Fit::Width, Icon::FitWidth, "Fit width"),
            ];
            for (fit, icon, label) in fits {
                let command = Command::Zoom(ZoomCommand::Fit(fit));
                let keys = keymap::hint(platform, &command);
                let on = zoom.fit == Some(fit);
                if widgets::icon_button_keyed(ui, icon, label, on, keys.as_deref()).clicked() {
                    out.push(command);
                }
            }
        });
    });
}

/// A disclosure row; the list stays open between visits to the menu.
fn shortcuts(ui: &mut Ui, platform: Platform) {
    let t = Tokens::get(ui.ctx());
    let id = Id::new(KEYS_OPEN);
    let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let row = Button::image_and_text(Icon::Keyboard.image(16.0), "Keyboard shortcuts")
        .right_text(chevron.image(14.0))
        .image_tint_follows_text_color(true)
        .frame_when_inactive(false)
        .min_size(egui::vec2(ui.available_width(), 32.0));
    let r = ui.add(row);
    r.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, open, "Keyboard shortcuts")
    });
    if r.clicked() {
        ui.data_mut(|d| d.insert_temp(id, !open));
    }
    if !open {
        return;
    }
    for (label, keys) in keymap::reference(platform) {
        ui.horizontal(|ui| {
            ui.set_min_height(26.0);
            ui.add_space(10.0);
            ui.label(RichText::new(label).size(t.text_small));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(10.0);
                ui.label(RichText::new(keys).size(t.text_small).color(t.muted));
            });
        });
    }
}
