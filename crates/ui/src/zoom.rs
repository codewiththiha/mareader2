//! Zoom: the preset ladder, the fit modes, and the zoom a pane reports.

use egui::Vec2;
use mareader_session::PageSize;

pub const MIN: f32 = 0.25;
pub const MAX: f32 = 5.0;

/// The ladder `+` and `-` climb; 25 to 500 %.
pub const PRESETS: [f32; 15] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

/// Zooms closer than this are the same zoom.
const SAME: f32 = 0.005;

/// A zoom that follows the pane's size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Width,
    Page,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    In,
    Out,
}

/// What the user asked of a pane's zoom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomCommand {
    Step(Step),
    Fit(Fit),
}

/// A pane's zoom, as the menu shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomState {
    pub zoom: f32,
    pub fit: Option<Fit>,
}

impl ZoomState {
    /// `125%`; the target, so a running tween doesn't flicker digits.
    pub fn percent(&self) -> String {
        format!("{:.0}%", self.zoom * 100.0)
    }

    pub fn can_step(&self, step: Step) -> bool {
        next(self.zoom, step).is_some()
    }
}

/// Garbage collapses to the smallest zoom, never into geometry.
pub fn clamp(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(MIN, MAX)
    } else {
        MIN
    }
}

/// The next preset beyond `zoom`; none past either end.
pub fn next(zoom: f32, step: Step) -> Option<f32> {
    match step {
        Step::In => PRESETS.iter().copied().find(|p| *p > zoom + SAME),
        Step::Out => PRESETS.iter().rev().copied().find(|p| *p < zoom - SAME),
    }
}

/// The zoom that fits `page` into `room`, edge to edge.
pub fn fit(fit: Fit, room: Vec2, page: PageSize) -> f32 {
    let width = room.x.max(1.0) / page.width.max(1.0);
    let height = room.y.max(1.0) / page.height.max(1.0);
    clamp(match fit {
        Fit::Width => width,
        Fit::Page => width.min(height),
    })
}

/// Whether two zooms differ enough to lay out again.
pub fn differs(a: f32, b: f32) -> bool {
    (a - b).abs() > 1e-4
}

/// A zoom step's glide, seconds; linear, as in mareader.
const GLIDE: f64 = 0.12;
/// Quiet after a pinch or resize before pages render sharp again.
const SETTLE: f64 = 0.18;

/// A zoom step in flight: it glides from `from` to the target.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Glide {
    from: f32,
    start: f64,
}

/// One pane's zoom: what was asked, what shows, what renders.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneZoom {
    /// While set, the zoom follows the pane's size.
    pub fit: Option<Fit>,
    /// The page a fit measures: the one read when it was chosen.
    pub fit_page: usize,
    /// The zoom asked for; a fit recomputes it every frame.
    target: f32,
    glide: Option<Glide>,
    /// Pages render at this zoom; it trails pinches until they settle.
    sharp: f32,
    settle_at: f64,
    /// Where a pinch pivots, in the viewport; else its centre.
    pub pivot: Option<Vec2>,
    /// The room a fit last measured, and when it last changed.
    room: Vec2,
    room_at: f64,
}

impl Default for PaneZoom {
    fn default() -> Self {
        Self {
            fit: Some(Fit::Width),
            fit_page: 0,
            target: 1.0,
            glide: None,
            sharp: 1.0,
            settle_at: 0.0,
            pivot: None,
            room: Vec2::ZERO,
            room_at: 0.0,
        }
    }
}

impl PaneZoom {
    pub fn state(&self) -> ZoomState {
        ZoomState {
            zoom: self.target,
            fit: self.fit,
        }
    }

    /// Steps glide from `shown` about the centre and render sharp at once.
    pub fn command(&mut self, command: ZoomCommand, shown: Option<f32>, page: usize, now: f64) {
        match command {
            ZoomCommand::Step(step) => {
                let Some(zoom) = next(self.target, step) else {
                    return;
                };
                self.target = zoom;
                self.fit = None;
                self.fit_page = page;
            }
            ZoomCommand::Fit(fit) => {
                self.fit = Some(fit);
                self.fit_page = page;
            }
        }
        self.glide = shown.map(|from| Glide { from, start: now });
        self.pivot = None;
        self.settle_at = now;
    }

