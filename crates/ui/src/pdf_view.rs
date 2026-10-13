//! A PDF in a pane: a scrolling column of pages, drawn as they arrive.

use egui::emath::GuiRounding as _;
use egui::scroll_area::{DragScroll, ScrollSource};
use egui::{Color32, CornerRadius, Rect, ScrollArea, Ui, pos2, vec2};
use mareader_session::{DocId, Document, PageKey, PageSize, Scale};

use crate::pages::{Lookup, PageCache};
use crate::theme::Tokens;

pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 5.0;
/// Between pages, and around the column.
const GAP: f32 = 12.0;
const MARGIN: f32 = 16.0;
/// Pages drawn ahead of the viewport, each way.
const PREFETCH: usize = 1;

/// How soon a page is needed; lower is sooner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    ActivePane,
    OtherPane,
    Prefetch,
}

/// The frame's page requests from every pane.
pub type Demand = Vec<(Urgency, PageKey)>;

/// One pane's view of a document; the document itself is shared.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfView {
    pub doc: DocId,
    /// Fit width sizes to this page: the one read when the fit was set.
    pub fit_page: usize,
    layout: Option<Column>,
    /// A horizontal offset to reach, kept until the area takes it.
    recentre: Option<f32>,
    /// Pages on screen last frame.
    pub visible: std::ops::Range<usize>,
}

impl PdfView {
    pub fn new(doc: DocId) -> Self {
        Self {
            doc,
            fit_page: 0,
            layout: None,
            recentre: None,
            visible: 0..0,
        }
    }
}

/// What the pane needs from the app this frame.
pub struct ViewContext {
    pub active: bool,
    /// Space a pinned title bar takes from the pane's top.
    pub top_inset: f32,
}

/// Page rects in content space; rebuilt only when its inputs change.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    inputs: (f32, f32, f32),
    pub zoom: f32,
    pub rects: Vec<Rect>,
    pub size: egui::Vec2,
}

impl Column {
    /// Fit width: one zoom for every page, so text stays one size.
    pub fn fit_width(sizes: &[PageSize], fit_page: usize, width: f32, top: f32) -> Self {
        let fit = sizes
            .get(fit_page)
            .or(sizes.first())
            .map_or(612.0, |s| s.width);
        let zoom = ((width - 2.0 * MARGIN) / fit).clamp(MIN_ZOOM, MAX_ZOOM);
        let widest = sizes.iter().map(|s| s.width).fold(0.0, f32::max) * zoom;
        let content_w = width.max(widest + 2.0 * MARGIN);
        let mut y = top;
        let rects = sizes
            .iter()
            .map(|s| {
                let size = vec2(s.width, s.height) * zoom;
                let r = Rect::from_min_size(pos2((content_w - size.x) / 2.0, y), size);
                y += size.y + GAP;
                r
            })
            .collect();
        Self {
            inputs: (width, top, fit),
            zoom,
            rects,
            size: vec2(content_w, y - GAP + MARGIN),
        }
    }

    /// Indices of the pages meeting `viewport`, in content space.
    pub fn visible(&self, viewport: Rect) -> std::ops::Range<usize> {
        let first = self.rects.partition_point(|r| r.bottom() <= viewport.top());
        let end = self.rects.partition_point(|r| r.top() < viewport.bottom());
        first..end.max(first)
    }
}

pub fn show(
    ui: &mut Ui,
    view: &mut PdfView,
    doc: &Document,
    pages: &mut PageCache,
    demand: &mut Demand,
    cx: &ViewContext,
) {
    let width = ui.max_rect().width();
    let top = MARGIN + cx.top_inset;
    let sizes = doc.page_sizes();
    let fit = sizes.get(view.fit_page).map_or(612.0, |s| s.width);
    let stale = view
        .layout
        .as_ref()
        .is_none_or(|c| c.inputs != (width, top, fit));
    if stale {
        view.layout = Some(Column::fit_width(sizes, view.fit_page, width, top));
    }
    let Some(column) = &view.layout else {
        return;
    };
    // A refit centres the column; wider pages overflow both sides evenly.
    if stale {
        view.recentre = Some((column.size.x - width).max(0.0) / 2.0);
    }
    let ppp = ui.ctx().pixels_per_point();
    let wanted = Scale::covering(column.zoom * ppp);
    let max_side = ui.ctx().input(|i| i.max_texture_side) as f32;
    // Big pages at high zoom stop at the GPU's and hayro's limits.
    let key = |page: usize| {
        let long = sizes.get(page).map_or(1.0, |s| s.width.max(s.height));
        let cap = doc.fitted_scale(page, max_side / long).unwrap_or(1.0);
        PageKey {
            doc: view.doc,
            page,
            scale: wanted.min(Scale::within(cap)),
        }
    };
    let urgency = if cx.active {
        Urgency::ActivePane
    } else {
        Urgency::OtherPane
    };
    let t = Tokens::get(ui.ctx());
    let mut area = ScrollArea::both().id_salt(("pdf", view.doc));
    if let Some(x) = view.recentre {
        area = area.horizontal_scroll_offset(x);
    }
    let out = area
        .auto_shrink(false)
        .content_margin(0.0)
        // Dragging will select text, not scroll.
        .scroll_source(ScrollSource {
            drag: DragScroll::Never,
            ..Default::default()
        })
        .show_viewport(ui, |ui, viewport| {
            ui.set_min_size(column.size);
            let origin = ui.max_rect().min.to_vec2();
            let shown = column.visible(viewport);
            view.visible = shown.clone();
            for page in shown.clone() {
                let Some(rect) = column.rects.get(page) else {
                    continue;
                };
                let rect = rect.translate(origin).round_to_pixels(ppp);
                let look = pages.lookup(key(page));
                paint_page(ui, rect, look, &t);
                if !matches!(look, Lookup::Sharp(_) | Lookup::Failed) {
                    demand.push((urgency, key(page)));
                }
            }
            let before = shown.start.saturating_sub(PREFETCH)..shown.start;
            let after = shown.end..(shown.end + PREFETCH).min(column.rects.len());
            for page in before.chain(after) {
                if !pages.touch(key(page)) {
                    demand.push((Urgency::Prefetch, key(page)));
                }
            }
        });
    // A new area clamps offsets to last frame's size, so retry once.
    if let Some(x) = view.recentre {
        if (out.state.offset.x - x).abs() < 0.5 {
            view.recentre = None;
        } else {
            ui.ctx().request_repaint();
        }
    }
}

