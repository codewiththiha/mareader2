//! The reader's floating Dictionary: moved, resized, folded or a page.

use egui::{
    Align, Context, CursorIcon, Id, Layout, Order, Rect, RichText, Sense, Stroke, Ui, UiBuilder,
    Vec2, pos2, vec2,
};
use mareader_dict::Dictionaries;
use mareader_settings::{DictionarySettings, WINDOW_MIN};

use super::page::Page;
use super::{Action, Window};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

/// The controls' names, for screen readers and tests.
pub const MINIMIZE: &str = "Minimize the dictionary";
pub const RESTORE: &str = "Restore the dictionary";
pub const CLOSE: &str = "Close the dictionary";
pub const EXPAND: &str = "Open as a page";

/// Its size on first opening.
const START: Vec2 = vec2(380.0, 520.0);
const HEAD: f32 = 38.0;
/// The band along each edge that resizes.
const GRIP: f32 = 6.0;
const PILL_W: f32 = 240.0;
/// Room kept between the window and the stage's edges.
const INSET: f32 = 12.0;

/// What one frame of the window needs.
pub struct Frame<'a> {
    pub service: &'a mut Dictionaries,
    pub page: &'a mut Page,
    /// Its rect while dragged or resized, saved on release.
    pub live: &'a mut Option<Rect>,
    pub settings: &'a DictionarySettings,
    /// Where it may sit: below the bar, beside any rail.
    pub stage: Rect,
}

/// Draws the window; returns where it was drawn.
pub fn show(ctx: &Context, f: Frame<'_>, out: &mut Commands) -> Rect {
    let t = Tokens::get(ctx);
    let w = &f.settings.window;
    let saved = w
        .rect
        .map(|[x, y, wd, h]| Rect::from_min_size(pos2(x, y), vec2(wd, h)));
    let mut rect = fit(f.live.or(saved).unwrap_or_else(|| start(f.stage)), f.stage);
    let folded = w.minimized;
    let shown = match folded {
        true => Rect::from_min_size(rect.min, vec2(PILL_W, HEAD)),
        false => rect,
    };
    let mut released = false;
    egui::Area::new(Id::new("dict-window"))
        .order(Order::Middle)
        .fixed_pos(shown.min)
        .constrain(false)
        .movable(false)
        .show(ctx, |ui| {
            ui.set_min_size(shown.size());
            ui.set_max_size(shown.size());
            paint(ui, shown, folded, &t);
            let head = Rect::from_min_size(shown.min, vec2(shown.width(), HEAD));
            let grab = ui.interact(head, Id::new("dict-window-head"), Sense::click_and_drag());
            if grab.dragged() {
                rect = rect.translate(grab.drag_delta());
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            }
            released |= grab.drag_stopped();
            // The pill restores on a click; the head folds on a double.
            if folded && grab.clicked() {
                out.push(Command::Dict(Action::Window(Window::Minimize(false))));
            } else if !folded && grab.double_clicked() {
                out.push(Command::Dict(Action::Window(Window::Minimize(true))));
            }
            header(ui, head, folded, f.page.text(), &t, out);
            if folded {
                return;
            }
            let line = Stroke::new(1.0, t.line);
            ui.painter().hline(shown.x_range(), head.bottom(), line);
            let body = Rect::from_min_max(
                pos2(shown.left() + 12.0, head.bottom() + 10.0),
                pos2(shown.right() - 12.0, shown.bottom() - 10.0),
            );
            let builder = UiBuilder::new()
                .max_rect(body)
                .layout(Layout::top_down(Align::Min));
            ui.scope_builder(builder, |ui| {
                ui.set_clip_rect(body.expand(2.0));
                f.page.compact(ui, f.service, f.settings, out);
            });
            // Last, so the edges win over what lies under them.
            released |= edges(ui, shown, &mut rect);
        });
    let rect = fit(rect, f.stage);
    if released {
        *f.live = None;
        out.push(Command::Dict(Action::Window(Window::Place(rect))));
    } else if Some(rect) != saved {
        *f.live = Some(rect);
    }
    shown
}

/// The card behind it: a soft shadow, the paper and a hairline.
fn paint(ui: &Ui, r: Rect, folded: bool, t: &Tokens) {
    let radius = if folded { HEAD / 2.0 } else { t.radius_lg };
    let shadow = egui::Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: t.shadow,
    };
    let painter = ui.painter();
    painter.add(shadow.as_shape(r, radius));
    painter.rect(
        r,
        radius,
        t.paper,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
}

