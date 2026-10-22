//! Split-view behaviour and how panes are framed.

use mareader_appearance::Rgb;
use serde::{Deserialize, Serialize};

use crate::{Swatch, clamp};

/// What clicking a book in the library does to the open panes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryClick {
    #[default]
    Replace,
    Split,
    DragOnly,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceSettings {
    pub library_click: LibraryClick,
    /// Each pane keeps its own appearance instead of sharing one.
    pub independent_themes: bool,
    /// With independent themes, still share light, dark or dim.
    pub shared_base_mode: bool,
    pub independent_textures: bool,
    /// While split, each pane keeps its own Fonts settings.
    pub independent_text: bool,
    pub look: PaneLook,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            library_click: LibraryClick::Replace,
            independent_themes: false,
            shared_base_mode: true,
            independent_textures: false,
            independent_text: false,
            look: PaneLook::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corners {
    #[default]
    Square,
    Rounded,
}

/// How split panes are separated and outlined; one pane draws none.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaneLook {
    /// Space between panes, in points.
    pub gap: f32,
    /// The active pane's outline width; zero hides it.
    pub outline_width: f32,
    pub outline_color: Swatch,
    pub outline_custom: Rgb,
    pub corners: Corners,
    pub shadow: bool,
}

impl Default for PaneLook {
    fn default() -> Self {
        Self {
            gap: 0.0,
            // A seam shows the split; the outline only marks focus.
            outline_width: 1.0,
            outline_color: Swatch::Auto,
            outline_custom: Rgb(0x6b, 0xa3, 0xf5),
            corners: Corners::Square,
            shadow: false,
        }
    }
}

impl PaneLook {
    pub const MAX_GAP: f32 = 24.0;
    pub const MAX_OUTLINE: f32 = 8.0;
    /// The gap opened for corners or a shadow, which touching panes hide.
    pub const EDGE_GAP: f32 = 8.0;

    /// Opens a gap if panes touch, so corners and shadows can show.
    pub fn make_room(&mut self) {
        if self.gap < 1.0 {
            self.gap = Self::EDGE_GAP;
        }
    }

    pub fn sanitize(&mut self) {
        let d = Self::default();
        self.gap = clamp(self.gap, 0.0, Self::MAX_GAP, d.gap);
        let width = clamp(self.outline_width, 0.0, Self::MAX_OUTLINE, d.outline_width);
        self.outline_width = width;
    }

    pub fn rounded(&self) -> bool {
        self.corners == Corners::Rounded
    }

    /// The outline colour, or `None` to follow the theme's accent.
    pub fn outline_rgb(&self) -> Option<Rgb> {
        self.outline_color.rgb(self.outline_custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_and_shadows_open_a_gap_only_when_panes_touch() {
        let mut look = PaneLook::default();
        look.make_room();
        assert!((look.gap - PaneLook::EDGE_GAP).abs() < f32::EPSILON);
        look.gap = 3.0;
        look.make_room();
        assert!((look.gap - 3.0).abs() < f32::EPSILON, "a chosen gap stays");
    }
}
