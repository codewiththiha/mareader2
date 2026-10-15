//! The breadcrumb in the title bar: the way back up from an open shelf.

use egui::text::{CCursor, CCursorRange};
use egui::{Button, Id, Popup, PopupCloseBehavior, RectAlign, Response, Stroke, StrokeKind, Ui};
use mareader_library::{NAME_MAX, ShelfId};

use super::living::OPEN_DWELL;
use super::{Action, Carry, Crumb, ShelfAction};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

pub const CRUMB_MENU: &str = "library-crumb-menu";
const ELIDED_MENU: &str = "library-crumb-elided";
/// Past this many crumbs, the middle ones fold into "…".
const SHOWN: usize = 4;
/// A long name is cut, so the title bar keeps its buttons.
const CRUMB_MAX: f32 = 160.0;
const MENU_WIDTH: f32 = 200.0;

/// The trail from the top level; the last crumb is the open shelf.
pub fn show(ui: &mut Ui, trail: &[Crumb], renaming: Option<ShelfId>, out: &mut Commands) {
    let Some((current, above)) = trail.split_last() else {
        return;
    };
    // Long trails keep the top and the last two levels.
    let cut = above.len().saturating_sub(SHOWN - 1);
    let (elided, kept) = match above.split_first() {
        Some((top, rest)) if cut > 0 => {
            let (elided, kept) = rest.split_at(cut.min(rest.len()));
            (elided, std::iter::once(top).chain(kept).collect::<Vec<_>>())
        }
        _ => (&[][..], above.iter().collect()),
    };
    ui.spacing_mut().item_spacing.x = 2.0;
    for (n, crumb) in kept.iter().enumerate() {
        if n > 0 {
            chevron(ui);
        }
        let r = crumb_button(ui, crumb, false);
        if r.clicked() {
            out.push(go(crumb.id));
        }
        take_drops(ui, &r, crumb.id, out);
        if n == 0 && !elided.is_empty() {
            chevron(ui);
            folded(ui, elided, out);
        }
    }
    chevron(ui);
    match (renaming, current.id) {
        (Some(editing), Some(id)) if editing == id => rename(ui, id, &current.name, out),
        _ => current_menu(ui, current, out),
    }
}

fn go(id: Option<ShelfId>) -> Command {
    Command::Library(Action::Shelf(ShelfAction::GoTo(id)))
}

fn chevron(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.add(Icon::ChevronRight.image(14.0).tint(t.muted));
}

/// A crumb: the top level shows the library's mark beside its name.
fn crumb_button(ui: &mut Ui, crumb: &Crumb, open: bool) -> Response {
    ui.scope(|ui| {
        ui.set_max_width(CRUMB_MAX);
        let button = match crumb.id {
            None => Button::image_and_text(Icon::Library.image(16.0), crumb.name.as_str())
                .image_tint_follows_text_color(true),
            Some(_) => Button::new(crumb.name.as_str()),
        };
        ui.add(button.truncate().frame_when_inactive(open).selected(open))
    })
    .inner
}

/// A book or shelf dropped on a crumb moves there.
fn take_drops(ui: &Ui, r: &Response, id: Option<ShelfId>, out: &mut Commands) {
    let keep = ui.input(|i| i.modifiers.alt);
    let hovered = r.dnd_hover_payload::<Carry>();
    if hovered.as_ref().is_some_and(|c| c.onto(id, keep).is_some()) {
        let t = Tokens::get(ui.ctx());
        let ring = Stroke::new(2.0, t.accent);
        ui.painter()
            .rect_stroke(r.rect, t.radius, ring, StrokeKind::Inside);
    }
    if hovered.is_some() {
        rest_to_go(ui, id, out);
    }
    if let Some(carry) = r.dnd_release_payload::<Carry>() {
        out.extend(carry.onto(id, keep).map(Command::Library));
    }
}

