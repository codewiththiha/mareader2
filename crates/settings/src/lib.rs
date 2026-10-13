//! Every user setting as typed data, with ranges and loading (L2).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod load;
mod mareader;
mod reader;
mod study;
mod swatch;
mod text;
mod workspace;

use mareader_appearance::{Appearance, Preset, builtin_presets};
use serde::{Deserialize, Serialize};

pub use load::{Loaded, load, to_json};
pub use mareader::{ImportError, import_mareader};
pub use reader::{
    ChromeSettings, FloatingLabelStyle, LayoutSettings, MotionSettings, PageIndicatorStyle,
    PaperArea, PaperSettings, StartupFit,
};
pub use study::StudySettings;
pub use study::{CefrLevel, CefrSettings, DictionarySettings, GlossDensity, GlossSettings};
pub use swatch::Swatch;
pub use text::{ColumnAlign, FontChoice, FontFamily, SystemFont, TextSettings};
pub use workspace::{Corners, LibraryClick, PaneLook, WorkspaceSettings};

/// Bumped with every migration step in `load.rs`.
pub const SCHEMA_VERSION: u32 = 1;

/// The whole persisted state of the user's choices.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub appearance: Appearance,
    /// The user's own presets; the built-ins live in code.
    pub user_presets: Vec<Preset>,
    pub chrome: ChromeSettings,
    pub layout: LayoutSettings,
    pub paper: PaperSettings,
    pub motion: MotionSettings,
    /// EPUB typography; PDFs never read it.
    pub text: TextSettings,
    pub study: StudySettings,
    pub dictionary: DictionarySettings,
    pub workspace: WorkspaceSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            appearance: Appearance::default(),
            user_presets: Vec::new(),
            chrome: ChromeSettings::default(),
            layout: LayoutSettings::default(),
            paper: PaperSettings::default(),
            motion: MotionSettings::default(),
            text: TextSettings::default(),
            study: StudySettings::default(),
            dictionary: DictionarySettings::default(),
            workspace: WorkspaceSettings::default(),
        }
    }
}

impl Settings {
    /// Pulls every value into its range; runs on load and on every edit.
    pub fn sanitize(&mut self) {
        self.version = SCHEMA_VERSION;
        self.appearance.sanitize();
        self.layout.sanitize();
        self.text.sanitize();
        self.study.sanitize();
        self.workspace.look.sanitize();
        let builtin: Vec<String> = builtin_presets().into_iter().map(|p| p.id).collect();
        self.user_presets.retain(|p| {
            !p.id.trim().is_empty() && !p.name.trim().is_empty() && !builtin.contains(&p.id)
        });
        for p in &mut self.user_presets {
            p.appearance.sanitize();
        }
    }

    /// Built-ins first, then the user's own: the menu's order.
    pub fn all_presets(&self) -> Vec<Preset> {
        let mut all = builtin_presets();
        all.extend(self.user_presets.iter().cloned());
        all
    }
}

/// `f32::clamp` that also turns NaN and infinities into `fallback`.
pub(crate) fn clamp(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_trip_keeps_every_field() {
        let mut s = Settings::default();
        s.chrome.sidebar_overlay = true;
        s.motion.zoom = false;
        s.text.font_size = 21.0;
        s.text.serif_font = FontChoice::System(SystemFont::Georgia);
        s.workspace.look.outline_color = Swatch::Custom;
        s.dictionary.langs = vec!["en-my".into()];
        let back = load(&to_json(&s)).settings;
        assert_eq!(back, s);
    }

    #[test]
    fn sanitize_clamps_and_drops_bad_presets() {
        let mut s = Settings::default();
        s.layout.page_margin = f32::NAN;
        s.workspace.look.gap = 99.0;
        let mut shadow = builtin_presets().remove(0);
        shadow.name = "Mine".into();
        s.user_presets = vec![shadow];
        s.sanitize();
        assert_eq!(s.layout.page_margin, 0.0);
        assert_eq!(s.workspace.look.gap, PaneLook::MAX_GAP);
        assert!(s.user_presets.is_empty(), "a built-in id can't be shadowed");
    }
}
