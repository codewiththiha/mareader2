//! Where a document dragged over the workspace would land.

use crate::tree::{Bounds, Edge, PaneId, Side};

/// What a drop does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropTarget {
    /// A new pane on `edge` of `pane`, taking half its box.
    Split { pane: PaneId, edge: Edge },
    /// Into `pane` itself, offered only while it holds no document.
    Here { pane: PaneId },
}

impl DropTarget {
    /// The pane under `point`: empty, it takes the document; else its
    /// nearest edge splits.
    pub fn at(
        panes: &[(PaneId, Bounds)],
        point: (f32, f32),
        empty: impl Fn(PaneId) -> bool,
    ) -> Option<Self> {
        let (pane, rect) = panes.iter().find(|(_, r)| r.contains(point))?;
        let pane = *pane;
        Some(match empty(pane) {
            true => DropTarget::Here { pane },
            false => DropTarget::Split {
                pane,
                edge: Edge::nearest(*rect, point),
            },
        })
    }

    pub fn pane(self) -> PaneId {
        match self {
            DropTarget::Split { pane, .. } | DropTarget::Here { pane } => pane,
        }
    }

    /// The box the document would get, by the tree's own rounding.
    pub fn predicted(self, rect: Bounds) -> Bounds {
        match self {
            DropTarget::Here { .. } => rect,
            DropTarget::Split { edge, .. } => edge.half(rect),
        }
    }

    /// The pending drop in words; `name` is what the pane shows.
    pub fn describe(self, name: &str) -> String {
        match self {
            DropTarget::Split { edge, .. } => format!("Split {} {name}", edge.phrase()),
            DropTarget::Here { .. } => "Open here".to_owned(),
        }
    }
}

impl Edge {
    /// The half of `rect` a pane docking on this edge takes.
    pub fn half(self, rect: Bounds) -> Bounds {
        let (axis, side) = self.placement();
        let (first, second) = rect.split(axis, 0.5);
        match side {
            Side::Before => first,
            Side::After => second,
        }
    }

    fn phrase(self) -> &'static str {
        match self {
            Edge::Left => "left of",
            Edge::Right => "right of",
            Edge::Top => "above",
            Edge::Bottom => "below",
        }
    }
}

impl Bounds {
    /// Left and top edges inside, right and bottom outside, so panes tile.
    pub fn contains(self, (x, y): (f32, f32)) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> Vec<(PaneId, Bounds)> {
        vec![
            (PaneId(1), Bounds::new(0.0, 0.0, 400.0, 600.0)),
            (PaneId(2), Bounds::new(400.0, 0.0, 401.0, 600.0)),
        ]
    }

    #[test]
    fn a_pane_with_a_document_splits_at_its_nearest_edge() {
        let at = |p| DropTarget::at(&two(), p, |_| false);
        let split = |pane, edge| Some(DropTarget::Split { pane, edge });
        assert_eq!(at((10.0, 300.0)), split(PaneId(1), Edge::Left));
        assert_eq!(at((390.0, 300.0)), split(PaneId(1), Edge::Right));
        assert_eq!(at((600.0, 590.0)), split(PaneId(2), Edge::Bottom));
        assert_eq!(at((600.0, 5.0)), split(PaneId(2), Edge::Top));
        // The shared seam belongs to the pane on its right.
        assert_eq!(at((400.0, 300.0)), split(PaneId(2), Edge::Left));
        assert_eq!(at((900.0, 300.0)), None, "outside every pane");
    }

    #[test]
    fn an_empty_pane_takes_the_document_whole() {
        let at = DropTarget::at(&two(), (450.0, 20.0), |p| p == PaneId(2));
        assert_eq!(at, Some(DropTarget::Here { pane: PaneId(2) }));
        let rect = Bounds::new(400.0, 0.0, 401.0, 600.0);
        assert_eq!(DropTarget::Here { pane: PaneId(2) }.predicted(rect), rect);
    }

    #[test]
    fn the_predicted_box_is_the_half_the_new_pane_gets() {
        let rect = Bounds::new(100.0, 50.0, 801.0, 600.0);
        let half = |edge| {
            DropTarget::Split {
                pane: PaneId(1),
                edge,
            }
            .predicted(rect)
        };
        // The first half rounds 400.5 up, so the second takes 400.
        assert_eq!(half(Edge::Left), Bounds::new(100.0, 50.0, 401.0, 600.0));
        assert_eq!(half(Edge::Right), Bounds::new(501.0, 50.0, 400.0, 600.0));
        assert_eq!(half(Edge::Top), Bounds::new(100.0, 50.0, 801.0, 300.0));
        assert_eq!(half(Edge::Bottom), Bounds::new(100.0, 350.0, 801.0, 300.0));
    }

    #[test]
    fn the_words_name_the_operation_and_the_pane() {
        let right = DropTarget::Split {
            pane: PaneId(1),
            edge: Edge::Right,
        };
        assert_eq!(right.describe("Middlemarch"), "Split right of Middlemarch");
        let below = DropTarget::Split {
            pane: PaneId(1),
            edge: Edge::Bottom,
        };
        assert_eq!(below.describe("The Raven"), "Split below The Raven");
        assert_eq!(
            DropTarget::Here { pane: PaneId(2) }.describe("x"),
            "Open here"
        );
    }
}
