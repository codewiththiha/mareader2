//! A card anchored to words on the page, its caret pointing at them.

use egui::{Align2, Context, Frame, Id, Margin, Order, Rect, Shape, Stroke, Ui, pos2};

use crate::motion;
use crate::theme::Tokens;

/// From the words to the card's edge; the caret spans most of it.
pub const GAP: f32 = 11.0;
/// How far the caret reaches out of the card.
const CARET: f32 = 8.0;
/// The caret keeps this far from the card's rounded corners.
const CORNER: f32 = 16.0;
/// The card keeps this far inside its room.
const MARGIN: f32 = 8.0;
/// Unmeasured, below wins while this much is left there.
const ROOM_BELOW: f32 = 240.0;
/// How far the card rises into place as it opens.
const LIFT: f32 = 4.0;
/// Seconds the opening takes.
const ENTER: f32 = 0.16;

/// Which side of its words the card sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Below,
    Above,
}

/// Where a card points and how wide it is.
pub struct Bubble {
    /// One per opened card: a new id measures before it shows.
    pub id: Id,
    /// The words pointed at.
    pub at: Rect,
    /// Where the card must stay, as its pane.
    pub room: Rect,
    /// The content's width inside the margins.
    pub width: f32,
    pub margin: Margin,
}

/// Last frame's height and side, and when the card opened.
#[derive(Clone, Copy, Debug, Default)]
struct Memo {
    height: f32,
    side: Option<Side>,
    opened: f64,
    /// The frame it was last drawn in; a gap means it reopened.
    pass: u64,
    /// The words and room it was placed for.
    placed: Option<(Rect, Rect)>,
}

impl Bubble {
    /// Draws the card; `add` is told the tallest its content may grow.
    pub fn show<R>(
        self,
        ctx: &Context,
        t: &Tokens,
        add: impl FnOnce(&mut Ui, f32) -> R,
    ) -> (Rect, R) {
        let memo_id = self.id.with("memo");
        let now = ctx.input(|i| i.time);
        let pass = ctx.cumulative_pass_nr();
        let fresh = Memo {
            opened: now,
            ..Memo::default()
        };
        let kept: Option<Memo> = ctx.data(|d| d.get_temp(memo_id));
        let mut memo = kept.filter(|m| m.pass + 1 >= pass).unwrap_or(fresh);
        let (at, room) = (self.at, self.room);
        let rooms = (
            room.bottom() - at.bottom() - GAP - MARGIN,
            at.top() - room.top() - GAP - MARGIN,
        );
        let need = (memo.height > 0.0).then_some(memo.height);
        // A card keeps its side only while its words and room stay put.
        let still = |a: Rect, b: Rect| (a.min - b.min).length() + (a.max - b.max).length() < 1.0;
        let unmoved = |(a, r): (Rect, Rect)| still(a, at) && still(r, room);
        let was = memo.side.filter(|_| memo.placed.is_some_and(unmoved));
        let side = side(rooms, need, was);
        let space = match side {
            Side::Below => rooms.0,
            Side::Above => rooms.1,
        };
        // Unmeasured, the content may grow as tall as the roomier side.
        let space = if need.is_some() {
            space
        } else {
            rooms.0.max(rooms.1)
        };
        let sides = self.margin.sum() + egui::vec2(2.0, 2.0);
        let cap = (space - sides.y).max(0.0);
        let width = self
            .width
            .min(room.width() - 2.0 * MARGIN - sides.x)
            .max(0.0);
        let half = (width + sides.x) / 2.0;
        let lo = room.left() + MARGIN + half;
        let x = at
            .center()
            .x
            .clamp(lo, (room.right() - MARGIN - half).max(lo));
        let lift = (1.0 - entrance(ctx, memo.opened, now)) * LIFT;
        let (pos, pivot) = match side {
            Side::Below => (pos2(x, at.bottom() + GAP + lift), Align2::CENTER_TOP),
            Side::Above => (pos2(x, at.top() - GAP - lift), Align2::CENTER_BOTTOM),
        };
        let shown = egui::Area::new(self.id)
            .order(Order::Foreground)
            .pivot(pivot)
            .fixed_pos(pos)
            .constrain(false)
            .sizing_pass(need.is_none())
            .show(ctx, |ui| {
                let frame = Frame::popup(ui.style())
                    .fill(t.surface)
                    .stroke(Stroke::new(1.0, t.line))
                    .corner_radius(t.radius_lg)
                    .inner_margin(self.margin)
                    .show(ui, |ui| {
                        ui.set_width(width);
                        add(ui, cap)
                    });
                caret(ui, frame.response.rect, at.center().x, side, t);
                frame.inner
            });
        memo.height = shown.response.rect.height();
        memo.side = Some(side);
        memo.pass = pass;
        memo.placed = Some((at, room));
        ctx.data_mut(|d| d.insert_temp(memo_id, memo));
        (shown.response.rect, shown.inner)
    }
}

