//! A PDF in a pane: a scrolling column of pages, drawn as they arrive.

use egui::emath::GuiRounding as _;
use egui::scroll_area::{DragScroll, ScrollSource};
use egui::{Color32, CornerRadius, Pos2, Rect, ScrollArea, Ui, Vec2, pos2, vec2};
use mareader_session::{DocId, Document, PageKey, PageSize, Scale};

use crate::pages::{Lookup, PageCache};
use crate::theme::Tokens;
use crate::zoom::{PaneZoom, ZoomCommand};

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
    pub zoom: PaneZoom,
    layout: Option<Column>,
    /// An offset to reach, and the frames left to reach it.
    scroll_to: Option<(Vec2, u8)>,
    offset: Vec2,
    viewport: Vec2,
    /// Pages on screen last frame.
    pub visible: std::ops::Range<usize>,
    /// The page under the viewport's centre.
    pub current: usize,
}

impl PdfView {
    pub fn new(doc: DocId) -> Self {
        Self {
            doc,
            zoom: PaneZoom::default(),
            layout: None,
            scroll_to: None,
            offset: Vec2::ZERO,
            viewport: Vec2::ZERO,
            visible: 0..0,
            current: 0,
        }
    }

    pub fn command(&mut self, command: ZoomCommand, now: f64) {
        let shown = self.layout.as_ref().map(|c| c.zoom);
        self.zoom.command(command, shown, self.current, now);
    }
}