    /// Keeps the zoom a fit reached, no longer following the pane.
    pub fn unfit(&mut self) {
        self.fit = None;
    }

    /// A pinch or Ctrl+wheel: follows the fingers, pivots on the pointer.
    pub fn pinch(&mut self, factor: f32, at: Vec2, now: f64) {
        self.target = clamp(self.target * factor);
        self.fit = None;
        self.glide = None;
        self.pivot = Some(at);
        self.settle_at = now + SETTLE;
    }

    /// A set zoom scaled by `factor`, to keep its scale on another page.
    pub fn rescale(&mut self, factor: f32, now: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        self.target = clamp(self.target * factor);
        self.glide = None;
        self.pivot = None;
        self.settle_at = self.settle_at.max(now + SETTLE);
    }

    /// A fit follows the pane; without `follow`, returns the wait for rest.
    pub fn refit(
        &mut self,
        page: Option<PageSize>,
        room: Vec2,
        shown: bool,
        follow: bool,
        now: f64,
    ) -> Option<f32> {
        let (Some(mode), Some(page)) = (self.fit, page) else {
            return None;
        };
        if self.room != room {
            self.room = room;
            self.room_at = now;
        }
        let quiet = now - self.room_at;
        if !follow && shown && quiet < SETTLE {
            return Some((SETTLE - quiet) as f32);
        }
        let fitted = fit(mode, room, page);
        if differs(fitted, self.target) && self.glide.is_none() && shown {
            self.settle_at = self.settle_at.max(now + SETTLE);
        }
        self.target = fitted;
        None
    }

    /// This frame's zoom, advancing a glide; true while one runs.
    pub fn advance(&mut self, now: f64) -> (f32, bool) {
        let Some(g) = self.glide else {
            return (self.target, false);
        };
        let t = ((now - g.start) / GLIDE) as f32;
        if !(0.0..1.0).contains(&t) {
            self.glide = None;
            return (self.target, false);
        }
        (g.from + (self.target - g.from) * t, true)
    }

    /// Moves the render zoom to the target once settled; else the wait.
    pub fn sharpen(&mut self, now: f64) -> Option<f32> {
        if now >= self.settle_at {
            self.sharp = self.target;
            None
        } else {
            Some((self.settle_at - now) as f32)
        }
    }

    pub fn sharp(&self) -> f32 {
        self.sharp
    }