fn paint_page(ui: &Ui, rect: Rect, look: Lookup, t: &Tokens) {
    let painter = ui.painter();
    let shadow = egui::Shadow {
        offset: [0, 1],
        blur: 6,
        spread: 0,
        color: t.shadow.gamma_multiply(0.6),
    };
    painter.add(shadow.as_shape(rect, CornerRadius::ZERO));
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    match look {
        Lookup::Sharp(id) | Lookup::Stale(id) => {
            painter.image(id, rect, uv, Color32::WHITE);
        }
        // Rasters are white under the ink, so the wait looks the same.
        Lookup::Missing => {
            painter.rect_filled(rect, 0.0, Color32::WHITE);
        }
        Lookup::Failed => {
            painter.rect_filled(rect, 0.0, Color32::WHITE);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "This page couldn't be drawn",
                egui::FontId::proportional(13.0),
                Color32::from_gray(110),
            );
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
    const PLATE: PageSize = PageSize {
        width: 792.0,
        height: 612.0,
    };

    #[test]
    fn fit_width_fills_the_pane_less_its_margins() {
        let c = Column::fit_width(&[LETTER; 3], 0, 644.0, 16.0);
        assert!((c.zoom - 1.0).abs() < 1e-6);
        let first = c.rects.first().copied().unwrap_or(Rect::NOTHING);
        assert_eq!(
            first,
            Rect::from_min_size(pos2(16.0, 16.0), vec2(612.0, 792.0))
        );
        assert_eq!(c.size, vec2(644.0, 16.0 + 3.0 * 792.0 + 2.0 * GAP + MARGIN));
    }

    #[test]
    fn wider_pages_keep_the_zoom_and_widen_the_content() {
        let c = Column::fit_width(&[LETTER, PLATE], 0, 644.0, 16.0);
        let plate = c.rects.get(1).copied().unwrap_or(Rect::NOTHING);
        assert_eq!(plate.width(), 792.0);
        assert_eq!(c.size.x, 792.0 + 2.0 * MARGIN);
        // The narrower page is centred in the wider column.
        let letter = c.rects.first().copied().unwrap_or(Rect::NOTHING);
        assert_eq!(letter.center().x, plate.center().x);
    }

    #[test]
    fn the_fit_follows_the_page_it_was_set_on() {
        let c = Column::fit_width(&[LETTER, PLATE], 1, 824.0, 0.0);
        assert!((c.zoom - 1.0).abs() < 1e-6);
    }

    #[test]
    fn only_pages_meeting_the_viewport_are_visible() {
        let c = Column::fit_width(&[LETTER; 10_000], 0, 644.0, 16.0);
        let pitch = 792.0 + GAP;
        let at =
            |page: f32| Rect::from_min_size(pos2(0.0, 16.0 + page * pitch), vec2(644.0, 900.0));
        assert_eq!(c.visible(at(0.0)), 0..2);
        assert_eq!(c.visible(at(5_000.0)), 5_000..5_002);
        assert_eq!(c.visible(at(20_000.0)), 10_000..10_000);
        let gap_only = Rect::from_min_size(pos2(0.0, 16.0 + 792.0 + 1.0), vec2(9.0, 4.0));
        assert!(c.visible(gap_only).is_empty());
    }

    #[test]
    fn tiny_panes_clamp_the_zoom() {
        let c = Column::fit_width(&[LETTER], 0, 40.0, 0.0);
        assert_eq!(c.zoom, MIN_ZOOM);
    }
}