/// Icon and title, then open as a page, fold and close.
fn header(ui: &mut Ui, head: Rect, folded: bool, word: &str, t: &Tokens, out: &mut Commands) {
    let row = head.shrink2(vec2(10.0, 0.0));
    let builder = UiBuilder::new()
        .max_rect(row)
        .layout(Layout::right_to_left(Align::Center));
    ui.scope_builder(builder, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        // The title is part of the handle, not text to select.
        ui.style_mut().interaction.selectable_labels = false;
        let send = |w: Window| Command::Dict(Action::Window(w));
        if widgets::small_icon_button(ui, Icon::Close, CLOSE).clicked() {
            out.push(send(Window::Close));
        }
        let (icon, label) = match folded {
            true => (Icon::ChevronUp, RESTORE),
            false => (Icon::Minus, MINIMIZE),
        };
        if widgets::small_icon_button(ui, icon, label).clicked() {
            out.push(send(Window::Minimize(!folded)));
        }
        if !folded && widgets::small_icon_button(ui, Icon::Fullscreen, EXPAND).clicked() {
            out.push(send(Window::Expand));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add(Icon::Dictionary.image(16.0).tint(t.accent));
            // Folded, it names the word last searched.
            let word = word.trim();
            let title = if folded && !word.is_empty() {
                word
            } else {
                "Dictionary"
            };
            let title = RichText::new(title).font(crate::theme::medium(t.text));
            crate::shaped::line(ui, title, t.text, t.ink);
        });
    });
}

/// The eight resize bands; true when one was let go.
fn edges(ui: &Ui, r: Rect, rect: &mut Rect) -> bool {
    let mut released = false;
    let sides = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    for (i, (dx, dy)) in sides.into_iter().enumerate() {
        let zone = band(r, dx, dy);
        let resp = ui.interact(zone, Id::new(("dict-window-edge", i)), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(cursor(dx, dy));
        }
        if resp.dragged() {
            *rect = stretch(*rect, dx, dy, resp.drag_delta());
        }
        released |= resp.drag_stopped();
    }
    released
}

/// `r` with the named sides moved by `d`, kept at least the minimum.
fn stretch(mut r: Rect, dx: i8, dy: i8, d: Vec2) -> Rect {
    let [min_w, min_h] = WINDOW_MIN;
    match dx {
        -1 => r.min.x = (r.min.x + d.x).min(r.max.x - min_w),
        1 => r.max.x = (r.max.x + d.x).max(r.min.x + min_w),
        _ => {}
    }
    match dy {
        -1 => r.min.y = (r.min.y + d.y).min(r.max.y - min_h),
        1 => r.max.y = (r.max.y + d.y).max(r.min.y + min_h),
        _ => {}
    }
    r
}

/// The strip inside `r` along the sides `dx` and `dy` name.
fn band(r: Rect, dx: i8, dy: i8) -> Rect {
    let span = |d: i8, lo: f32, hi: f32| match d {
        -1 => (lo, lo + GRIP),
        1 => (hi - GRIP, hi),
        _ => (lo + GRIP, hi - GRIP),
    };
    let (x0, x1) = span(dx, r.left(), r.right());
    let (y0, y1) = span(dy, r.top(), r.bottom());
    Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
}

fn cursor(dx: i8, dy: i8) -> CursorIcon {
    match (dx, dy) {
        (-1, -1) | (1, 1) => CursorIcon::ResizeNwSe,
        (1, -1) | (-1, 1) => CursorIcon::ResizeNeSw,
        (_, 0) => CursorIcon::ResizeHorizontal,
        _ => CursorIcon::ResizeVertical,
    }
}

/// First opened: at the stage's top right, beside the page.
fn start(stage: Rect) -> Rect {
    let min = pos2(stage.right() - START.x - INSET, stage.top() + INSET);
    Rect::from_min_size(min, START)
}

/// `r` no larger than the stage and wholly on it.
pub fn fit(r: Rect, stage: Rect) -> Rect {
    let room = stage.shrink(INSET);
    let floor = Vec2::from(WINDOW_MIN).min(room.size().max(Vec2::ZERO));
    let size = r.size().max(floor).min(room.size().max(floor));
    let far_x = (room.max.x - size.x).max(room.min.x);
    let far_y = (room.max.y - size.y).max(room.min.y);
    let x = r.min.x.clamp(room.min.x, far_x);
    let y = r.min.y.clamp(room.min.y, far_y);
    Rect::from_min_size(pos2(x, y), size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_stays_on_the_stage_and_never_below_its_minimum() {
        let stage = Rect::from_min_size(pos2(0.0, 40.0), vec2(1000.0, 700.0));
        let off = Rect::from_min_size(pos2(900.0, -50.0), vec2(500.0, 100.0));
        let r = fit(off, stage);
        assert!(stage.contains_rect(r), "{r:?}");
        assert_eq!(r.height(), WINDOW_MIN[1]);
        let shrunk = stretch(r, -1, 1, vec2(400.0, -400.0));
        assert_eq!(shrunk.size(), Vec2::from(WINDOW_MIN));
        assert_eq!(shrunk.max.x, r.max.x, "the right edge held");
    }
}
