//! The reader's floating Dictionary: moved, resized, folded or a page.

use egui::{
    Align, Context, CursorIcon, Id, Layout, Order, Rect, RichText, Sense, Stroke, Ui, UiBuilder,
    Vec2, pos2, vec2,
};
use mareader_dict::Dictionaries;
use mareader_settings::{DictionarySettings, WINDOW_MIN};

use super::page::Page;
use super::{Action, Voice, Window};
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
const PILL: Vec2 = vec2(240.0, HEAD);
/// Room kept between the window and the stage's edges.
const INSET: f32 = 12.0;

/// What one frame of the window needs.
pub struct Frame<'a> {
    pub service: &'a mut Dictionaries,
    pub page: &'a mut Page,
    pub voice: &'a Voice,
    /// Its rect while dragged or resized, saved on release.
    pub live: &'a mut Option<Rect>,
    /// Folded when last drawn; a change moves the rect to suit.
    pub was_folded: &'a mut Option<bool>,
    pub settings: &'a DictionarySettings,
    /// Where it may sit: below the bar, beside any rail.
    pub stage: Rect,
}

/// How long it takes to appear or leave, and to fold.
const APPEAR: f32 = 0.18;
const FOLD: f32 = 0.24;

/// How far it has come in: 0 closed, 1 open; it keeps drawing until 0.
pub fn appearing(ctx: &Context, open: bool) -> f32 {
    let secs = crate::motion::secs(ctx, APPEAR);
    ctx.animate_bool_with_time(Id::new("dict-window-appear"), open, secs)
}

/// Draws the window and returns where; closing, it fades without input.
pub fn show(ctx: &Context, f: Frame<'_>, appear: f32, out: &mut Commands) -> Rect {
    let t = Tokens::get(ctx);
    let w = &f.settings.window;
    let closing = !w.open;
    let saved = w
        .rect
        .map(|[x, y, wd, h]| Rect::from_min_size(pos2(x, y), vec2(wd, h)));
    let folded = w.minimized;
    let mut rect = f.live.or(saved).unwrap_or_else(|| start(f.stage));
    // Folding puts the pill where the button was; unfolding grows from it.
    let turned = f.was_folded.is_some_and(|was| was != folded);
    if turned {
        rect = match folded {
            true => to_pill(rect, f.stage),
            false => from_pill(rect, f.stage),
        };
    }
    *f.was_folded = Some(folded);
    let keep = |r: Rect| match folded {
        true => keep_pill(r, f.stage),
        false => fit(r, f.stage),
    };
    rect = keep(rect);
    let morph = Morph::new(ctx, rect, folded, f.stage);
    let shown = morph.shown();
    let pillish = morph.fold > 0.5;
    let mut released = false;
    let area = egui::Area::new(Id::new("dict-window"))
        .order(Order::Middle)
        .fixed_pos(shown.min)
        .constrain(false)
        .movable(false)
        .interactable(!closing);
    let layer = egui::LayerId::new(Order::Middle, area.layer().id);
    // Coming in it grows a little and rises; leaving, the reverse.
    let grow = 0.96 + 0.04 * appear;
    let anchor = pos2(shown.center().x, shown.top());
    let lift = vec2(0.0, 8.0 * (1.0 - appear));
    let scale = egui::emath::TSTransform::from_translation(anchor.to_vec2() + lift)
        * egui::emath::TSTransform::from_scaling(grow)
        * egui::emath::TSTransform::from_translation(-anchor.to_vec2());
    ctx.set_transform_layer(layer, scale);
    area.show(ctx, |ui| {
        ui.multiply_opacity(appear);
        if closing {
            ui.disable();
        }
        ui.set_min_size(shown.size());
        ui.set_max_size(shown.size());
        paint(ui, shown, morph.fold, &t);
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
        let named = Named {
            word: f.page.text().trim(),
            glance: f.page.glance().filter(|_| pillish),
        };
        if folded {
            grab.clone().on_hover_text("Click to restore");
        }
        // The head fades through its middle as it turns into the pill.
        let head_shown = (2.0 * morph.fold - 1.0).abs();
        ui.scope(|ui| {
            ui.multiply_opacity(head_shown);
            header(ui, head, pillish, &named, &t, out);
        });
        if folded {
            // Still asked, so the pill has its glance.
            f.page.ask(ui.ctx(), f.service, f.settings);
        }
        let body_shown = (1.0 - 2.0 * morph.fold).max(0.0);
        if body_shown <= 0.0 {
            return;
        }
        ui.multiply_opacity(body_shown);
        let line = Stroke::new(1.0, t.line);
        ui.painter().hline(shown.x_range(), head.bottom(), line);
        // Laid out at the open size and cut to what shows, so it never reflows.
        let open = morph.open;
        let body = Rect::from_min_max(
            pos2(shown.left() + 12.0, head.bottom() + 10.0),
            pos2(
                shown.left() + open.width() - 12.0,
                shown.top() + open.height() - 10.0,
            ),
        );
        let cut = Rect::from_min_max(head.left_bottom(), shown.max).shrink(1.0);
        let builder = UiBuilder::new()
            .max_rect(body)
            .layout(Layout::top_down(Align::Min));
        ui.scope_builder(builder, |ui| {
            ui.set_clip_rect(body.expand(2.0).intersect(cut));
            f.page.compact(ui, f.service, (f.settings, f.voice), out);
        });
        // Last, so the edges win over what lies under them.
        if morph.fold <= 0.0 && !closing {
            released |= edges(ui, shown, &mut rect);
        }
    });
    if closing {
        f.page.hide();
        return shown;
    }
    let rect = keep(rect);
    if released || turned {
        *f.live = None;
        out.push(Command::Dict(Action::Window(Window::Place(rect))));
    } else if Some(rect) != saved {
        *f.live = Some(rect);
    }
    shown
}