    /// A pinch's pivot lasts until the zoom comes to rest.
    pub fn end_frame(&mut self, gliding: bool, now: f64) {
        if !gliding && now >= self.settle_at {
            self.pivot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: PageSize = PageSize {
        width: 612.0,
        height: 792.0,
    };

    #[test]
    fn steps_climb_to_the_next_preset_and_stop_at_the_ends() {
        assert_eq!(next(1.0, Step::In), Some(1.25));
        assert_eq!(next(1.0, Step::Out), Some(0.9));
        // Off the ladder, the nearest preset that way, never a skip.
        assert_eq!(next(1.2, Step::In), Some(1.25));
        assert_eq!(next(1.2, Step::Out), Some(1.0));
        assert_eq!(next(MAX, Step::In), None);
        assert_eq!(next(MIN, Step::Out), None);
        // Float noise from a fit is still the preset itself.
        assert_eq!(next(1.249, Step::In), Some(1.5));
    }

    #[test]
    fn chained_steps_walk_the_whole_ladder() {
        let mut zoom = MIN;
        let mut seen = vec![zoom];
        while let Some(z) = next(zoom, Step::In) {
            zoom = z;
            seen.push(z);
        }
        assert_eq!(seen, PRESETS);
    }

    #[test]
    fn fit_page_takes_the_tighter_side() {
        let room = Vec2::new(612.0, 2000.0);
        assert!((fit(Fit::Width, room, LETTER) - 1.0).abs() < 1e-6);
        assert!((fit(Fit::Page, room, LETTER) - 1.0).abs() < 1e-6);
        let short = Vec2::new(612.0, 396.0);
        assert!((fit(Fit::Page, short, LETTER) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn fits_clamp_and_garbage_does_not_leak() {
        assert_eq!(fit(Fit::Width, Vec2::new(10.0, 10.0), LETTER), MIN);
        assert_eq!(fit(Fit::Width, Vec2::new(1e6, 10.0), LETTER), MAX);
        assert_eq!(clamp(f32::NAN), MIN);
        let state = ZoomState {
            zoom: 1.25,
            fit: None,
        };
        assert_eq!(state.percent(), "125%");
        assert!(
            !ZoomState {
                zoom: MAX,
                fit: None
            }
            .can_step(Step::In)
        );
    }

    #[test]
    fn steps_glide_and_leave_the_fit() {
        let mut z = PaneZoom::default();
        z.command(ZoomCommand::Step(Step::In), Some(1.0), 0, 10.0);
        assert_eq!(
            z.state(),
            ZoomState {
                zoom: 1.25,
                fit: None
            }
        );
        let (mid, gliding) = z.advance(10.0 + GLIDE / 2.0);
        assert!(gliding && (mid - 1.125).abs() < 1e-3, "{mid}");
        assert_eq!(z.advance(10.0 + GLIDE), (1.25, false));
        // A step renders sharp straight away.
        assert_eq!(z.sharpen(10.0), None);
        assert_eq!(z.sharp(), 1.25);
        z.command(ZoomCommand::Fit(Fit::Page), Some(1.25), 4, 11.0);
        assert_eq!((z.fit, z.fit_page), (Some(Fit::Page), 4));
    }

    #[test]
    fn a_pinch_stretches_until_it_settles() {
        let mut z = PaneZoom::default();
        z.pinch(1.1, Vec2::new(10.0, 10.0), 3.0);
        assert!((z.target - 1.1).abs() < 1e-6);
        assert_eq!(z.fit, None);
        assert!(z.sharpen(3.05).is_some());
        assert_eq!(z.sharp(), 1.0);
        assert_eq!(z.sharpen(3.0 + SETTLE), None);
        assert!((z.sharp() - 1.1).abs() < 1e-6);
        z.pinch(100.0, Vec2::ZERO, 4.0);
        assert_eq!(z.target, MAX);
    }

    #[test]
    fn a_fit_follows_the_room() {
        let mut z = PaneZoom::default();
        z.refit(Some(LETTER), Vec2::new(612.0, 900.0), false, true, 0.0);
        assert!((z.state().zoom - 1.0).abs() < 1e-6);
        // A resize refits now and sharpens later.
        z.refit(Some(LETTER), Vec2::new(1224.0, 900.0), true, true, 1.0);
        assert!((z.state().zoom - 2.0).abs() < 1e-6);
        assert!(z.sharpen(1.0).is_some());
    }

    #[test]
    fn without_follow_a_fit_waits_for_the_resize_to_rest() {
        let mut z = PaneZoom::default();
        z.refit(Some(LETTER), Vec2::new(612.0, 900.0), false, false, 0.0);
        let wide = Vec2::new(1224.0, 900.0);
        // Still dragging: the zoom holds, and says how long to wait.
        assert!(z.refit(Some(LETTER), wide, true, false, 1.0).is_some());
        assert!((z.state().zoom - 1.0).abs() < 1e-6);
        assert!(z.refit(Some(LETTER), wide, true, false, 1.1).is_some());
        // At rest: one fit.
        let rested = 1.01 + SETTLE;
        assert_eq!(z.refit(Some(LETTER), wide, true, false, rested), None);
        assert!((z.state().zoom - 2.0).abs() < 1e-6);
    }
}
