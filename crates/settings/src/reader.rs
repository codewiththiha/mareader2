//! How the reader and its chrome behave: bars, layout, paper, motion.

use serde::{Deserialize, Serialize};

use crate::clamp;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChromeSettings {
    /// Keeps the reader's title bar visible instead of revealing on hover.
    pub titlebar_pinned: bool,
    pub library_titlebar_pinned: bool,
    /// The sidebar floats over the page rather than pushing it aside.
    pub sidebar_overlay: bool,
}

impl Default for ChromeSettings {
    fn default() -> Self {
        Self {
            titlebar_pinned: false,
            library_titlebar_pinned: true,
            sidebar_overlay: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageIndicatorStyle {
    #[default]
    PageNumber,
    Percentage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatingLabelStyle {
    #[default]
    FileName,
    Chapter,
}

/// How a freshly opened document is zoomed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupFit {
    Page,
    #[default]
    Width,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutSettings {
    pub page_indicator: bool,
    pub page_indicator_style: PageIndicatorStyle,
    pub floating_label: bool,
    pub floating_label_style: FloatingLabelStyle,
    /// Keeps the label shown instead of fading it while idle.
    pub floating_label_persist: bool,
    /// Widest the label may grow, as a percent of the pane.
    pub floating_label_max_pct: f32,
    pub progress_bar: bool,
    pub startup_fit: StartupFit,
    /// Zoom used when `startup_fit` can't decide, e.g. no page yet.
    pub default_zoom: f32,
    pub no_gap: bool,
    /// Re-fits the zoom when the pane resizes.
    pub auto_scale: bool,
    pub auto_resize: bool,
    pub page_shadow: bool,
    /// Extra margin around pages, in points.
    pub page_margin: f32,
    /// Page column width as a percent of the fitted width.
    pub column_width_pct: f32,
}

impl LayoutSettings {
    pub const ZOOM: (f32, f32) = (0.25, 5.0);
    pub const MARGIN: (f32, f32) = (0.0, 64.0);
    pub const COLUMN_PCT: (f32, f32) = (60.0, 140.0);
    pub const LABEL_PCT: (f32, f32) = (10.0, 100.0);

    pub fn sanitize(&mut self) {
        let d = Self::default();
        let (lo, hi) = Self::ZOOM;
        self.default_zoom = clamp(self.default_zoom, lo, hi, d.default_zoom);
        let (lo, hi) = Self::MARGIN;
        self.page_margin = clamp(self.page_margin, lo, hi, d.page_margin);
        let (lo, hi) = Self::COLUMN_PCT;
        self.column_width_pct = clamp(self.column_width_pct, lo, hi, d.column_width_pct);
        let (lo, hi) = Self::LABEL_PCT;
        let max = clamp(
            self.floating_label_max_pct,
            lo,
            hi,
            d.floating_label_max_pct,
        );
        self.floating_label_max_pct = max;
    }
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            page_indicator: true,
            page_indicator_style: PageIndicatorStyle::PageNumber,
            floating_label: true,
            floating_label_style: FloatingLabelStyle::FileName,
            floating_label_persist: false,
            floating_label_max_pct: 100.0,
            progress_bar: true,
            startup_fit: StartupFit::Width,
            default_zoom: 1.0,
            no_gap: false,
            auto_scale: true,
            auto_resize: true,
            page_shadow: true,
            page_margin: 0.0,
            column_width_pct: 100.0,
        }
    }
}

/// Where paper blending tints a PDF page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperArea {
    #[default]
    WholePage,
    Edges,
}

/// Blends PDF pages into the themed paper instead of showing white.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaperSettings {
    pub blend: bool,
    pub area: PaperArea,
}

/// `enabled` gates the rest; each switch keeps its value while off.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MotionSettings {
    pub enabled: bool,
    pub sidebar_slide: bool,
    pub canvas_resize: bool,
    pub zoom: bool,
    pub scroll_jumps: bool,
}

impl MotionSettings {
    pub fn sidebar_slide(&self) -> bool {
        self.enabled && self.sidebar_slide
    }

    pub fn canvas_resize(&self) -> bool {
        self.enabled && self.canvas_resize
    }

    pub fn zoom(&self) -> bool {
        self.enabled && self.zoom
    }

    pub fn scroll_jumps(&self) -> bool {
        self.enabled && self.scroll_jumps
    }
}

impl Default for MotionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sidebar_slide: true,
            canvas_resize: true,
            zoom: true,
            scroll_jumps: true,
        }
    }
}