/// The window turning into its pill: both ends, and `fold` from 0 to 1.
struct Morph {
    open: Rect,
    pill: Rect,
    fold: f32,
}

impl Morph {
    fn new(ctx: &Context, rect: Rect, folded: bool, stage: Rect) -> Self {
        let secs = crate::motion::secs(ctx, FOLD);
        let id = Id::new("dict-window-fold");
        let fold = ctx.animate_bool_with_time(id, folded, secs);
        // Each end is kept as last seen, so the turn starts where it was.
        let kept = |which: &str| ctx.data(|d| d.get_temp::<Rect>(id.with(which)));
        let (open, pill) = match folded {
            true => {
                let pill = Rect::from_min_size(rect.min, PILL);
                (kept("open").unwrap_or_else(|| from_pill(rect, stage)), pill)
            }
            false => {
                let pill = Rect::from_min_size(to_pill(rect, stage).min, PILL);
                (rect, kept("pill").unwrap_or(pill))
            }
        };
        let open_size = rect.size().max(Vec2::from(WINDOW_MIN));
        let open = Rect::from_min_size(open.min, open_size);
        ctx.data_mut(|d| {
            if fold <= 0.0 {
                d.insert_temp(id.with("open"), open);
            }
            if fold >= 1.0 {
                d.insert_temp(id.with("pill"), pill);
            }
        });
        Self { open, pill, fold }
    }

    fn shown(&self) -> Rect {
        let e = ease(self.fold);
        Rect::from_min_max(
            self.open.min.lerp(self.pill.min, e),
            self.open.max.lerp(self.pill.max, e),
        )
    }
}

/// Fast out, soft landing.
fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

/// Shadow, paper and hairline, rounding as it folds into the pill.
fn paint(ui: &Ui, r: Rect, fold: f32, t: &Tokens) {
    let radius = egui::emath::lerp(t.radius_lg..=HEAD / 2.0, ease(fold));
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

/// What the folded pill names: the word, and its top answer at a glance.
struct Named<'a> {
    word: &'a str,
    glance: Option<&'a str>,
}

/// Icon and title, then open as a page, fold and close.
fn header(ui: &mut Ui, head: Rect, folded: bool, n: &Named<'_>, t: &Tokens, out: &mut Commands) {
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
            // Folded, it names the word last searched and its answer.
            let title = match folded && !n.word.is_empty() {
                true => n.word,
                false => "Dictionary",
            };
            let title = RichText::new(title).font(crate::theme::medium(t.text));
            let Some(glance) = n.glance else {
                crate::shaped::line(ui, title, t.text, t.ink);
                return;
            };
            // The word keeps at most half, the answer the rest, on one line.
            let half = vec2(
                (ui.available_width() * 0.5).max(40.0),
                ui.available_height(),
            );
            let lead = Layout::left_to_right(Align::Center);
            ui.allocate_ui_with_layout(half, lead, |ui| {
                crate::shaped::line(ui, title, t.text, t.ink);
            });
            crate::shaped::line(ui, glance, t.text_small, t.muted);
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

/// Folded, the rect starts at the pill: top right on the right half.
pub fn to_pill(r: Rect, stage: Rect) -> Rect {
    let right = r.center().x > stage.center().x;
    let x = if right { r.right() - PILL.x } else { r.left() };
    Rect::from_min_size(pos2(x, r.top()), r.size())
}

/// Unfolded, it grows from the pill away from the nearer edges.
pub fn from_pill(r: Rect, stage: Rect) -> Rect {
    let pill = Rect::from_min_size(r.min, PILL);
    let x = match pill.center().x > stage.center().x {
        true => pill.right() - r.width(),
        false => pill.left(),
    };
    let y = match pill.center().y > stage.center().y {
        true => pill.bottom() - r.height(),
        false => pill.top(),
    };
    fit(Rect::from_min_size(pos2(x, y), r.size()), stage)
}

/// The pill wholly on the stage, by its own size; the window's size kept.
pub fn keep_pill(r: Rect, stage: Rect) -> Rect {
    let room = stage.shrink(INSET);
    let far = (room.max - PILL).max(room.min);
    let min = r.min.clamp(room.min, far);
    Rect::from_min_size(min, r.size())
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

    #[test]
    fn the_pill_reaches_every_edge_and_unfolds_onto_the_stage() {
        let stage = Rect::from_min_size(pos2(0.0, 40.0), vec2(1000.0, 700.0));
        let room = stage.shrink(INSET);
        let window = Rect::from_min_size(pos2(600.0, 60.0), START);
        // Folding on the right half keeps the pill under the − button.
        let folded = to_pill(window, stage);
        assert_eq!(folded.min.x + PILL.x, window.right());
        // Dragged into the bottom right corner, the pill gets there.
        let far = keep_pill(folded.translate(vec2(2000.0, 2000.0)), stage);
        assert_eq!(far.min + PILL, room.max);
        // And grows up and to the left from there, wholly on the stage.
        let open = from_pill(far, stage);
        assert!(stage.contains_rect(open), "{open:?}");
        assert_eq!(open.size(), START);
        assert_eq!(open.max, room.max);
    }
}
