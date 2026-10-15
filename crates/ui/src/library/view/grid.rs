//! The living grid: rows drawn only in view, cards easing to slots.

use egui::epaint::Shadow;
use egui::{
    Color32, CornerRadius, CursorIcon, DragAndDrop, Id, LayerId, Order, Pos2, Rect, Sense, Ui,
    Vec2, vec2,
};

use super::super::add::Here;
use super::super::living::Slot;
use super::super::shelving::{Carry, ShelfAction};
use super::super::{Action, Pick};
use super::{A4, CARD_TEXT, Cx, GAP, Item, add, centred, cover, line, margin, plate, prefetch};
use super::{grid_card, shelf_card};
use crate::command::Command;
use crate::motion;
use crate::theme;

/// The cards in rows; a carried card opens a hole the rest move around.
pub(super) fn show(ui: &mut Ui, items: &[Item<'_>], columns: u8, cx: &mut Cx<'_>) {
    let cols = usize::from(columns.max(1));
    let width = ui.available_width();
    let card = ((width - GAP * (cols - 1) as f32) / cols as f32).max(1.0);
    let height = card * A4 + CARD_TEXT;
    let pitch = vec2(card + GAP, height + GAP);
    let slots = items.len() + usize::from(cx.adding.is_some());
    let rows = slots.div_ceil(cols);
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    cx.motion.begin((cols, card), dt, motion::enabled(ui.ctx()));
    follow_carry(ui, items, cx);
    // The rest: the cards not lifted, as indexes into `items`.
    let rest: Vec<usize> = match &cx.motion.lift {
        Some(lift) => (items.iter().enumerate())
            .filter(|(_, i)| !lift.holds(i.pick()))
            .map(|(n, _)| n)
            .collect(),
        None => Vec::new(),
    };
    let mut area = egui::ScrollArea::vertical()
        .id_salt("library-grid")
        .auto_shrink(false)
        .content_margin(margin(cx));
    if let Some(at) = cx.scroll_to {
        area = area.vertical_scroll_offset(centred(ui, at / cols, pitch.y));
    }
    area.show_rows(ui, pitch.y, rows, |ui, range| {
        let size = vec2(width, range.len() as f32 * pitch.y);
        let (block, _) = ui.allocate_exact_size(size, Sense::hover());
        let origin = block.min - vec2(0.0, range.start as f32 * pitch.y);
        cx.motion.origin = origin;
        cx.motion.card = vec2(card, height);
        let fill = Fill {
            items,
            rest: &rest,
            cols,
            pitch,
            card: vec2(card, height),
        };
        if cx.motion.lift.is_some() {
            carry_over(ui, &fill, cx);
        }
        for slot in range.start * cols..(range.end * cols).min(slots) {
            let target = vec2(
                (slot % cols) as f32 * pitch.x,
                (slot / cols) as f32 * pitch.y,
            );
            let rect = |at: Vec2| Rect::from_min_size(origin + at, fill.card);
            match fill.at(slot, cx) {
                Some(InSlot::Card(item)) => {
                    let at = cx.motion.place(Slot::Card(item.pick()), target);
                    match item {
                        Item::Shelf(shelf) => shelf_card(ui, rect(at), shelf, cx),
                        Item::Book(book) => grid_card(ui, rect(at), book, cx),
                    }
                }
                Some(InSlot::Add(here)) => {
                    let at = cx.motion.place(Slot::Add, target);
                    let cover = Rect::from_min_size(origin + at, vec2(card, card * A4));
                    add::card(ui, cover, here, cx.t, cx.out);
                }
                Some(InSlot::Hole) => {
                    let cover = Rect::from_min_size(origin + target, vec2(card, card * A4));
                    ui.painter()
                        .rect_filled(cover, cx.t.radius_lg, cx.t.hover.gamma_multiply(0.6));
                }
                None => {}
            }
        }
        prefetch(cx.covers, items, range, cols);
    });
    if cx.motion.end() {
        ui.ctx().request_repaint();
    }
}

/// What the grid shows in a slot this frame.
enum InSlot<'a> {
    Card(Item<'a>),
    Hole,
    Add(Here<'a>),
}

/// The grid's shape this frame, for finding what sits where.
struct Fill<'a, 'b> {
    items: &'b [Item<'a>],
    rest: &'b [usize],
    cols: usize,
    pitch: Vec2,
    card: Vec2,
}

impl<'a> Fill<'a, '_> {
    /// What `slot` holds: a card, the hole, the plus card, or nothing.
    fn at(&self, slot: usize, cx: &Cx<'a>) -> Option<InSlot<'a>> {
        let (item, end) = match &cx.motion.lift {
            None => (self.items.get(slot), slot == self.items.len()),
            Some(lift) => {
                let Some(r) = lift.hole.rest_at(slot) else {
                    return Some(InSlot::Hole);
                };
                let item = self.rest.get(r).and_then(|&n| self.items.get(n));
                (item, r == self.rest.len())
            }
        };
        match (item, end) {
            (Some(item), _) => Some(InSlot::Card(*item)),
            (None, true) => cx.adding.map(InSlot::Add),
            (None, false) => None,
        }
    }
}

/// Starts or ends following a carry from this level.
fn follow_carry(ui: &Ui, items: &[Item<'_>], cx: &mut Cx<'_>) {
    let carry = DragAndDrop::payload::<Carry>(ui.ctx());
    let level = cx.arranging.map(|a| a.level);
    match (carry, level) {
        (None, _) => {
            if cx.motion.lift.is_some() {
                cx.motion.put_down();
            }
            cx.motion.grab = None;
        }
        (Some(carry), Some(level)) if cx.motion.lift.is_none() => {
            if let Some(moving) = carry.items_at(level, cx.model) {
                let shown: Vec<Pick> = items.iter().map(|i| i.pick()).collect();
                cx.motion.pick_up(moving, &shown);
            }
        }
        _ => {}
    }
}

/// Moves the hole under the pointer, files on a shelf's middle, and drops.
fn carry_over(ui: &Ui, fill: &Fill<'_, '_>, cx: &mut Cx<'_>) {
    let Some(pointer) = ui.ctx().pointer_latest_pos() else {
        return;
    };
    let view = ui.clip_rect();
    scroll_near_edges(ui, view, pointer);
    if !view.contains(pointer) {
        return;
    }
    let p = pointer - cx.motion.origin;
    let (col, row) = ((p.x / fill.pitch.x).floor(), (p.y / fill.pitch.y).floor());
    let inside = vec2(p.x - col * fill.pitch.x, p.y - row * fill.pitch.y);
    let on_card = inside.x <= fill.card.x && inside.y <= fill.card.y;
    // Where the pointer asks for the hole: beside the card under it.
    let mut want = None;
    if let Some(lift) = &cx.motion.lift
        && (0.0..fill.cols as f32).contains(&col)
        && row >= 0.0
    {
        let slot = row as usize * fill.cols + col as usize;
        let across = inside.x / fill.card.x;
        let rest = lift.hole.rest_at(slot);
        let item = rest
            .and_then(|r| fill.rest.get(r))
            .and_then(|&n| fill.items.get(n));
        match (item, rest) {
            // A shelf's middle takes the card in; its edges move it.
            (Some(Item::Shelf(shelf)), _) if on_card && (0.25..=0.75).contains(&across) => {
                cx.filing = Some(shelf.id);
            }
            (Some(_), Some(r)) if on_card => want = Some(r + usize::from(across > 0.5)),
            // Empty space, the plus card included, leaves the hole put.
            _ => {}
        }
    }
    let now = ui.input(|i| i.time);
    if let Some(lift) = &mut cx.motion.lift
        && let Some(left) = lift.hole.aim(want, now)
    {
        ui.ctx().request_repaint_after_secs(left as f32);
    }
    // Letting go lands where the pointer is, without waiting.
    if cx.filing.is_none() && ui.input(|i| i.pointer.any_released()) {
        let at = want.or(cx.motion.lift.as_ref().map(|l| l.hole.at));
        arrange_at_hole(fill, at, cx);
    }
}

/// Lets go over the grid: the lifted cards land at rest index `at`.
fn arrange_at_hole(fill: &Fill<'_, '_>, at: Option<usize>, cx: &mut Cx<'_>) {
    let (Some(lift), Some(arranging), Some(at)) = (&cx.motion.lift, cx.arranging, at) else {
        return;
    };
    if at == lift.hole.start {
        return;
    }
    let next = fill.rest.get(at).and_then(|&n| fill.items.get(n));
    let action = ShelfAction::Arrange {
        items: lift.moving.clone(),
        level: arranging.level,
        before: next.map(|i| i.pick()),
    };
    cx.out.push(Command::Library(Action::Shelf(action)));
    // Dropped while sorted, the page shows the order just made.
    cx.out.extend(arranging.switch.map(Command::SetLibraryView));
}

/// Held near the top or bottom, the grid scrolls toward that edge.
fn scroll_near_edges(ui: &Ui, view: Rect, pointer: Pos2) {
    const BAND: f32 = 32.0;
    let x_inside = view.x_range().contains(pointer.x);
    let up = (view.top() + BAND - pointer.y).clamp(0.0, BAND);
    let down = (pointer.y - (view.bottom() - BAND)).clamp(0.0, BAND);
    if !x_inside || (up == 0.0 && down == 0.0) {
        return;
    }
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    // Up to 900 points a second, faster the closer to the edge.
    let speed = (up - down) / BAND * 900.0;
    ui.scroll_with_delta(vec2(0.0, speed * dt));
    ui.ctx().request_repaint();
}

/// The lifted card under the pointer, raised; false if none is lifted.
pub(super) fn ghost(ui: &mut Ui, cx: &mut Cx<'_>) -> bool {
    let (Some(lift), Some(grab)) = (&cx.motion.lift, cx.motion.grab) else {
        return false;
    };
    let Some(pointer) = ui.ctx().pointer_latest_pos() else {
        return false;
    };
    let count = lift.moving.len();
    let Some(item) = Item::of(cx.model, grab.pick) else {
        return false;
    };
    let size = cx.motion.card;
    let rect = Rect::from_min_size(pointer - grab.offset, size);
    cx.motion.ghost = Some(rect.min - cx.motion.origin);
    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    let layer = LayerId::new(Order::Tooltip, Id::new("library-ghost"));
    let ui = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(rect));
    let t = cx.t;
    // Lifted: a touch larger, on a deeper shadow.
    let lifted = rect.expand(rect.width() * 0.03);
    let bounds = Rect::from_min_size(lifted.min, vec2(lifted.width(), lifted.width() * A4));
    let shadow = Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: t.shadow,
    };
    ui.painter()
        .add(shadow.as_shape(bounds, CornerRadius::from(t.radius.min(6.0))));
    let name = match item {
        Item::Book(book) => {
            cover(&ui, bounds, book, cx, 1.0, false);
            book.title.value.as_str()
        }
        Item::Shelf(shelf) => {
            plate(&ui, bounds, shelf.id, cx);
            shelf.name.value.as_str()
        }
    };
    let p = ui.painter();
    let font = theme::medium(13.0);
    line(
        p,
        name,
        font,
        t.ink,
        (bounds.left(), bounds.bottom() + 8.0),
        bounds.width(),
    );
    // Alt adds a copy instead of moving it off its shelf.
    let carry = DragAndDrop::payload::<Carry>(ui.ctx());
    let keep = ui.input(|i| i.modifiers.alt) && carry.is_some_and(|c| c.off_shelf());
    let badge = match (count, keep) {
        (1, false) => None,
        (1, true) => Some("+".to_owned()),
        (n, false) => Some(n.to_string()),
        (n, true) => Some(format!("+{n}")),
    };
    if let Some(text) = badge {
        let g = p.layout_no_wrap(text, theme::medium(12.0), Color32::WHITE);
        let d = g.size().x.max(g.size().y) + 10.0;
        let dot = Rect::from_center_size(bounds.right_top(), vec2(d, d));
        p.rect_filled(dot, d / 2.0, t.accent);
        p.galley(dot.center() - g.size() / 2.0, g, Color32::WHITE);
    }
    true
}
