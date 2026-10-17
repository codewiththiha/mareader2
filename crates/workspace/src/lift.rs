//! Lifting a pane: where it would go if it were let go now.

use crate::tree::{Bounds, Edge, MIN_PANE_PX, PaneId};

/// Where a lifted pane would land.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LiftTarget {
    /// Trade places with the pane.
    Swap(PaneId),
    /// Dock on an edge of the pane, taking half its box.
    Dock(PaneId, Edge),
}

impl LiftTarget {
    pub fn pane(self) -> PaneId {
        match self {
            LiftTarget::Swap(pane) | LiftTarget::Dock(pane, _) => pane,
        }
    }

    /// The box the lifted pane would get, given the target's.
    pub fn predicted(self, rect: Bounds) -> Bounds {
        match self {
            LiftTarget::Swap(_) => rect,
            LiftTarget::Dock(_, edge) => edge.half(rect),
        }
    }

    /// The pending move in words, for the preview.
    pub fn describe(self) -> &'static str {
        match self {
            LiftTarget::Swap(_) => "Release to swap panes",
            LiftTarget::Dock(_, Edge::Left) => "Release to move left of this pane",
            LiftTarget::Dock(_, Edge::Right) => "Release to move right of this pane",
            LiftTarget::Dock(_, Edge::Top) => "Release to move above this pane",
            LiftTarget::Dock(_, Edge::Bottom) => "Release to move below this pane",
        }
    }
}

/// The target under `point`: the middle third swaps, else the nearest
/// edge with room for half docks; too small to halve, it swaps.
pub fn resolve(
    lifted: PaneId,
    point: (f32, f32),
    panes: &[(PaneId, Bounds)],
) -> Option<LiftTarget> {
    let (pane, rect) = *panes.iter().find(|(_, r)| r.contains(point))?;
    if pane == lifted {
        return None;
    }
    let fx = (point.0 - rect.x) / rect.w.max(1.0);
    let fy = (point.1 - rect.y) / rect.h.max(1.0);
    let middle = |f: f32| (1.0 / 3.0..=2.0 / 3.0).contains(&f);
    if middle(fx) && middle(fy) {
        return Some(LiftTarget::Swap(pane));
    }
    let fits = |edge: Edge| match edge {
        Edge::Left | Edge::Right => rect.w / 2.0 >= MIN_PANE_PX,
        Edge::Top | Edge::Bottom => rect.h / 2.0 >= MIN_PANE_PX,
    };
    let edges = [
        (Edge::Left, fx),
        (Edge::Right, 1.0 - fx),
        (Edge::Top, fy),
        (Edge::Bottom, 1.0 - fy),
    ];
    let nearest = edges
        .into_iter()
        .filter(|(edge, _)| fits(*edge))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    Some(match nearest {
        Some((edge, _)) => LiftTarget::Dock(pane, edge),
        None => LiftTarget::Swap(pane),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> Vec<(PaneId, Bounds)> {
        vec![
            (PaneId(1), Bounds::new(0.0, 0.0, 600.0, 800.0)),
            (PaneId(2), Bounds::new(600.0, 0.0, 600.0, 800.0)),
        ]
    }

    #[test]
    fn over_the_lifted_pane_or_outside_there_is_no_target() {
        assert_eq!(resolve(PaneId(1), (300.0, 400.0), &pair()), None);
        assert_eq!(resolve(PaneId(1), (5000.0, 400.0), &pair()), None);
    }

    #[test]
    fn a_pane_docks_beside_the_nearest_edge_and_swaps_in_the_middle() {
        let at = |p| resolve(PaneId(1), p, &pair());
        assert_eq!(
            at((900.0, 20.0)),
            Some(LiftTarget::Dock(PaneId(2), Edge::Top))
        );
        assert_eq!(
            at((1150.0, 400.0)),
            Some(LiftTarget::Dock(PaneId(2), Edge::Right))
        );
        assert_eq!(
            at((650.0, 500.0)),
            Some(LiftTarget::Dock(PaneId(2), Edge::Left))
        );
        assert_eq!(at((900.0, 400.0)), Some(LiftTarget::Swap(PaneId(2))));
    }

    #[test]
    fn a_pane_too_narrow_to_halve_docks_on_the_other_axis_or_swaps() {
        let narrow = vec![
            (PaneId(1), Bounds::new(0.0, 0.0, 850.0, 800.0)),
            (PaneId(2), Bounds::new(850.0, 0.0, 350.0, 800.0)),
        ];
        let at = |p| resolve(PaneId(1), p, &narrow);
        assert_eq!(
            at((1190.0, 700.0)),
            Some(LiftTarget::Dock(PaneId(2), Edge::Bottom))
        );
        let tiny = vec![(PaneId(2), Bounds::new(850.0, 0.0, 350.0, 300.0))];
        let at = resolve(PaneId(1), (1190.0, 10.0), &tiny);
        assert_eq!(at, Some(LiftTarget::Swap(PaneId(2))));
    }

    #[test]
    fn a_dock_predicts_half_the_target() {
        let rect = Bounds::new(0.0, 0.0, 600.0, 800.0);
        let half = LiftTarget::Dock(PaneId(2), Edge::Bottom).predicted(rect);
        assert_eq!((half.y, half.h), (400.0, 400.0));
        assert_eq!(LiftTarget::Swap(PaneId(2)).predicted(rect), rect);
    }
}