/// Where the card fits, below first; a placed card stays while it fits.
pub fn side((below, above): (f32, f32), need: Option<f32>, was: Option<Side>) -> Side {
    let Some(need) = need else {
        let below_wins = below >= ROOM_BELOW || below >= above;
        return if below_wins { Side::Below } else { Side::Above };
    };
    let fits = |s: Side| match s {
        Side::Below => below >= need,
        Side::Above => above >= need,
    };
    match was {
        Some(s) if fits(s) => s,
        _ if fits(Side::Below) => Side::Below,
        _ if fits(Side::Above) => Side::Above,
        _ if below >= above => Side::Below,
        _ => Side::Above,
    }
}

/// How far open the card is, eased; 1 once settled.
fn entrance(ctx: &Context, opened: f64, now: f64) -> f32 {
    let span = motion::secs(ctx, ENTER);
    if span <= 0.0 {
        return 1.0;
    }
    let x = ((now - opened) as f32 / span).clamp(0.0, 1.0);
    if x < 1.0 {
        ctx.request_repaint();
    }
    1.0 - (1.0 - x).powi(3)
}

/// The caret on the card's near edge, under the words' middle.
fn caret(ui: &Ui, card: Rect, x: f32, side: Side, t: &Tokens) {
    let lo = card.left() + CORNER;
    let x = x.clamp(lo, (card.right() - CORNER).max(lo));
    // Outward from the card, and the edge the caret grows from.
    let (edge, out) = match side {
        Side::Below => (card.top(), -1.0),
        Side::Above => (card.bottom(), 1.0),
    };
    let tip = pos2(x, edge + out * CARET);
    let left = pos2(x - CARET, edge);
    let right = pos2(x + CARET, edge);
    // The fill dips into the card to hide its border under the caret.
    let mut fill = vec![
        pos2(x - CARET - 1.0, edge - out),
        left,
        tip,
        right,
        pos2(x + CARET + 1.0, edge - out),
    ];
    if side == Side::Above {
        fill.reverse();
    }
    let painter = ui.painter().with_clip_rect(ui.ctx().content_rect());
    painter.add(Shape::convex_polygon(fill, t.surface, Stroke::NONE));
    painter.add(Shape::line(
        vec![left, tip, right],
        Stroke::new(1.0, t.line),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_takes_the_side_it_fits() {
        // Burmese lines run tall: 400 points fit above, not below.
        assert_eq!(side((300.0, 500.0), Some(400.0), None), Side::Above);
        assert_eq!(side((500.0, 300.0), Some(400.0), None), Side::Below);
        // Neither fits: the roomier side, the content scrolling.
        assert_eq!(side((350.0, 300.0), Some(600.0), None), Side::Below);
        assert_eq!(side((300.0, 350.0), Some(600.0), None), Side::Above);
        // Unmeasured, below wins while it leaves room.
        assert_eq!(side((300.0, 500.0), None, None), Side::Below);
        assert_eq!(side((100.0, 500.0), None, None), Side::Above);
    }

    #[test]
    fn a_placed_card_keeps_its_side_while_it_fits() {
        let was = Some(Side::Above);
        assert_eq!(side((500.0, 300.0), Some(200.0), was), Side::Above);
        // Grown past its side, it moves where it fits.
        assert_eq!(side((500.0, 300.0), Some(400.0), was), Side::Below);
    }
}
