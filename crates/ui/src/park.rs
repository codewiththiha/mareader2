//! Whole floating panels resting partly beyond a stage edge.

use egui::{Context, CornerRadius, CursorIcon, Id, Order, Rect, Sense, Vec2, pos2};

/// The visible part of a panel left at rest, in points.
pub const PEEK: f32 = 20.0;
const HIDE: f32 = 0.26;
const REVEAL: f32 = 0.34;
const MAGNET: f32 = 8.0;
const MAGNET_TIME: f32 = 0.18;

#[derive(Clone, Copy)]
struct Motion {
    from: f32,
    to: f32,
    started: f64,
    duration: f64,
}

impl Motion {
    fn value(self, now: f64) -> f32 {
        if self.duration <= 0.0 {
            return self.to;
        }
        let t = ((now - self.started) / self.duration).clamp(0.0, 1.0) as f32;
        self.from + (self.to - self.from) * ease_out(t)
    }

    fn moving(self, now: f64) -> bool {
        self.duration > 0.0 && self.from != self.to && now < self.started + self.duration
    }
}

fn ease_out(k: f32) -> f32 {
    1.0 - (1.0 - k.clamp(0.0, 1.0)).powi(3)
}

fn animate(ctx: &Context, id: Id, to: f32, seconds: f32) -> f32 {
    let now = ctx.input(|i| i.time);
    let duration = f64::from(crate::motion::secs(ctx, seconds));
    let motion = ctx.data_mut(|data| {
        let previous = data.get_temp::<Motion>(id);
        let next = if duration <= 0.0 {
            Motion {
                from: to,
                to,
                started: now,
                duration: 0.0,
            }
        } else if let Some(previous) = previous {
            if previous.to == to {
                previous
            } else {
                Motion {
                    from: previous.value(now),
                    to,
                    started: now,
                    duration,
                }
            }
        } else {
            Motion {
                from: 0.0,
                to,
                started: now,
                duration,
            }
        };
        data.insert_temp(id, next);
        next
    });
    if motion.moving(now) {
        ctx.request_repaint();
    }
    motion.value(now)
}

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
        self.docked(rect, stage, PEEK)
    }

    /// As `dock`, with `peek` points of `rect` left on the stage.
    pub fn docked(self, rect: Rect, stage: Rect, peek: f32) -> Rect {
        let peek = peek.min(match self {
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
        Self::docked(edge, from, home, stage, PEEK)
    }

    /// Start moving `from` fully beyond `edge`, its tab standing in for it.
    pub fn beyond(edge: Edge, from: Rect, home: Rect, stage: Rect) -> Self {
        Self::docked(edge, from, home, stage, 0.0)
    }

    fn docked(edge: Edge, from: Rect, home: Rect, stage: Rect, peek: f32) -> Self {
        Self {
            edge,
            from,
            home,
            dock: edge.docked(from, stage, peek),
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

    fn inward(self, amount: f32) -> Vec2 {
        inward(self.edge, amount)
    }
}

/// Animate a whole panel toward or away from its edge peek.
pub fn factor(ctx: &Context, id: Id, tucked: bool, active: bool) -> f32 {
    if !active {
        return animate(ctx, id, 0.0, 0.0);
    }
    let (to, seconds) = if tucked { (1.0, HIDE) } else { (0.0, REVEAL) };
    animate(ctx, id, to, seconds)
}

pub(crate) fn magnetic_offset(
    ctx: &Context,
    id: Id,
    tuck: Option<Tuck>,
    rect: Rect,
    stage: Rect,
    enabled: bool,
) -> Vec2 {
    let Some(tuck) = tuck else {
        animate(ctx, id, 0.0, 0.0);
        return Vec2::ZERO;
    };
    let visible = rect.intersect(stage);
    let hovered = enabled
        && ctx
            .pointer_latest_pos()
            .is_some_and(|point| visible.contains(point));
    let lift = animate(ctx, id, if hovered { 1.0 } else { 0.0 }, MAGNET_TIME);
    tuck.inward(MAGNET * lift)
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

/// How far `amount` points reach from `edge` into the stage.
fn inward(edge: Edge, amount: f32) -> Vec2 {
    match edge {
        Edge::Left => Vec2::new(amount, 0.0),
        Edge::Right => Vec2::new(-amount, 0.0),
        Edge::Bottom => Vec2::new(0.0, -amount),
    }
}

/// A tab of `width` by `height` flush on `edge`, centred `along` it.
fn spot(edge: Edge, along: f32, stage: Rect, width: f32, height: f32) -> Rect {
    match edge {
        Edge::Left | Edge::Right => {
            let y = along.clamp(
                stage.top() + TAB_MARGIN + height / 2.0,
                stage.bottom() - TAB_MARGIN - height / 2.0,
            );
            let x = if edge == Edge::Left {
                stage.left()
            } else {
                stage.right() - width
            };
            Rect::from_min_max(pos2(x, y - height / 2.0), pos2(x + width, y + height / 2.0))
        }
        Edge::Bottom => {
            let x = along.clamp(
                stage.left() + TAB_MARGIN + width / 2.0,
                stage.right() - TAB_MARGIN - width / 2.0,
            );
            Rect::from_min_max(
                pos2(x - width / 2.0, stage.bottom() - height),
                pos2(x + width / 2.0, stage.bottom()),
            )
        }
    }
}

/// What a resting panel's tab says about it.
pub struct Tab {
    /// Telling one parked panel from another at a glance.
    pub icon: crate::icons::Icon,
    /// Its name, or the word it holds.
    pub label: String,
    /// A live line under the name, as "playing · 34%".
    pub state: Option<String>,
    /// What it is, for the tab's accessible name: "the read-aloud player".
    pub name: &'static str,
}

/// The tab's heights on the side and bottom edges.
const TAB_SIDE: f32 = 52.0;
const TAB_FOOT: f32 = 42.0;
/// Room the tab keeps from the stage's ends along its edge.
const TAB_MARGIN: f32 = 8.0;
const CHIP: f32 = 26.0;
const PAD: f32 = 10.0;
const GAP: f32 = 8.0;

/// The resting panel's tab, `tucked` of the way in; true when clicked.
pub fn tab(
    ctx: &Context,
    id: Id,
    edge: Edge,
    along: f32,
    stage: Rect,
    tucked: f32,
    shown: Tab,
) -> bool {
    if tucked <= 0.0 {
        return false;
    }
    let t = crate::theme::Tokens::get(ctx);
    let wide = match edge {
        Edge::Left | Edge::Right => 168.0,
        Edge::Bottom => 224.0,
    };
    let budget = wide - 2.0 * PAD - CHIP - GAP;
    let elided = |text: String, size: f32, colour: egui::Color32| {
        let mut job =
            egui::text::LayoutJob::simple_singleline(text, crate::theme::medium(size), colour);
        job.wrap = egui::text::TextWrapping {
            max_width: budget,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        ctx.fonts_mut(|f| f.layout_job(job))
    };
    let label = elided(shown.label, t.text, t.ink);
    let state = shown.state.map(|s| elided(s, t.text_small, t.muted));
    let text_w = label
        .size()
        .x
        .max(state.as_ref().map_or(0.0, |g| g.size().x));
    let own = (2.0 * PAD + CHIP + GAP + text_w).max(112.0);
    let (height, width) = match edge {
        Edge::Left | Edge::Right => (TAB_SIDE, own.min(wide)),
        Edge::Bottom => (TAB_FOOT, own.max(128.0).min(wide)),
    };
    let rect = spot(edge, along, stage, width, height);
    let hovered_now = ctx.pointer_latest_pos().is_some_and(|p| rect.contains(p));
    let lift = animate(ctx, id.with("magnet"), f32::from(hovered_now), MAGNET_TIME);
    let slide = inward(edge, (1.0 - tucked) * 24.0);
    let at = rect.translate(inward(edge, MAGNET * lift) + slide);
    let mut clicked = false;
    egui::Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(at.min)
        .constrain(false)
        .interactable(tucked >= 1.0)
        .movable(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(stage.expand(32.0));
            ui.set_min_size(at.size());
            ui.set_max_size(at.size());
            ui.set_opacity(tucked);
            let name = format!("Show {}", shown.name);
            let hit = ui.interact(at, id.with("hit"), Sense::click());
            hit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
            if hit.hovered() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            clicked = hit.clicked();
            let painter = ui.painter();
            let round = match edge {
                Edge::Left => CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: 10,
                    se: 10,
                },
                Edge::Right => CornerRadius {
                    nw: 10,
                    sw: 10,
                    ne: 0,
                    se: 0,
                },
                Edge::Bottom => CornerRadius {
                    nw: 10,
                    sw: 0,
                    ne: 10,
                    se: 0,
                },
            };
            let shadow = ui.ctx().global_style().visuals.window_shadow;
            painter.add(shadow.as_shape(at, round));
            painter.rect_filled(at, round, t.surface);
            let edge_stroke = if hit.hovered() {
                egui::Stroke::new(1.0, t.accent)
            } else {
                egui::Stroke::new(1.0, t.line)
            };
            painter.rect_stroke(at, round, edge_stroke, egui::StrokeKind::Inside);
            let chip_center = pos2(at.left() + PAD + CHIP / 2.0, at.center().y);
            let disc = Rect::from_center_size(chip_center, Vec2::splat(CHIP));
            painter.rect_filled(disc, CornerRadius::same(7), t.accent_soft);
            let glyph = Rect::from_center_size(chip_center, Vec2::splat(CHIP - 10.0));
            shown.icon.paint(ui, glyph, t.accent);
            let text_top = match &state {
                Some(g) => at.center().y - (label.size().y + g.size().y) / 2.0,
                None => at.center().y - label.size().y / 2.0,
            };
            let text_left = disc.right() + GAP;
            let line = label.size().y;
            painter.galley(pos2(text_left, text_top), label, t.ink);
            if let Some(g) = state {
                painter.galley(pos2(text_left, text_top + line), g, t.muted);
            }
        });
    clicked
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

    #[test]
    fn motion_uses_cubic_ease_out() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.5) > 0.5);
    }

    #[test]
    fn reversing_motion_keeps_its_current_position() {
        let motion = Motion {
            from: 0.0,
            to: 1.0,
            started: 0.0,
            duration: 1.0,
        };
        let now = 0.5;
        let current = motion.value(now);
        let reversed = Motion {
            from: current,
            to: 0.0,
            started: now,
            duration: 1.0,
        };
        assert_eq!(reversed.value(now), current);
    }

    #[test]
    fn magnetic_pull_exposes_more_of_every_edge() {
        let stage = Rect::from_min_size(pos2(0.0, 20.0), egui::vec2(900.0, 700.0));
        let home = Rect::from_min_size(pos2(250.0, 180.0), egui::vec2(380.0, 420.0));
        for edge in [Edge::Left, Edge::Right, Edge::Bottom] {
            let tuck = Tuck::new(edge, home, home, stage);
            let pulled = tuck.rect(1.0).translate(tuck.inward(MAGNET));
            let visible = pulled.intersect(stage);
            let exposed = match edge {
                Edge::Left | Edge::Right => visible.width(),
                Edge::Bottom => visible.height(),
            };
            assert!(
                (exposed - (PEEK + MAGNET)).abs() < 0.01,
                "{edge:?}: {visible:?}"
            );
        }
    }

    #[test]
    fn resting_beyond_an_edge_leaves_the_panel_whole_offstage() {
        let stage = Rect::from_min_size(pos2(0.0, 20.0), egui::vec2(900.0, 700.0));
        let home = Rect::from_min_size(pos2(250.0, 180.0), egui::vec2(380.0, 420.0));
        for edge in [Edge::Left, Edge::Right, Edge::Bottom] {
            let tuck = Tuck::beyond(edge, home, home, stage);
            assert_eq!(tuck.dock.size(), home.size(), "{edge:?} keeps the size");
            assert!(
                tuck.dock.intersect(stage).area() <= f32::EPSILON,
                "{edge:?}: {:?}",
                tuck.dock.intersect(stage)
            );
        }
    }

    #[test]
    fn a_tab_sits_flush_on_its_edge_and_never_leaves_the_stage() {
        let stage = Rect::from_min_size(pos2(0.0, 20.0), egui::vec2(900.0, 700.0));
        let (width, height) = (140.0, TAB_SIDE);
        let left = spot(Edge::Left, 900.0, stage, width, height);
        let right = spot(Edge::Right, -50.0, stage, width, height);
        assert_eq!(left.left(), stage.left());
        assert_eq!(left.bottom(), stage.bottom() - TAB_MARGIN);
        assert_eq!(right.right(), stage.right());
        assert_eq!(right.top(), stage.top() + TAB_MARGIN);
        let bottom = spot(Edge::Bottom, 400.0, stage, 200.0, TAB_FOOT);
        assert_eq!(bottom.bottom(), stage.bottom());
        assert!((bottom.center().x - 400.0).abs() < 0.01);
        let clamped = spot(Edge::Bottom, -400.0, stage, 200.0, TAB_FOOT);
        assert_eq!(clamped.left(), stage.left() + TAB_MARGIN);
    }
}
