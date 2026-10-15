//! The library's View menu: layout, columns, covers and order.

use egui::{Id, Popup, PopupCloseBehavior, RectAlign, RichText, Ui};
use mareader_library::{COLUMNS_MAX, COLUMNS_MIN, CoverFit, Home, Layout, SortKey, View};

use super::{Action, ShelfAction};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

pub const LIBRARY_VIEW_MENU: &str = "library-view-menu";
const WIDTH: f32 = 248.0;

/// The menu's button; `auto` is the grid's automatic column count.
pub fn button(ui: &mut Ui, view: &View, auto: u8, out: &mut Commands) {
    let id = Id::new(LIBRARY_VIEW_MENU);
    let open = Popup::is_id_open(ui.ctx(), id);
    let r = widgets::icon_button(ui, Icon::ViewOptions, "View options", open);
    Popup::from_toggle_button_response(&r)
        .id(id)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_END)
        .gap(6.0)
        .width(WIDTH)
        .show(|ui| {
            // Every change shows at once; the menu stays open to chain them.
            let mut next = *view;
            if widgets::menu_row(ui, Icon::Plus, "New shelf", None, false).clicked() {
                out.push(Command::Library(Action::Shelf(ShelfAction::New)));
                ui.close();
            }
            ui.separator();
            contents(ui, &mut next, auto);
            if next != *view {
                out.push(Command::SetLibraryView(next));
            }
        });
}

fn contents(ui: &mut Ui, view: &mut View, auto: u8) {
    ui.set_width(WIDTH);
    ui.spacing_mut().item_spacing.y = 2.0;
    heading(ui, "Layout");
    for (layout, icon, label) in [
        (Layout::Grid, Icon::Grid, "Grid"),
        (Layout::List, Icon::List, "List"),
    ] {
        if widgets::menu_row(ui, icon, label, None, view.layout == layout).clicked() {
            view.layout = layout;
        }
    }
    let grid = view.layout == Layout::Grid;
    ui.add_enabled_ui(grid, |ui| columns(ui, view, auto));
    ui.separator();
    heading(ui, "Covers");
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        for (fit, label) in [(CoverFit::Fit, "Fit"), (CoverFit::Crop, "Crop")] {
            let button = egui::Button::selectable(view.cover == fit, label);
            if ui.add_sized([(WIDTH - 16.0) / 2.0, 28.0], button).clicked() {
                view.cover = fit;
            }
        }
    });
    ui.separator();
    heading(ui, "Sort by");
    for key in SortKey::ALL {
        let on = view.sort == key;
        if widgets::menu_row(ui, Icon::Sort, key.label(), None, on).clicked() && !on {
            view.set_sort(key);
        }
    }
    // The reader's own order has no other way round.
    if let Some([up, down]) = view.sort.directions() {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            for (ascending, label) in [(true, up), (false, down)] {
                let button = egui::Button::selectable(view.ascending == ascending, label);
                if ui.add_sized([(WIDTH - 16.0) / 2.0, 28.0], button).clicked() {
                    view.ascending = ascending;
                }
            }
        });
    }
    ui.separator();
    // Shelved books can stay on the top level too, or only on shelves.
    heading(ui, "Library shows");
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        for (home, label) in [
            (Home::Everything, "All books"),
            (Home::Unshelved, "Unshelved"),
        ] {
            let button = egui::Button::selectable(view.home == home, label);
            if ui.add_sized([(WIDTH - 16.0) / 2.0, 28.0], button).clicked() {
                view.home = home;
            }
        }
    });
}

/// Auto, or a pinned count stepped from what Auto shows now.
fn columns(ui: &mut Ui, view: &mut View, auto: u8) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label("Columns");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let shown = view.columns.unwrap_or(auto);
            let more = ui.add_enabled_ui(shown < COLUMNS_MAX, |ui| {
                widgets::small_icon_button(ui, Icon::Plus, "More columns")
            });
            let count = RichText::new(shown.to_string()).strong();
            ui.add_sized([22.0, 24.0], egui::Label::new(count));
            let fewer = ui.add_enabled_ui(shown > COLUMNS_MIN, |ui| {
                widgets::small_icon_button(ui, Icon::Minus, "Fewer columns")
            });
            let auto_on = view.columns.is_none();
            if ui.add(egui::Button::selectable(auto_on, "Auto")).clicked() {
                view.columns = if auto_on { Some(auto) } else { None };
            }
            if more.inner.clicked() {
                view.columns = Some(shown.saturating_add(1).min(COLUMNS_MAX));
            }
            if fewer.inner.clicked() {
                view.columns = Some(shown.saturating_sub(1).max(COLUMNS_MIN));
            }
        });
    });
}

fn heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(text).size(t.text_small).color(t.muted));
    });
    ui.add_space(2.0);
}