/// The offset keeping the point under the pivot there in `new`.
fn hold(old: &Column, new: &Column, offset: Vec2, pivots: (Vec2, Vec2)) -> Option<Vec2> {
    let (page, at) = old.locate(Pos2::ZERO + offset + pivots.0)?;
    let p = new.place(page, at)?;
    Some((p - pivots.1).to_vec2().max(Vec2::ZERO))
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
    /// One zoom for every page, so text stays one size; pages centre.
    pub fn new(sizes: &[PageSize], zoom: f32, width: f32, top: f32) -> Self {
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
            inputs: (width, top, zoom),
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

    /// The page at a content point, and the point in page units.
    pub fn locate(&self, p: Pos2) -> Option<(usize, Vec2)> {
        let last = self.rects.len().checked_sub(1)?;
        let page = self.rects.partition_point(|r| r.bottom() + GAP <= p.y);
        let page = page.min(last);
        let rect = self.rects.get(page)?;
        Some((page, (p - rect.min) / self.zoom))
    }

    /// Where a point `locate` found sits in this layout.
    pub fn place(&self, page: usize, at: Vec2) -> Option<Pos2> {
        Some(self.rects.get(page)?.min + at * self.zoom)
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
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let frame = ui.max_rect();
    let top = MARGIN + cx.top_inset;
    let sizes = doc.page_sizes();
    if ui.rect_contains_pointer(frame) {
        let (factor, pointer) = ctx.input(|i| (i.zoom_delta(), i.pointer.hover_pos()));
        if let (Some(p), true) = (pointer, factor != 1.0) {
            let at = p - frame.min - vec2(0.0, cx.top_inset);
            view.zoom.pinch(factor, at, now);
        }
    }
    let room = frame.size() - vec2(2.0 * MARGIN, 2.0 * MARGIN + cx.top_inset);
    let fit_page = sizes.get(view.zoom.fit_page).or(sizes.first()).copied();
    view.zoom.refit(fit_page, room, view.layout.is_some(), now);
    let (shown, gliding) = view.zoom.advance(now);
    if gliding {
        ctx.request_repaint();
    }
    let viewport = frame.size() - vec2(0.0, cx.top_inset);
    let inputs = (frame.width(), top, shown);
    if view.layout.as_ref().is_none_or(|c| c.inputs != inputs) {
        let new = Column::new(sizes, shown, frame.width(), top);
        match view.layout.take() {
            // Zooms and resizes keep the reading spot in place.
            Some(old) => {
                let pivot = view.zoom.pivot;
                let pivots = (
                    pivot.unwrap_or(view.viewport / 2.0),
                    pivot.unwrap_or(viewport / 2.0),
                );
                if let Some(to) = hold(&old, &new, view.offset, pivots) {
                    view.scroll_to = Some((to, 2));
                }
            }
            // The first layout centres any overflow from wider pages.
            None => {
                let x = (new.size.x - frame.width()).max(0.0) / 2.0;
                view.scroll_to = Some((vec2(x, 0.0), 2));
            }
        }
        view.layout = Some(new);
    }
    if let Some(wait) = view.zoom.sharpen(now) {
        ctx.request_repaint_after_secs(wait);
    }
    let Some(column) = &view.layout else {
        return;
    };
    let ppp = ctx.pixels_per_point();
    let wanted = Scale::covering(view.zoom.sharp() * ppp);
    let max_side = ctx.input(|i| i.max_texture_side) as f32;
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
    let t = Tokens::get(&ctx);
    let mut area = ScrollArea::both().id_salt(("pdf", view.doc));
    if let Some((to, _)) = view.scroll_to {
        area = area.scroll_offset(to);
    }
    let mut shown_pages = 0..0;
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
            shown_pages = shown.clone();
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
    let centre = Pos2::ZERO + out.state.offset + viewport / 2.0;
    view.current = column.locate(centre).map_or(0, |(page, _)| page);
    view.visible = shown_pages;
    view.offset = out.state.offset;
    view.viewport = viewport;
    view.zoom.end_frame(gliding, now);
    // A new area clamps offsets to last frame's size, so retry once.
    if let Some((to, tries)) = view.scroll_to {
        let reached = (out.state.offset - to).length() < 0.5;
        view.scroll_to = (!reached && tries > 1).then_some((to, tries - 1));
        if view.scroll_to.is_some() {
            ctx.request_repaint();
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

    fn first(c: &Column) -> Rect {
        c.rects.first().copied().unwrap_or(Rect::NOTHING)
    }

    #[test]
    fn a_column_lays_pages_at_its_zoom() {
        let c = Column::new(&[LETTER; 3], 1.0, 644.0, 16.0);
        assert_eq!(
            first(&c),
            Rect::from_min_size(pos2(16.0, 16.0), vec2(612.0, 792.0))
        );
        assert_eq!(c.size, vec2(644.0, 16.0 + 3.0 * 792.0 + 2.0 * GAP + MARGIN));
    }

    #[test]
    fn wider_pages_keep_the_zoom_and_widen_the_content() {
        let c = Column::new(&[LETTER, PLATE], 1.0, 644.0, 16.0);
        let plate = c.rects.get(1).copied().unwrap_or(Rect::NOTHING);
        assert_eq!(plate.width(), 792.0);
        assert_eq!(c.size.x, 792.0 + 2.0 * MARGIN);
        // The narrower page is centred in the wider column.
        assert_eq!(first(&c).center().x, plate.center().x);
    }

    #[test]
    fn only_pages_meeting_the_viewport_are_visible() {
        let c = Column::new(&[LETTER; 10_000], 1.0, 644.0, 16.0);
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
    fn a_point_on_a_page_survives_a_relayout() {
        let old = Column::new(&[LETTER; 20], 1.0, 644.0, 16.0);
        let new = Column::new(&[LETTER, PLATE].repeat(10), 2.5, 900.0, 64.0);
        let spot = pos2(300.0, 16.0 + 7.0 * (792.0 + GAP) + 400.0);
        let (page, at) = old.locate(spot).unwrap_or_default();
        assert_eq!(page, 7);
        let moved = new.place(page, at).unwrap_or(Pos2::ZERO);
        let back = new.locate(moved).unwrap_or_default();
        assert_eq!(back.0, 7);
        assert!((back.1 - at).length() < 1e-3);
    }

    #[test]
    fn zooming_holds_the_spot_under_the_pivot() {
        let old = Column::new(&[LETTER; 20], 1.0, 644.0, 16.0);
        let new = Column::new(&[LETTER; 20], 2.0, 644.0, 16.0);
        let (offset, viewport) = (vec2(0.0, 5_000.0), vec2(644.0, 800.0));
        let centre = viewport / 2.0;
        let before = old.locate(Pos2::ZERO + offset + centre).unwrap_or_default();
        let to = hold(&old, &new, offset, (centre, centre)).unwrap_or_default();
        let after = new.locate(Pos2::ZERO + to + centre).unwrap_or_default();
        assert_eq!(after.0, before.0);
        assert!(
            (after.1 - before.1).length() < 1e-3,
            "{after:?} vs {before:?}"
        );
    }
}
