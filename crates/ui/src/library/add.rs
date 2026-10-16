//! Adding books: the plus card, its one menu, and how a folder is kept.

use egui::{
    Align2, Color32, CornerRadius, FontId, Id, Popup, PopupCloseBehavior, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};
use mareader_library::{Book, BookId, Library, ShelfId, Shelving};

use super::io::Walk;
use super::{Action, BookAction, Scope, ShelfAction};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::{self, Tokens};

pub const ADD_MENU: &str = "library-add";
const WIDTH: f32 = 300.0;
const ROW_H: f32 = 46.0;
/// Removed books listed by name before "Restore all".
const RESTORE_ROWS: usize = 5;

/// How a folder's books are kept, asked when one is added.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FolderKeep {
    Copy,
    #[default]
    InPlace,
    /// Read in place, and walked again when the window comes back.
    Watch,
}

/// A folder's keeping, which of its files count, and their shelves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderPlan {
    pub keep: FolderKeep,
    pub walk: Walk,
}

impl Default for FolderPlan {
    /// Read in place, a shelf for each folder.
    fn default() -> Self {
        let walk = Walk {
            shelving: Shelving::Each,
            ..Walk::default()
        };
        Self {
            keep: FolderKeep::InPlace,
            walk,
        }
    }
}

/// Where the menu adds: the shelf open, and the folder chip chosen.
#[derive(Clone, Copy)]
pub struct Here<'a> {
    pub model: &'a Library,
    pub at: Option<ShelfId>,
    pub scope: Scope,
}

/// The plus card ending the grid: a book's size, a menu behind it.
pub fn card(ui: &mut Ui, cover: Rect, here: Here<'_>, t: &Tokens, out: &mut Commands) {
    let resp = ui.interact(cover, Id::new("library-add-card"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add books"));
    let open = Popup::is_id_open(ui.ctx(), Id::new(ADD_MENU));
    let lit = resp.hovered() || open;
    let fill = match lit {
        true => t.hover,
        false => t.hover.gamma_multiply(0.45),
    };
    let radius = CornerRadius::same(6);
    ui.painter().rect_filled(cover, radius, fill);
    let edge = if lit { t.muted } else { t.line };
    let stroke = Stroke::new(1.0, edge.gamma_multiply(0.8));
    ui.painter()
        .rect_stroke(cover, radius, stroke, StrokeKind::Inside);
    let size = (cover.width() * 0.22).clamp(20.0, 36.0);
    let mark = Rect::from_center_size(cover.center(), vec2(size, size));
    Icon::Plus.paint(ui, mark, if lit { t.ink } else { t.muted });
    menu(&resp, here, t, out);
}

/// The list's last row: the same menu, in a row's shape.
pub fn row(ui: &mut Ui, rect: Rect, inset: f32, here: Here<'_>, t: &Tokens, out: &mut Commands) {
    let resp = ui.interact(rect, Id::new("library-add-row"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add books"));
    if resp.hovered() || Popup::is_id_open(ui.ctx(), Id::new(ADD_MENU)) {
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    let icon = Rect::from_center_size(
        pos2(rect.left() + inset + 17.0, rect.center().y),
        vec2(18.0, 18.0),
    );
    Icon::Plus.paint(ui, icon, t.muted);
    let at = pos2(icon.right() + 22.0, rect.center().y);
    let font = FontId::proportional(14.0);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, "Add books", font, t.muted);
    menu(&resp, here, t, out);
}

/// The empty library's one button.
pub fn button(ui: &mut Ui, here: Here<'_>, t: &Tokens, out: &mut Commands) {
    let text = RichText::new("Add books").color(Color32::WHITE);
    let button = egui::Button::image_and_text(Icon::Plus.image(16.0).tint(Color32::WHITE), text)
        .fill(t.accent)
        .corner_radius(t.radius)
        .min_size(vec2(132.0, 34.0));
    let resp = ui.add(button);
    menu(&resp, here, t, out);
}

/// One menu for every way books arrive, anchored under `anchor`.
fn menu(anchor: &Response, here: Here<'_>, t: &Tokens, out: &mut Commands) {
    Popup::from_toggle_button_response(anchor)
        .id(Id::new(ADD_MENU))
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .gap(6.0)
        .width(WIDTH)
        .show(|ui| {
            contents(ui, here, t, out);
            crate::widgets::close_on_key(ui);
        });
}

fn contents(ui: &mut Ui, here: Here<'_>, t: &Tokens, out: &mut Commands) {
    ui.set_width(WIDTH);
    ui.spacing_mut().item_spacing.y = 2.0;
    let import = "Copied into the library; the originals can go";
    if entry(ui, Icon::Import, "Import files…", import, t).clicked() {
        out.push(Command::Library(Action::Book(BookAction::PickImport)));
    }
    let folder = "Copy it in, or read it where it is";
    if entry(ui, Icon::FolderPlus, "Add a folder…", folder, t).clicked() {
        out.push(Command::Library(Action::PickFolder));
    }
    ui.separator();
    let (shelf, detail) = match here.at {
        Some(_) => ("New shelf inside", "A shelf on this shelf"),
        None => ("New shelf", "Drag books onto it to file them"),
    };
    if entry(ui, Icon::Shelf, shelf, detail, t).clicked() {
        out.push(Command::Library(Action::Shelf(ShelfAction::New)));
    }
    if let Scope::Folder(id) = here.scope {
        restore(ui, here.model, id, t, out);
    }
}

/// Books removed from the folder shown, each with a way back.
fn restore(
    ui: &mut Ui,
    model: &Library,
    id: mareader_library::SourceId,
    t: &Tokens,
    out: &mut Commands,
) {
    let hidden: Vec<&Book> = model.hidden_in(id);
    if hidden.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Restore").size(11.5).color(t.muted));
    let back = |out: &mut Commands, books: Vec<BookId>| {
        out.push(Command::Library(Action::Book(BookAction::Restore(books))));
    };
    for book in hidden.iter().take(RESTORE_ROWS) {
        let detail = "Removed; its file is still there";
        if entry(ui, Icon::Restore, &book.title.value, detail, t).clicked() {
            back(out, vec![book.id]);
        }
    }
    if hidden.len() > 1 {
        let all = format!("Restore all {}", hidden.len());
        let detail = "Every book removed from this folder";
        if entry(ui, Icon::Restore, &all, detail, t).clicked() {
            back(out, hidden.iter().map(|b| b.id).collect());
        }
    }
}

/// A menu row: an icon, a title and what it does underneath.
fn entry(ui: &mut Ui, icon: Icon, title: &str, detail: &str, t: &Tokens) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    let mark = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(17.0, 17.0));
    icon.paint(ui, mark, t.muted);
    let x = mark.right() + 12.0;
    let room = rect.right() - x - 8.0;
    let line = |text: &str, font: FontId, color: Color32, top: f32| {
        let text = egui::WidgetText::from(RichText::new(text).font(font).color(color));
        let wrap = Some(egui::TextWrapMode::Truncate);
        let galley = text.into_galley(ui, wrap, room.max(16.0), egui::FontSelection::Default);
        ui.painter()
            .galley(pos2(x, rect.top() + top), galley, color);
    };
    line(title, theme::medium(13.5), t.ink, 6.0);
    line(detail, FontId::proportional(12.0), t.muted, 25.0);
    resp
}
