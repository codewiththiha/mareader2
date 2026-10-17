//! The split tree. [`MAX_PANES`] bounds its depth, so recursion is safe.

use serde::{Deserialize, Serialize};

/// The most panes a workspace holds at once.
pub const MAX_PANES: usize = 4;
/// The smallest share either side of a split may have.
pub const MIN_RATIO: f32 = 0.15;
/// The narrowest a split may leave either side, in points.
pub const MIN_PANE_PX: f32 = 200.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PaneId(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitAxis {
    /// Side by side: a vertical seam.
    Horizontal,
    /// Stacked: a horizontal seam.
    Vertical,
}

/// Where a new pane goes relative to the one it splits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// Left of it, or above it.
    Before,
    /// Right of it, or below it.
    After,
}

/// One edge of a pane, as a drop target names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    /// Tie-break order when a point is equally near two edges.
    pub const PRIORITY: [Edge; 4] = [Edge::Right, Edge::Bottom, Edge::Left, Edge::Top];

    pub fn placement(self) -> (SplitAxis, Side) {
        match self {
            Edge::Left => (SplitAxis::Horizontal, Side::Before),
            Edge::Right => (SplitAxis::Horizontal, Side::After),
            Edge::Top => (SplitAxis::Vertical, Side::Before),
            Edge::Bottom => (SplitAxis::Vertical, Side::After),
        }
    }

    /// The edge of `rect` nearest `point`, relative to the rect's size.
    pub fn nearest(rect: Bounds, point: (f32, f32)) -> Edge {
        let w = rect.w.max(1.0);
        let h = rect.h.max(1.0);
        // A non-finite pointer counts as the centre, so ties pick Right.
        let frac = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.5
            }
        };
        let fx = frac((point.0 - rect.x) / w);
        let fy = frac((point.1 - rect.y) / h);
        let distance = |e: Edge| match e {
            Edge::Left => fx,
            Edge::Right => 1.0 - fx,
            Edge::Top => fy,
            Edge::Bottom => 1.0 - fy,
        };
        Edge::PRIORITY
            .into_iter()
            .fold((Edge::Right, f32::INFINITY), |best, e| {
                let d = distance(e);
                if d < best.1 { (e, d) } else { best }
            })
            .0
    }
}

