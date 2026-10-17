//! Dragging empty space pans the pages; a quick release flings them.

use egui::style::ScrollAnimation;
use egui::{CursorIcon, Response, Ui, Vec2};

/// A release this long (s) after the last movement rests, not flings.
const REST: f32 = 0.08;
/// Below this speed (points a second) a fling has stopped.
const STOP: f32 = 20.0;
/// The share of a fling's speed kept each 16 ms.
const DECAY: f32 = 0.94;

#[derive(Clone, Copy, Default)]
struct Grab {
    panning: bool,
    /// Points a second the pages still glide.
    fling: Vec2,
}

/// Pans while a drag begun on empty space lasts; true meanwhile.
pub fn pan(ui: &Ui, response: &Response, from_empty: bool) -> bool {
    let id = ui.id().with("grab");
    let mut g: Grab = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let (pressed, dt, speed, still, origin) = ui.input(|i| {
        let p = &i.pointer;
        let dt = i.stable_dt.min(0.1);
        let still = p.time_since_last_movement();
        (p.any_pressed(), dt, p.velocity(), still, p.press_origin())
    });
    if pressed {
        g.fling = Vec2::ZERO;
    }
    if response.drag_started() && from_empty {
        g.panning = true;
    }
    let panning = g.panning;
    if panning {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        // The first step makes up the distance before a drag counts.
        let at = response.interact_pointer_pos();
        let delta = match (response.drag_started(), origin, at) {
            (true, Some(o), Some(p)) => p - o,
            _ => response.drag_delta(),
        };
        ui.scroll_with_delta_animation(delta, ScrollAnimation::none());
        if !response.dragged() {
            g.panning = false;
            if still < REST {
                g.fling = speed;
            }
        }
    } else if g.fling != Vec2::ZERO {
        ui.scroll_with_delta_animation(g.fling * dt, ScrollAnimation::none());
        g.fling *= DECAY.powf(dt / 0.016);
        if g.fling.length() < STOP {
            g.fling = Vec2::ZERO;
        }
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(id, g));
    panning
}
