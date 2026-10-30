//! Whole floating panels resting partly beyond a stage edge.

use egui::{Context, CursorIcon, Id, Order, Rect, Sense, pos2};

/// The visible part of a panel left at rest, in points.
pub const PEEK: f32 = 20.0;
const HIDE: f32 = 0.26;
const REVEAL: f32 = 0.34;

/// The edge a whole panel slides through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Bottom,
}

impl Edge {
    /// How far `rect` extends beyond this edge.
    pub fn over(self, rect: Rect, stage: Rect) -> f32 {
        let over = match self {
            Self::Left => stage.left() - rect.left(),
            Self::Right => rect.right() - stage.right(),
            Self::Bottom => rect.bottom() - stage.bottom(),
        };
        over.max(0.0)
    }

    /// Keep the panel's size, leaving only its edge peek on the stage.
    pub fn dock(self, rect: Rect, stage: Rect) -> Rect {
        let peek = PEEK.min(match self {
            Self::Left | Self::Right => rect.width(),
            Self::Bottom => rect.height(),
        });
        let min = match self {
            Self::Left => pos2(
                stage.left() + peek - rect.width(),
                rect.top().clamp(
                    stage.top(),
                    (stage.bottom() - rect.height()).max(stage.top()),
                ),
            ),
            Self::Right => pos2(
                stage.right() - peek,
                rect.top().clamp(
                    stage.top(),
                    (stage.bottom() - rect.height()).max(stage.top()),
                ),
            ),
            Self::Bottom => pos2(
                rect.left().clamp(
                    stage.left(),
                    (stage.right() - rect.width()).max(stage.left()),
                ),
                stage.bottom() - peek,
            ),
        };
        Rect::from_min_size(min, rect.size())
    }
}

/// The source, home, and resting positions for one parked panel.
#[derive(Clone, Copy, Debug)]
pub struct Tuck {
    pub edge: Edge,
    pub from: Rect,
    pub home: Rect,
    pub dock: Rect,
    /// The stored float rect after the visible shape finishes revealing.
    pub resume: Rect,
    pub revealing: bool,
}

impl Tuck {
    /// Start moving `from` toward the same-size peek at `edge`.
    pub fn new(edge: Edge, from: Rect, home: Rect, stage: Rect) -> Self {
        Self {
            edge,
            from,
            home,
            dock: edge.dock(from, stage),
            resume: home,
            revealing: false,
        }
    }

    /// Restore an underlying float rect different from its visible home shape.
    pub fn with_resume(mut self, resume: Rect) -> Self {
        self.resume = resume;
        self
    }

    /// Where the panel is now, with 0 shown and 1 tucked.
    pub fn rect(self, tucked: f32) -> Rect {
        let (from, to, k) = if self.revealing {
            (self.dock, self.home, 1.0 - tucked)
        } else {
            (self.from, self.dock, tucked)
        };
        Rect::from_min_max(from.min.lerp(to.min, k), from.max.lerp(to.max, k))
    }
}

/// Animate a whole panel toward or away from its edge peek.
pub fn factor(ctx: &Context, id: Id, tucked: bool) -> f32 {
    let time = crate::motion::secs(ctx, if tucked { HIDE } else { REVEAL });
    let k = ctx.animate_bool_with_time(id, tucked, time).clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// A click-only hit area over the panel's exposed, original surface.
pub fn reveal(ctx: &Context, id: Id, rect: Rect, stage: Rect, label: &str) -> bool {
    let peek = rect.intersect(stage);
    if peek.width() <= 0.0 || peek.height() <= 0.0 {
        return false;
    }
    egui::Area::new(id)
        .order(Order::Tooltip)
        .fixed_pos(peek.min)
        .constrain(false)
        .interactable(true)
        .movable(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(peek);
            ui.set_min_size(peek.size());
            ui.set_max_size(peek.size());
            let hit = ui.interact(peek, id.with("hit"), Sense::click());
            hit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
            if hit.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            hit.clicked()
        })
        .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docking_keeps_the_original_size_and_leaves_a_peek() {
        let stage = Rect::from_min_size(pos2(0.0, 20.0), egui::vec2(900.0, 700.0));
        let panel = Rect::from_min_size(pos2(250.0, 180.0), egui::vec2(380.0, 420.0));
        let left = Edge::Left.dock(panel, stage);
        let right = Edge::Right.dock(panel, stage);
        let bottom = Edge::Bottom.dock(panel, stage);
        assert_eq!(left.size(), panel.size());
        assert_eq!(right.size(), panel.size());
        assert_eq!(bottom.size(), panel.size());
        assert_eq!(left.intersect(stage).width(), PEEK);
        assert_eq!(right.intersect(stage).width(), PEEK);
        assert_eq!(bottom.intersect(stage).height(), PEEK);
    }

    #[test]
    fn revealing_slides_from_the_same_dock_back_home() {
        let stage = Rect::from_min_size(pos2(0.0, 20.0), egui::vec2(900.0, 700.0));
        let home = Rect::from_min_size(pos2(250.0, 180.0), egui::vec2(380.0, 420.0));
        let mut tuck = Tuck::new(Edge::Right, home, home, stage);
        assert_eq!(tuck.rect(0.0), home);
        assert_eq!(tuck.rect(1.0), tuck.dock);
        tuck.revealing = true;
        assert_eq!(tuck.rect(1.0), tuck.dock);
        assert_eq!(tuck.rect(0.0), home);
    }
}