/// An axis-aligned box in points.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Bounds {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            x,
            y,
            w: w.max(0.0),
            h: h.max(0.0),
        }
    }

    /// Splits in whole points so the two sides tile the box exactly.
    pub fn split(self, axis: SplitAxis, ratio: f32) -> (Bounds, Bounds) {
        let ratio = if ratio.is_finite() {
            ratio.clamp(0.0, 1.0)
        } else {
            0.5
        };
        match axis {
            SplitAxis::Horizontal => {
                let a = (self.w * ratio).round();
                (
                    Bounds::new(self.x, self.y, a, self.h),
                    Bounds::new(self.x + a, self.y, self.w - a, self.h),
                )
            }
            SplitAxis::Vertical => {
                let a = (self.h * ratio).round();
                (
                    Bounds::new(self.x, self.y, self.w, a),
                    Bounds::new(self.x, self.y + a, self.w, self.h - a),
                )
            }
        }
    }

    pub(crate) fn extent(self, axis: SplitAxis) -> f32 {
        match axis {
            SplitAxis::Horizontal => self.w,
            SplitAxis::Vertical => self.h,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeError {
    #[error("the workspace already holds {MAX_PANES} panes")]
    Full,
    #[error("no pane {0:?} in this workspace")]
    Missing(PaneId),
    #[error("pane {0:?} has no room for another split")]
    NoRoom(PaneId),
    #[error("the last pane cannot be closed")]
    LastPane,
    #[error("a pane cannot move beside itself")]
    Itself,
}

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Leaf(PaneId),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

/// Root-to-split turns: `false` enters `first`, `true` enters `second`.
pub type DividerPath = Vec<bool>;

/// A draggable seam between two sides of one split.
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    pub path: DividerPath,
    pub axis: SplitAxis,
    /// The box the split divides; a drag maps the pointer into it.
    pub parent: Bounds,
    /// The seam's position along the split axis, in points.
    pub at: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaneTree {
    root: Node,
    next: u64,
}

impl Default for PaneTree {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneTree {
    /// A workspace with one empty pane, `PaneId(1)`.
    pub fn new() -> Self {
        Self {
            root: Node::Leaf(PaneId(1)),
            next: 2,
        }
    }

    pub fn panes(&self) -> Vec<PaneId> {
        fn walk(n: &Node, out: &mut Vec<PaneId>) {
            match n {
                Node::Leaf(id) => out.push(*id),
                Node::Split { first, second, .. } => {
                    walk(first, out);
                    walk(second, out);
                }
            }
        }
        let mut out = Vec::with_capacity(MAX_PANES);
        walk(&self.root, &mut out);
        out
    }

    pub fn len(&self) -> usize {
        self.panes().len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// Splits `of`, adding a pane on `side`, if both halves stay usable.
    pub fn split(
        &mut self,
        of: PaneId,
        axis: SplitAxis,
        side: Side,
        area: Bounds,
    ) -> Result<PaneId, TreeError> {
        if self.len() >= MAX_PANES {
            return Err(TreeError::Full);
        }
        let new = PaneId(self.next);
        self.insert(of, (axis, side), area, new)?;
        self.next = self.next.saturating_add(1);
        Ok(new)
    }

    /// Trades the places of `a` and `b`, each keeping its own box size.
    pub fn swap(&mut self, a: PaneId, b: PaneId) -> Result<(), TreeError> {
        for id in [a, b] {
            if !self.panes().contains(&id) {
                return Err(TreeError::Missing(id));
            }
        }
        fn walk(n: &mut Node, a: PaneId, b: PaneId) {
            match n {
                Node::Leaf(id) if *id == a => *id = b,
                Node::Leaf(id) if *id == b => *id = a,
                Node::Leaf(_) => {}
                Node::Split { first, second, .. } => {
                    walk(first, a, b);
                    walk(second, a, b);
                }
            }
        }
        walk(&mut self.root, a, b);
        Ok(())
    }

    /// Moves `pane` to `edge` of `target`; unchanged if it won't fit.
    pub fn move_to(
        &mut self,
        pane: PaneId,
        target: PaneId,
        edge: Edge,
        area: Bounds,
    ) -> Result<(), TreeError> {
        if pane == target {
            return Err(TreeError::Itself);
        }
        let mut next = self.clone();
        next.close(pane)?;
        next.insert(target, edge.placement(), area, pane)?;
        *self = next;
        Ok(())
    }

    /// Puts `new` on `side` of `of`, if both halves stay usable.
    fn insert(
        &mut self,
        of: PaneId,
        (axis, side): (SplitAxis, Side),
        area: Bounds,
        new: PaneId,
    ) -> Result<(), TreeError> {
        let bounds = self
            .layout(area)
            .into_iter()
            .find(|(id, _)| *id == of)
            .map(|(_, b)| b);
        let bounds = bounds.ok_or(TreeError::Missing(of))?;
        if bounds.extent(axis) > 0.0 && bounds.extent(axis) / 2.0 < MIN_PANE_PX {
            return Err(TreeError::NoRoom(of));
        }
        fn replace(n: &mut Node, of: PaneId, new: PaneId, axis: SplitAxis, side: Side) -> bool {
            match n {
                Node::Leaf(id) if *id == of => {
                    let (first, second) = match side {
                        Side::Before => (new, of),
                        Side::After => (of, new),
                    };
                    *n = Node::Split {
                        axis,
                        ratio: 0.5,
                        first: Box::new(Node::Leaf(first)),
                        second: Box::new(Node::Leaf(second)),
                    };
                    true
                }
                Node::Leaf(_) => false,
                Node::Split { first, second, .. } => {
                    replace(first, of, new, axis, side) || replace(second, of, new, axis, side)
                }
            }
        }
        if !replace(&mut self.root, of, new, axis, side) {
            return Err(TreeError::Missing(of));
        }
        Ok(())
    }

    /// Closes `pane`; its sibling takes the whole of their split.
    pub fn close(&mut self, pane: PaneId) -> Result<(), TreeError> {
        if !self.panes().contains(&pane) {
            return Err(TreeError::Missing(pane));
        }
        if matches!(self.root, Node::Leaf(_)) {
            return Err(TreeError::LastPane);
        }
        fn remove(n: &mut Node, pane: PaneId) -> bool {
            let Node::Split { first, second, .. } = n else {
                return false;
            };
            let survivor = match (first.as_ref(), second.as_ref()) {
                (Node::Leaf(id), _) if *id == pane => Some(second.as_ref().clone()),
                (_, Node::Leaf(id)) if *id == pane => Some(first.as_ref().clone()),
                _ => None,
            };
            match survivor {
                Some(s) => {
                    *n = s;
                    true
                }
                None => remove(first, pane) || remove(second, pane),
            }
        }
        if remove(&mut self.root, pane) {
            Ok(())
        } else {
            Err(TreeError::Missing(pane))
        }
    }

    /// Each pane's box inside `area`, in tree order.
    pub fn layout(&self, area: Bounds) -> Vec<(PaneId, Bounds)> {
        fn walk(n: &Node, b: Bounds, out: &mut Vec<(PaneId, Bounds)>) {
            match n {
                Node::Leaf(id) => out.push((*id, b)),
                Node::Split {
                    axis,
                    ratio,
                    first,
                    second,
                } => {
                    let (a, c) = b.split(*axis, *ratio);
                    walk(first, a, out);
                    walk(second, c, out);
                }
            }
        }
        let mut out = Vec::with_capacity(MAX_PANES);
        walk(&self.root, area, &mut out);
        out
    }

    pub fn dividers(&self, area: Bounds) -> Vec<Divider> {
        fn walk(n: &Node, b: Bounds, path: &mut DividerPath, out: &mut Vec<Divider>) {
            let Node::Split {
                axis,
                ratio,
                first,
                second,
            } = n
            else {
                return;
            };
            let (a, c) = b.split(*axis, *ratio);
            let at = match axis {
                SplitAxis::Horizontal => c.x,
                SplitAxis::Vertical => c.y,
            };
            out.push(Divider {
                path: path.clone(),
                axis: *axis,
                parent: b,
                at,
            });
            path.push(false);
            walk(first, a, path, out);
            path.pop();
            path.push(true);
            walk(second, c, path, out);
            path.pop();
        }
        let mut out = Vec::new();
        walk(&self.root, area, &mut Vec::new(), &mut out);
        out
    }

    /// Moves a divider; the ratio stays within [`MIN_RATIO`] of each end.
    pub fn set_ratio(&mut self, path: &[bool], ratio: f32) -> bool {
        let mut n = &mut self.root;
        for step in path {
            match n {
                Node::Split { first, second, .. } => n = if *step { second } else { first },
                Node::Leaf(_) => return false,
            }
        }
        match n {
            Node::Split { ratio: r, .. } if ratio.is_finite() => {
                *r = ratio.clamp(MIN_RATIO, 1.0 - MIN_RATIO);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Bounds = Bounds {
        x: 0.0,
        y: 0.0,
        w: 1600.0,
        h: 1000.0,
    };

    #[test]
    fn splits_stop_at_four() {
        let mut t = PaneTree::new();
        let a = t
            .split(PaneId(1), SplitAxis::Horizontal, Side::After, AREA)
            .unwrap();
        t.split(PaneId(1), SplitAxis::Vertical, Side::After, AREA)
            .unwrap();
        t.split(a, SplitAxis::Vertical, Side::After, AREA).unwrap();
        assert_eq!(t.len(), 4);
        assert_eq!(
            t.split(a, SplitAxis::Horizontal, Side::After, AREA),
            Err(TreeError::Full)
        );
    }

    #[test]
    fn layout_tiles_the_area_exactly() {
        let mut t = PaneTree::new();
        t.split(PaneId(1), SplitAxis::Horizontal, Side::After, AREA)
            .unwrap();
        t.split(PaneId(2), SplitAxis::Vertical, Side::Before, AREA)
            .unwrap();
        let total: f32 = t.layout(AREA).iter().map(|(_, b)| b.w * b.h).sum();
        assert_eq!(total, AREA.w * AREA.h);
        assert_eq!(t.panes(), [PaneId(1), PaneId(3), PaneId(2)]);
    }

    #[test]
    fn a_narrow_pane_refuses_to_split() {
        let mut t = PaneTree::new();
        let narrow = Bounds::new(0.0, 0.0, 300.0, 1000.0);
        assert_eq!(
            t.split(PaneId(1), SplitAxis::Horizontal, Side::After, narrow),
            Err(TreeError::NoRoom(PaneId(1)))
        );
        assert!(
            t.split(PaneId(1), SplitAxis::Vertical, Side::After, narrow)
                .is_ok()
        );
    }

    #[test]
    fn closing_hands_the_space_to_the_sibling() {
        let mut t = PaneTree::new();
        let b = t
            .split(PaneId(1), SplitAxis::Horizontal, Side::After, AREA)
            .unwrap();
        t.close(PaneId(1)).unwrap();
        assert_eq!(t.layout(AREA), [(b, AREA)]);
        assert_eq!(t.close(b), Err(TreeError::LastPane));
        assert_eq!(t.close(PaneId(99)), Err(TreeError::Missing(PaneId(99))));
    }

    #[test]
    fn dividers_resize_within_limits() {
        let mut t = PaneTree::new();
        t.split(PaneId(1), SplitAxis::Horizontal, Side::After, AREA)
            .unwrap();
        let d = t.dividers(AREA);
        assert_eq!(d.len(), 1);
        assert_eq!(d.first().map(|d| d.at), Some(800.0));
        assert!(t.set_ratio(&[], 0.01));
        assert_eq!(t.dividers(AREA).first().map(|d| d.at), Some(240.0));
        assert!(!t.set_ratio(&[true], 0.5), "a leaf has no ratio");
        assert!(!t.set_ratio(&[], f32::NAN));
    }

    #[test]
    fn nearest_edge_prefers_right_then_bottom_on_ties() {
        let r = Bounds::new(0.0, 0.0, 100.0, 100.0);
        assert_eq!(Edge::nearest(r, (5.0, 50.0)), Edge::Left);
        assert_eq!(Edge::nearest(r, (50.0, 97.0)), Edge::Bottom);
        assert_eq!(Edge::nearest(r, (50.0, 50.0)), Edge::Right);
        assert_eq!(Edge::nearest(r, (f32::NAN, 50.0)), Edge::Right);
    }

    #[test]
    fn a_swap_trades_places_and_keeps_the_boxes() {
        let mut t = PaneTree::new();
        let b = t
            .split(PaneId(1), SplitAxis::Horizontal, Side::After, AREA)
            .unwrap();
        let before = t.layout(AREA);
        t.swap(PaneId(1), b).unwrap();
        let after = t.layout(AREA);
        assert_eq!(after[0], (b, before[0].1));
        assert_eq!(after[1], (PaneId(1), before[1].1));
        assert_eq!(
            t.swap(PaneId(1), PaneId(9)),
            Err(TreeError::Missing(PaneId(9)))
        );
    }

    #[test]
    fn a_moved_pane_halves_its_target_and_keeps_its_id() {
        // 1 | 2, then 1 moves below 2: one column, 2 over 1.
        let mut t = PaneTree::new();
        let b = t
            .split(PaneId(1), SplitAxis::Horizontal, Side::After, AREA)
            .unwrap();
        t.move_to(PaneId(1), b, Edge::Bottom, AREA).unwrap();
        let boxes = t.layout(AREA);
        assert_eq!(boxes[0], (b, Bounds::new(0.0, 0.0, 1600.0, 500.0)));
        assert_eq!(
            boxes[1],
            (PaneId(1), Bounds::new(0.0, 500.0, 1600.0, 500.0))
        );
        // New panes still get fresh ids.
        let c = t
            .split(b, SplitAxis::Horizontal, Side::After, AREA)
            .unwrap();
        assert!(c != b && c != PaneId(1));
    }

    #[test]
    fn a_move_that_will_not_fit_changes_nothing() {
        let mut t = PaneTree::new();
        let b = t
            .split(PaneId(1), SplitAxis::Horizontal, Side::After, AREA)
            .unwrap();
        let small = Bounds::new(0.0, 0.0, 1600.0, 300.0);
        let before = t.clone();
        let refused = t.move_to(PaneId(1), b, Edge::Bottom, small);
        assert_eq!(refused, Err(TreeError::NoRoom(b)));
        assert_eq!(t, before);
        assert_eq!(t.move_to(b, b, Edge::Left, AREA), Err(TreeError::Itself));
    }
}
