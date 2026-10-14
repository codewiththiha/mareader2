//! The reading look as data and colour maths; no egui, no wgpu (L0).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod color;
mod filter;
mod model;
mod pattern;
mod presets;
mod tokens;

pub use color::{Oklch, Rgb};
pub use filter::{Blend, ColorMatrix, FilterOp, MAX_FILTER_OPS, PageTone, apply_filter};
pub use model::{Appearance, BaseMode, NoiseMode, TextureMode};
pub use pattern::{
    DOT_PITCH, GRAIN_ALPHA, GRAIN_FREQUENCY, GRAIN_TILE, LINE_PITCH, PATTERN_PERIOD, Pattern,
};
pub use presets::{Preset, builtin_presets};
pub use tokens::{UiTokens, base_tokens};
