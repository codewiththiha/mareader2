//! Theme changes cross-fade: the old frame fades out over the new.

// As the web's view transitions: snap the frame, change, fade the snap.

use std::sync::Arc;

use egui::{
    Color32, ColorImage, Context, Event, Id, LayerId, Order, Rect, TextureHandle, TextureOptions,
    UserData, ViewportCommand, pos2,
};

/// How long the old frame takes to fade out, in seconds.
pub const FADE: f32 = 0.2;
/// Frames to wait for the snapshot before changing without one.
const PATIENCE: u32 = 4;

/// Tags this module's screenshot requests.
struct Snapshot;

/// A change held for its snapshot, then the fade over it.
pub struct Crossfade<T> {
    state: State<T>,
}

impl<T> Default for Crossfade<T> {
    fn default() -> Self {
        Self { state: State::Idle }
    }
}

enum State<T> {
    Idle,
    /// The change waits for a snapshot of the frame before it.
    Waiting {
        after: T,
        frames: u32,
    },
    Fading {
        image: TextureHandle,
        start: f64,
    },
}

impl<T> Crossfade<T> {
    /// Holds `after` back and asks for a snapshot of the current look.
    pub fn hold(&mut self, ctx: &Context, after: T) {
        ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::new(Snapshot)));
        self.state = State::Waiting { after, frames: 0 };
        ctx.request_repaint();
    }

    /// The held change, now due: its snapshot came, or waiting ran out.
    pub fn poll(&mut self, ctx: &Context) -> Option<T> {
        let State::Waiting { frames, .. } = &mut self.state else {
            return None;
        };
        let image = ctx.input(|i| i.events.iter().find_map(snapshot));
        if image.is_none() && *frames < PATIENCE {
            *frames += 1;
            ctx.request_repaint();
            return None;
        }
        let state = std::mem::replace(&mut self.state, State::Idle);
        let State::Waiting { after, .. } = state else {
            return None;
        };
        if let Some(image) = image {
            let image = Arc::unwrap_or_clone(image);
            let image = ctx.load_texture("crossfade", image, TextureOptions::LINEAR);
            let start = ctx.input(|i| i.time);
            self.state = State::Fading { image, start };
        }
        Some(after)
    }

    /// The held change, given up on because another lands over it.
    pub fn take(&mut self) -> Option<T> {
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Waiting { after, .. } => Some(after),
            other => {
                self.state = other;
                None
            }
        }
    }

    pub fn is_fading(&self) -> bool {
        matches!(self.state, State::Fading { .. })
    }

    /// Lays the old frame over everything, fading, till it is gone.
    pub fn paint(&mut self, ctx: &Context) {
        let State::Fading { image, start } = &self.state else {
            return;
        };
        let t = ((ctx.input(|i| i.time) - start) as f32 / FADE).clamp(0.0, 1.0);
        if t >= 1.0 {
            self.state = State::Idle;
            return;
        }
        // Ease out: most of the change shows early.
        let left = (1.0 - t) * (1.0 - t);
        let layer = LayerId::new(Order::Debug, Id::new("crossfade"));
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let tint = Color32::WHITE.gamma_multiply(left);
        let painter = ctx.layer_painter(layer);
        painter.image(image.id(), ctx.viewport_rect(), uv, tint);
        ctx.request_repaint();
    }
}

fn snapshot(event: &Event) -> Option<Arc<ColorImage>> {
    match event {
        Event::Screenshot {
            user_data, image, ..
        } => {
            let data = user_data.data.as_ref()?;
            data.downcast_ref::<Snapshot>().map(|_| Arc::clone(image))
        }
        _ => None,
    }
}
