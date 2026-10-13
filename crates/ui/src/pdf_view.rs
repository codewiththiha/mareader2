//! A PDF in a pane, in any view mode, drawn as pages arrive.

use egui::emath::GuiRounding as _;
use egui::scroll_area::{DragScroll, ScrollSource};
use egui::{Color32, CornerRadius, Pos2, Rect, ScrollArea, Ui, Vec2, pos2, vec2};
use mareader_session::{DocId, Document, PageKey, Scale};

use crate::gpu::{PagePass, PagesCallback, Quad};
use crate::layout::{self, Arrow, Inputs, Layout, MARGIN, ViewMode};
use crate::pages::{Lookup, PageCache, PageTexture};
use crate::theme::Tokens;
use crate::zoom::{Fit, PaneZoom, ZoomCommand};

/// Pages drawn ahead of the viewport, each way, in turns.
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
    pub mode: ViewMode,
    /// The paged modes' page; the strips follow the scroll instead.
    focus: usize,
    layout: Option<Layout>,
    /// A page to bring to the start of the view next frame.
    reveal: Option<usize>,
    /// An offset to reach, and the frames left to reach it.
    scroll_to: Option<(Vec2, u8)>,
    offset: Vec2,
    viewport: Vec2,
    /// Pages on screen last frame.
    pub visible: std::ops::Range<usize>,
    /// The page read: the paged modes' page, or under a strip's centre.
    pub current: usize,
}

impl PdfView {
    pub fn new(doc: DocId) -> Self {
        Self {
            doc,
            zoom: PaneZoom::default(),
            mode: ViewMode::default(),
            focus: 0,
            layout: None,
            reveal: None,
            scroll_to: None,
            offset: Vec2::ZERO,
            viewport: Vec2::ZERO,
            visible: 0..0,
            current: 0,
        }
    }

    pub fn command(&mut self, command: ZoomCommand, now: f64) {
        let shown = self.layout.as_ref().map(Layout::zoom);
        self.zoom.command(command, shown, self.current, now);
    }

    /// Switches mode on the page being read; `fit` fits the paged modes.
    pub fn set_mode(&mut self, mode: ViewMode, fit: bool, now: f64) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        // As in mareader: a spread always fits its width, the strip never.
        if mode == ViewMode::Spread || (fit && mode.paged()) {
            self.zoom
                .command(ZoomCommand::Fit(Fit::Width), None, self.current, now);
        } else if mode == ViewMode::Horizontal {
            self.zoom.unfit();
        }
        self.go(self.current);
    }

    /// An arrow key: a turn, unless it runs along a strip.
    pub fn arrow(&mut self, arrow: Arrow, count: usize) {
        if let Some(forward) = arrow.turns(self.mode) {
            self.go(self.mode.turn(self.current, forward, count));
        }
    }

    /// Shows `page` from its start.
    fn go(&mut self, page: usize) {
        self.focus = self.mode.start(page);
        self.current = page;
        self.reveal = Some(if self.mode.paged() { self.focus } else { page });
    }
}

/// The offset keeping the point under the pivot there in `new`.
fn hold(old: &Layout, new: &Layout, offset: Vec2, pivots: (Vec2, Vec2)) -> Option<Vec2> {
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

/// What drawing a pane reads and fills, shared across panes.
pub struct Sinks<'a> {
    pub pages: &'a mut PageCache,
    pub demand: &'a mut Demand,
    /// GPU pages drawn this frame, for the glass to blur.
    pub drawn: &'a mut Vec<Quad>,
    pub pass: Option<&'a std::sync::Arc<PagePass>>,
}