/// A carry resting on a crumb goes there, to be dropped in place.
fn rest_to_go(ui: &Ui, id: Option<ShelfId>, out: &mut Commands) {
    let key = Id::new("library-crumb-rest");
    let (now, pass) = (ui.input(|i| i.time), ui.ctx().cumulative_pass_nr());
    let since = ui.data_mut(|d| {
        let rest = d.get_temp_mut_or_insert_with(key, || (id, now, pass));
        // Another crumb, or a gap since the last frame: a fresh rest.
        if rest.0 != id || rest.2 + 1 < pass {
            *rest = (id, now, pass);
        }
        rest.2 = pass;
        rest.1
    });
    let left = OPEN_DWELL - (now - since);
    if left > 0.0 {
        ui.ctx().request_repaint_after_secs(left as f32);
        return;
    }
    ui.data_mut(|d| d.remove::<(Option<ShelfId>, f64, u64)>(key));
    out.push(go(id));
}

/// The folded middle of a long trail, listed in a menu.
fn folded(ui: &mut Ui, elided: &[Crumb], out: &mut Commands) {
    let r = ui.add(Button::new("…").frame_when_inactive(false));
    let r = r.on_hover_text("Shelves between");
    Popup::from_toggle_button_response(&r)
        .id(Id::new(ELIDED_MENU))
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .align(RectAlign::BOTTOM_START)
        .gap(6.0)
        .width(MENU_WIDTH)
        .show(|ui| {
            ui.set_width(MENU_WIDTH);
            for crumb in elided {
                if widgets::menu_row(ui, Icon::Shelf, &crumb.name, None, false).clicked() {
                    out.push(go(crumb.id));
                }
            }
        });
}

/// The open shelf's crumb: a menu to rename or remove it.
fn current_menu(ui: &mut Ui, current: &Crumb, out: &mut Commands) {
    let menu = Id::new(CRUMB_MENU);
    let open = Popup::is_id_open(ui.ctx(), menu);
    let r = crumb_button(ui, current, open);
    take_drops(ui, &r, current.id, out);
    let Some(id) = current.id else {
        return;
    };
    let act = |a: ShelfAction| Command::Library(Action::Shelf(a));
    Popup::from_toggle_button_response(&r)
        .id(menu)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .align(RectAlign::BOTTOM_START)
        .gap(6.0)
        .width(MENU_WIDTH)
        .show(|ui| {
            ui.set_width(MENU_WIDTH);
            if widgets::menu_row(ui, Icon::Rename, "Rename…", None, false).clicked() {
                out.push(act(ShelfAction::StartRename(id)));
            }
            if widgets::menu_row(ui, Icon::Remove, "Remove shelf…", None, false).clicked() {
                out.push(act(ShelfAction::AskRemove(id)));
            }
        });
}

/// Focuses a fresh name field with all of it selected.
pub(super) fn select_all(field: &egui::Response, id: Id, text: &str) {
    field.request_focus();
    let mut state = egui::TextEdit::load_state(&field.ctx, id).unwrap_or_default();
    let all = CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()));
    state.cursor.set_char_range(Some(all));
    state.store(&field.ctx, id);
}

/// The name in place: Enter or a click away keeps it, Escape doesn't.
fn rename(ui: &mut Ui, id: ShelfId, name: &str, out: &mut Commands) {
    let field = Id::new("library-shelf-name").with(id);
    let draft = ui.data(|d| d.get_temp::<String>(field));
    let fresh = draft.is_none();
    let mut text = draft.unwrap_or_else(|| name.to_owned());
    let edit = egui::TextEdit::singleline(&mut text)
        .id(field)
        .char_limit(NAME_MAX)
        .desired_width(CRUMB_MAX + 20.0);
    let r = ui.add(edit);
    if fresh {
        // The whole name selected, so typing replaces it.
        select_all(&r, field, &text);
    }
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let done = escape || (r.lost_focus() && !fresh);
    if !done {
        ui.data_mut(|d| d.insert_temp(field, text));
        return;
    }
    ui.data_mut(|d| d.remove::<String>(field));
    let action = match escape {
        true => ShelfAction::CancelRename,
        false => ShelfAction::Rename(id, text),
    };
    out.push(Command::Library(Action::Shelf(action)));
}
