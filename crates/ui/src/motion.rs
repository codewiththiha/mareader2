//! The Animations master. Off skips the in-between frames, never the end.

const ID: &str = "mareader-motion-off";

pub fn set_enabled(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(ID), !on));
    ctx.global_style_mut(|s| s.animation_time = if on { 1.0 / 12.0 } else { 0.0 });
}

pub fn enabled(ctx: &egui::Context) -> bool {
    !ctx.data(|d| d.get_temp::<bool>(egui::Id::new(ID)))
        .unwrap_or(false)
}

/// `secs` while animations are on, zero while they are off.
pub fn secs(ctx: &egui::Context, secs: f32) -> f32 {
    if enabled(ctx) { secs } else { 0.0 }
}
