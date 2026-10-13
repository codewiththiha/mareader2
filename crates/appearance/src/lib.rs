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
mod presets;
mod tokens;

pub use color::{Oklch, Rgb, hue_toward};
pub use filter::{Blend, ColorMatrix, FilterOp, MAX_FILTER_OPS, apply_filter};
pub use model::{Appearance, BaseMode, NoiseMode, TextureMode};
pub use presets::{Preset, builtin_presets};
pub use tokens::{UiTokens, base_tokens};