pub fn show(ui: &mut Ui, view: &mut PdfView, doc: &Document, io: Sinks<'_>, cx: &ViewContext) {
    let Sinks {
        pages,
        demand,
        drawn,
        pass,
    } = io;
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let frame = ui.max_rect();
    let sizes = doc.page_sizes();
    if ui.rect_contains_pointer(frame) {
        let (factor, pointer) = ctx.input(|i| (i.zoom_delta(), i.pointer.hover_pos()));
        if let (Some(p), true) = (pointer, factor != 1.0) {
            let at = p - frame.min - vec2(0.0, cx.top_inset);
            view.zoom.pinch(factor, at, now);
        }
    }
    let mut room = frame.size() - vec2(2.0 * MARGIN, 2.0 * MARGIN + cx.top_inset);
    // The strip fits a page's height alone: it scrolls sideways.
    if view.mode == ViewMode::Horizontal && view.zoom.fit == Some(Fit::Page) {
        room.x = f32::INFINITY;
    }
    let fit_box = layout::fit_box(view.mode, sizes, view.zoom.fit_page);
    view.zoom.refit(fit_box, room, view.layout.is_some(), now);
    let (shown, gliding) = view.zoom.advance(now);
    if gliding {
        ctx.request_repaint();
    }
    let viewport = frame.size() - vec2(0.0, cx.top_inset);
    let inputs = Inputs {
        mode: view.mode,
        frame: frame.size(),
        inset: cx.top_inset,
        zoom: shown,
        focus: if view.mode.paged() { view.focus } else { 0 },
    };
    if view.layout.as_ref().is_none_or(|l| l.inputs != inputs) {
        let new = Layout::new(sizes, inputs);
        match view.layout.take() {
            // Zooms and resizes keep the reading spot in place.
            Some(old) if view.reveal.is_none() && old.inputs.mode == view.mode => {
                let pivot = view.zoom.pivot;
                let pivots = (
                    pivot.unwrap_or(view.viewport / 2.0),
                    pivot.unwrap_or(viewport / 2.0),
                );
                if let Some(to) = hold(&old, &new, view.offset, pivots) {
                    view.scroll_to = Some((to, 2));
                }
            }
            _ => {
                view.reveal.get_or_insert(view.current);
            }
        }
        view.layout = Some(new);
    }
    if let Some(page) = view.reveal.take()
        && let Some(to) = view.layout.as_ref().and_then(|l| l.reveal(page))
    {
        view.scroll_to = Some((to, 2));
    }
    if let Some(wait) = view.zoom.sharpen(now) {
        ctx.request_repaint_after_secs(wait);
    }
    let Some(layout) = &view.layout else {
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
    // egui turns a plain wheel sideways only if that's the sole direction.
    let down = view.mode != ViewMode::Horizontal || layout.size.y > frame.height() + 0.5;
    ui.style_mut().always_scroll_the_only_direction = true;
    let mut area = ScrollArea::new([true, down]).id_salt(("pdf", view.doc, view.mode));
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
            ui.set_min_size(layout.size);
            let origin = ui.max_rect().min.to_vec2();
            let shown = layout.visible(viewport);
            shown_pages = shown.clone();
            let mut quads = Vec::new();
            for page in shown.clone() {
                let Some(rect) = layout.rect(page) else {
                    continue;
                };
                let rect = rect.translate(origin).round_to_pixels(ppp);
                let look = pages.lookup(key(page));
                if !matches!(look, Lookup::Sharp(_) | Lookup::Failed) {
                    demand.push((urgency, key(page)));
                }
                paint_page(ui, rect, look, &t, &mut quads);
            }
            // One callback draws every GPU page, over the shadows.
            if let (Some(pass), false) = (pass, quads.is_empty()) {
                let shape = PagesCallback::shape(pass.clone(), ui.clip_rect(), quads.clone());
                ui.painter().add(shape);
                drawn.extend(quads);
            }
            // The strips' neighbours, or the next and last turn's pages.
            let (near, count) = match view.mode {
                ViewMode::Single | ViewMode::Spread => (layout.pages(), sizes.len()),
                ViewMode::Vertical | ViewMode::Horizontal => (shown.clone(), sizes.len()),
            };
            let reach = PREFETCH * if view.mode == ViewMode::Spread { 2 } else { 1 };
            let before = near.start.saturating_sub(reach)..near.start;
            let after = near.end..(near.end + reach).min(count);
            for page in before.chain(after) {
                if !pages.touch(key(page)) {
                    demand.push((Urgency::Prefetch, key(page)));
                }
            }
        });
    if !view.mode.paged() && view.scroll_to.is_none() {
        let centre = Pos2::ZERO + out.state.offset + viewport / 2.0;
        view.current = layout.locate(centre).map_or(0, |(page, _)| page);
    }
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

/// Paints a page, or queues it for the page pass in `quads`.
fn paint_page(ui: &Ui, rect: Rect, look: Lookup, t: &Tokens, quads: &mut Vec<Quad>) {
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
        Lookup::Sharp(PageTexture::Gpu(page)) | Lookup::Stale(PageTexture::Gpu(page)) => {
            quads.push(Quad {
                rect,
                clip: ui.clip_rect(),
                tint: Color32::WHITE,
                texture: Some(page),
            });
        }
        Lookup::Sharp(PageTexture::Egui(h)) | Lookup::Stale(PageTexture::Egui(h)) => {
            painter.image(h.id(), rect, uv, Color32::WHITE);
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
    use mareader_session::PageSize;

    const LETTER: PageSize = PageSize {
        width: 612.0,
        height: 792.0,
    };

    fn column(zoom: f32) -> Layout {
        let inputs = Inputs {
            mode: ViewMode::Vertical,
            frame: vec2(644.0, 800.0),
            inset: 0.0,
            zoom,
            focus: 0,
        };
        Layout::new(&[LETTER; 20], inputs)
    }

    #[test]
    fn zooming_holds_the_spot_under_the_pivot() {
        let (old, new) = (column(1.0), column(2.0));
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

    #[test]
    fn a_mode_switch_keeps_the_page_and_a_turn_moves_it() -> Result<(), &'static str> {
        let mut view = PdfView::new(crate::testing::doc()?);
        view.current = 7;
        view.set_mode(ViewMode::Spread, false, 0.0);
        assert_eq!((view.focus, view.current, view.reveal), (6, 7, Some(6)));
        assert_eq!(view.zoom.fit, Some(Fit::Width));
        view.arrow(Arrow::Right, 20);
        assert_eq!((view.focus, view.current), (8, 8));
        view.set_mode(ViewMode::Horizontal, true, 0.0);
        assert_eq!((view.zoom.fit, view.reveal), (None, Some(8)));
        // Along the strip, the arrow is the scroll's, not a turn.
        view.arrow(Arrow::Right, 20);
        assert_eq!(view.current, 8);
        view.arrow(Arrow::Up, 20);
        assert_eq!(view.current, 8);
        // Across the column, left and right still turn.
        view.set_mode(ViewMode::Vertical, true, 0.0);
        view.arrow(Arrow::Left, 20);
        assert_eq!((view.current, view.reveal), (7, Some(7)));
        Ok(())
    }
}
