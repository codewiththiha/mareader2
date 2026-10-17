//! A PDF in a pane, in any view mode, drawn as pages arrive.

use egui::emath::GuiRounding as _;
use egui::scroll_area::{DragScroll, ScrollAreaOutput as ScrollOutput, ScrollSource};
use std::sync::Arc;

use egui::style::ScrollAnimation;
use egui::{Color32, CornerRadius, Pos2, Rect, ScrollArea, Ui, Vec2, pos2, vec2};
use mareader_library::{BookId, MarkId};
use mareader_pdf::{Bounds, LinkTarget, web_address};
use mareader_search::reveal;
use mareader_session::{Band, DocId, Document, PageKey, PageSize, Part, Scale, Tiling, Want};
use mareader_settings::{ColumnAlign, HighlightSettings, LayoutSettings, StartupFit, TextSettings};

use crate::gpu::{Anchor, PagePass, PagesCallback, Quad};
use crate::layout::{self, Arrow, GAP, Inputs, Layout, MARGIN, ViewMode};
use crate::pages::{Lookup, PageCache, PageTexture};
use crate::reach::{self, Press, Stop};
use crate::selection::{self, Pages, Selection, Sheet};
use crate::study::{self, BarPress, Gloss};
use crate::texts::TextLayers;
use crate::theme::Tokens;
use crate::vocab;
use crate::zoom::{Fit, PaneZoom, ZoomCommand};
use crate::{keymap, motion};
use mareader_appearance::{PageTone, Rgb};

/// Pages drawn ahead of the viewport, each way, in turns.
const PREFETCH: usize = 1;
/// A held arrow waits this long, then glides at this speed (mareader's).
const HOLD_DELAY: f64 = 0.35;
const HOLD_SPEED: f32 = 1000.0;
/// A slow frame moves a held glide no further than this one would.
const MAX_FRAME: f64 = 0.05;
/// Auto-scroll's pace along the strip, as mareader's 72 px a second.
pub const AUTO_SPEED: f32 = 72.0;
/// Along a strip, a page of another size refits once read this long.
const RESIZE_SETTLE: f64 = 0.25;
/// Tiles wait for the view to rest this long, so a pan queues none.
const TILE_REST: f64 = 0.25;
/// A tiled page's whole raster, stretched under its tiles.
const BACKDROP_SIDE: f32 = 2048.0;

/// What a page is drawn from: one raster, or a backdrop under tiles.
#[derive(Debug, Clone, Copy)]
enum Plan {
    Whole(PageKey),
    Tiled {
        backdrop: PageKey,
        tiling: Tiling,
        scale: Scale,
    },
}

impl Plan {
    /// The whole-page raster: the page itself, or its backdrop.
    fn base(self) -> PageKey {
        match self {
            Self::Whole(key) | Self::Tiled { backdrop: key, .. } => key,
        }
    }
}

/// A tap moves one reading line: 8 % of the view, 40 to 80 points.
pub fn line_step(len: f32) -> f32 {
    (len * 0.08).clamp(40.0, 80.0)
}

/// A page key moves nearly a screen, keeping a sliver to hold the place.
pub fn screen_step(len: f32) -> f32 {
    (len * 0.9).max(1.0)
}

/// A key's move along a strip, applied by the next frame.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Nudge {
    Line(f32),
    Screen(f32),
}

impl Nudge {
    fn points(self, len: f32) -> f32 {
        match self {
            Self::Line(dir) => dir * line_step(len),
            Self::Screen(dir) => dir * screen_step(len),
        }
    }
}

/// An arrow held along a strip, since `since`; `last` is the last frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Hold {
    arrow: Arrow,
    since: f64,
    last: f64,
}

/// A page jump gliding to `to` (a [`Glide`]); only its pages render meanwhile.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Jump {
    to: Vec2,
    started: bool,
    /// Given up by then: a wheel or a drag took the scroll over.
    until: f64,
}

/// How soon a page is needed; lower is sooner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    ActivePane,
    OtherPane,
    /// Text under the pointer matters more than a thumbnail.
    Text,
    /// Small and quick, and the backdrop waits on it.
    Paper,
    Thumbnail,
    Prefetch,
}

/// The frame's requests from every pane.
pub type Demand = Vec<(Urgency, Want)>;

/// One pane's view of a document; the document itself is shared.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfView {
    pub doc: DocId,
    /// The library book this pane reads as; none: the file's own.
    pub book: Option<BookId>,
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
    /// How far through the document, 0 to 1.
    pub progress: f32,
    pub selection: Option<Selection>,
    /// Pages on screen last frame, where they sat.
    pub sheets: Vec<Sheet>,
    /// The page looked at last frame, fractional, for the backdrop.
    paper_at: Option<f64>,
    nudge: Option<Nudge>,
    /// Where the last key's glide is heading along the strip, and until when.
    aim: Option<Glide>,
    hold: Option<Hold>,
    /// Auto-scroll is on: when it last moved the strip.
    drift: Option<f64>,
    jump: Option<Jump>,
    /// Zoom steps ease, as of last frame's motion settings.
    ease: bool,
    /// Until first drawn: the default fit is still to apply.
    fresh: bool,
    /// A page of another size Auto Resize waits on, and since when.
    resize: Option<(usize, f64)>,
    /// When the view last moved, for tiles to wait on.
    moved_at: f64,
    /// Where the last re-set landed, and on which page; reuse never drifts.
    place: Option<(mareader_session::Anchor, usize)>,
}

impl PdfView {
    pub fn new(doc: DocId) -> Self {
        Self {
            doc,
            book: None,
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
            progress: 0.0,
            selection: None,
            sheets: Vec::new(),
            paper_at: None,
            fresh: true,
            resize: None,
            moved_at: 0.0,
            nudge: None,
            aim: None,
            hold: None,
            drift: None,
            jump: None,
            ease: true,
            place: None,
        }
    }

    /// The place the last re-set landed on, while it's still read.
    pub fn place(&self) -> Option<mareader_session::Anchor> {
        self.place
            .filter(|(_, page)| *page == self.current)
            .map(|(a, _)| a)
    }

    /// Keeps `place` as the page being read's, as a re-set does.
    pub fn hold_place(&mut self, place: Option<mareader_session::Anchor>) {
        self.place = place.map(|a| (a, self.current));
    }

    /// Shows the book set again as `doc`, from `page`; mode and zoom stay.
    pub fn retypeset(
        &mut self,
        doc: DocId,
        page: usize,
        count: usize,
        place: Option<mareader_session::Anchor>,
    ) {
        let page = page.min(count.saturating_sub(1));
        self.doc = doc;
        // Page rects, selection and motion belong to the old pages.
        self.layout = None;
        self.selection = None;
        self.sheets.clear();
        self.visible = page..page + 1;
        (self.jump, self.aim, self.hold, self.nudge) = (None, None, None, None);
        self.scroll_to = None;
        self.resize = None;
        self.zoom.fit_page = page;
        self.place = place.map(|a| (a, page));
        self.go(page);
    }

    pub fn command(&mut self, command: ZoomCommand, now: f64) {
        let shown = self.layout.as_ref().map(Layout::zoom);
        let shown = shown.filter(|_| self.ease);
        self.zoom.command(command, shown, self.current, now);
    }

    /// Switches mode on the page being read; `fit` fits the paged modes.
    pub fn set_mode(&mut self, mode: ViewMode, fit: bool, now: f64) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        self.hold = None;
        // As in mareader: a spread always fits its width, the strip never.
        if mode == ViewMode::Spread || (fit && mode.paged()) {
            self.zoom
                .command(ZoomCommand::Fit(Fit::Width), None, self.current, now);
        } else if mode == ViewMode::Horizontal {
            self.zoom.unfit();
        }
        self.go(self.current);
    }

    /// Shift+A: auto-scroll on or off; false in a paged mode.
    pub fn toggle_drift(&mut self, now: f64) -> bool {
        if self.mode.paged() {
            return false;
        }
        self.drift = match self.drift {
            Some(_) => None,
            None => Some(now),
        };
        true
    }

    /// Stops auto-scroll; true if it was running.
    pub fn stop_drift(&mut self) -> bool {
        self.drift.take().is_some()
    }

    pub fn drifting(&self) -> bool {
        self.drift.is_some()
    }

    /// An arrow key: a turn, or along a strip a line and then a glide.
    pub fn arrow(&mut self, arrow: Arrow, count: usize, now: f64) {
        if let Some(forward) = arrow.turns(self.mode) {
            self.go(self.mode.turn(self.current, forward, count));
            return;
        }
        // Up and down across the strip do nothing, as in mareader.
        let sideways = matches!(arrow, Arrow::Left | Arrow::Right);
        if sideways != (self.mode == ViewMode::Horizontal) {
            return;
        }
        // The key repeating while held: the glide is already on its way.
        if self.hold.is_some_and(|h| h.arrow == arrow) {
            return;
        }
        self.hold = Some(Hold {
            arrow,
            since: now,
            last: now,
        });
        self.nudge = Some(Nudge::Line(arrow.sign()));
    }

    /// A page key: nearly a screen along a strip, a turn when paged.
    pub fn page_key(&mut self, forward: bool, count: usize) {
        if self.mode.paged() {
            self.turn(forward, count);
        } else {
            self.nudge = Some(Nudge::Screen(if forward { 1.0 } else { -1.0 }));
        }
    }

    /// Where the backdrop reads its paper: pages weighted by their share.
    pub fn paper_at(&self) -> f64 {
        self.paper_at.unwrap_or(self.current as f64)
    }

    /// Last frame's scroll, in content points; narrower pages sit centred.
    pub fn offset(&self) -> Vec2 {
        self.offset
    }

    /// A page turn from a button: it turns in every mode.
    pub fn turn(&mut self, forward: bool, count: usize) {
        self.go(self.mode.turn(self.current, forward, count));
    }

    /// Jumps to `page`, kept within the document's `count`.
    pub fn go_to(&mut self, page: usize, count: usize) {
        self.go(page.min(count.saturating_sub(1)));
    }

    /// Shows `spot` on `page`, offset by its stream `band`; false if unknown.
    pub fn seek(&mut self, page: usize, spot: Option<Bounds>, band: Vec2, count: usize) -> bool {
        // Already heading there: going again would restart the glide.
        let heading = self.jump.is_some() || self.reveal.is_some();
        let shows = self.visible.contains(&page);
        if self.mode.paged() || spot.is_none() || self.layout.is_none() {
            if !shows && !heading {
                self.go_to(page, count);
            }
            return self.mode.paged();
        }
        let (Some(layout), Some(b)) = (&self.layout, spot) else {
            return false;
        };
        let Some(rect) = layout.rect(page) else {
            return false;
        };
        let zoom = layout.zoom();
        // Only the vertical strip cuts pages to their bands.
        let band = if self.mode == ViewMode::Vertical {
            band
        } else {
            Vec2::ZERO
        };
        let origin = rect.min - band * zoom;
        let at = |x: f32, y: f32| origin + vec2(x, y) * zoom;
        let hit = Rect::from_min_max(at(b.x0, b.y0), at(b.x1, b.y1));
        let (o, v) = (self.offset, self.viewport);
        // The find bar floats over the pane's top, under any pinned bar.
        let top = layout.inputs.inset + FIND_CLEARANCE;
        let y = reveal(
            hit.top()..hit.bottom(),
            o.y + top..o.y + v.y - SEEK_MARGIN,
            o.y,
        );
        let x = reveal(
            hit.left()..hit.right(),
            o.x + SEEK_MARGIN..o.x + v.x - SEEK_MARGIN,
            o.x,
        );
        if x.is_some() || y.is_some() {
            self.scroll_to = Some((vec2(x.unwrap_or(o.x), y.unwrap_or(o.y)), 2));
            self.jump = None;
            self.aim = None;
            self.reveal = None;
        }
        true
    }

    /// Auto Resize: refits to the page read; returns any wait left.
    fn follow_size(&mut self, sizes: &[PageSize], on: bool, now: f64) -> Option<f64> {
        let page = self.current;
        let fit_box = |p| layout::fit_box(self.mode, sizes, p);
        // A refit mid-glide would move the glide's end; it waits.
        let gliding = self.jump.is_some() || self.aim.is_some();
        if !on || gliding || self.zoom.fit.is_none() || fit_box(page) == fit_box(self.zoom.fit_page)
        {
            self.resize = None;
            return None;
        }
        // A turn lands at once, so it refits at once: no wrong size shows.
        if self.mode.paged() {
            self.zoom.fit_page = page;
            self.resize = None;
            return None;
        }
        match self.resize {
            Some((waiting, since)) if waiting == page => {
                let left = RESIZE_SETTLE - (now - since);
                if left > 0.0 {
                    return Some(left);
                }
                self.zoom.fit_page = page;
                self.resize = None;
                None
            }
            _ => {
                self.resize = Some((page, now));
                Some(RESIZE_SETTLE)
            }
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
    /// The pane's look: the window's, or its own while split.
    pub tokens: Tokens,
    /// Space a pinned title bar takes from the pane's top.
    pub top_inset: f32,
    pub page: PageLook,
    /// Blend's paper behind the pages, raw; none draws no backdrop.
    pub paper: Option<Rgb>,
    /// The pane's corner radius, which the backdrop keeps.
    pub corner: f32,
    /// Read this pane's paper: blend is on and the pane is focused.
    pub sample: bool,
    /// The book's marks, and how marks and the selection bar look.
    pub marks: Vec<study::marks::Shown>,
    pub highlights: HighlightSettings,
    /// The mark whose card is open here, to keep the card beside it.
    pub card: Option<MarkId>,
    /// The marks picked here, while picking several.
    pub picked: Option<Vec<MarkId>>,
    /// How hard words are shaded; none while shading is off.
    pub shading: Option<crate::vocab::Look>,
    /// The find bar's hits, in the pane it searches.
    pub found: Option<crate::search::Lit>,
}

/// The Layout settings the pages follow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageLook {
    pub gap: f32,
    /// Air beside the pages; the sideways strip has none to give.
    pub margin: f32,
    pub shadow: bool,
    /// A fit follows the page read when its size differs.
    pub auto_resize: bool,
    /// The fit a document opens at.
    pub fit: Fit,
    /// Where a book's vertical stream sits across the pane.
    pub column: egui::Align,
    /// How much of a book's ink stays, 0 to 1: the ink dial.
    pub ink: f32,
    /// A book's pictures take the page's theme, not their own colours.
    pub theme_pictures: bool,
    /// So do its emoji, drawn from colour fonts.
    pub theme_emojis: bool,
}

impl PageLook {
    pub fn new(layout: &LayoutSettings, text: &TextSettings) -> Self {
        Self {
            gap: if layout.no_gap { 0.0 } else { GAP },
            margin: layout.page_margin,
            shadow: layout.page_shadow,
            auto_resize: layout.auto_resize,
            fit: match layout.startup_fit {
                StartupFit::Page => Fit::Page,
                StartupFit::Width => Fit::Width,
            },
            column: match text.column_align {
                ColumnAlign::Left => egui::Align::Min,
                ColumnAlign::Center => egui::Align::Center,
                ColumnAlign::Right => egui::Align::Max,
            },
            ink: (text.ink_contrast / 100.0).clamp(0.0, 1.0),
            theme_pictures: text.theme_pictures,
            theme_emojis: text.theme_emojis,
        }
    }
}

impl Default for PageLook {
    fn default() -> Self {
        Self::new(&LayoutSettings::default(), &TextSettings::default())
    }
}

/// What drawing a pane reads and fills, shared across panes.
pub struct Sinks<'a> {
    pub pages: &'a mut PageCache,
    pub demand: &'a mut Demand,
    pub texts: &'a mut TextLayers,
    /// GPU pages drawn this frame, for the glass to blur.
    pub drawn: &'a mut Vec<Quad>,
    pub pass: Option<&'a std::sync::Arc<PagePass>>,
    /// What the pages tell the study tools.
    pub gloss: &'a mut Vec<Gloss>,
    /// Which words are hard, for the pages that shade them.
    pub vocab: Option<&'a mut crate::vocab::Vocab>,
}

pub fn show(ui: &mut Ui, view: &mut PdfView, doc: &Document, io: Sinks<'_>, cx: &ViewContext) {
    let Sinks {
        pages,
        demand,
        texts,
        drawn,
        pass,
        gloss,
        mut vocab,
    } = io;
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    view.ease = motion::zoom(&ctx);
    let frame = ui.max_rect();
    let sizes = doc.page_sizes();
    // A book's vertical strip is a stream: each page cut to its text.
    let bands = doc.bands().filter(|_| view.mode == ViewMode::Vertical);
    if ui.rect_contains_pointer(frame) {
        let (factor, pointer) = ctx.input(|i| (i.zoom_delta(), i.pointer.hover_pos()));
        if let (Some(p), true) = (pointer, factor != 1.0) {
            let at = p - frame.min - vec2(0.0, cx.top_inset);
            view.zoom.pinch(factor, at, now);
        }
    }
    if std::mem::take(&mut view.fresh) && view.zoom.fit.is_some() {
        view.zoom.fit = Some(cx.page.fit);
    }
    if let Some(wait) = view.follow_size(sizes, cx.page.auto_resize, now) {
        ctx.request_repaint_after_secs(wait as f32);
    }
    let margin = if view.mode == ViewMode::Horizontal {
        MARGIN
    } else if bands.is_some() {
        // A stream's column never touches the pane's edge.
        cx.page.margin.max(MARGIN)
    } else {
        cx.page.margin
    };
    let mut room = frame.size() - vec2(2.0 * margin, 2.0 * MARGIN + cx.top_inset);
    // The strip fits a page's height alone: it scrolls sideways.
    if view.mode == ViewMode::Horizontal && view.zoom.fit == Some(Fit::Page) {
        room.x = f32::INFINITY;
    }
    let fit_box = layout::fit_box(view.mode, sizes, view.zoom.fit_page);
    let follow = motion::follow(&ctx);
    if let Some(wait) = view
        .zoom
        .refit(fit_box, room, view.layout.is_some(), follow, now)
    {
        ctx.request_repaint_after_secs(wait);
    }
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
        gap: if bands.is_some() { 0.0 } else { cx.page.gap },
        margin,
        align: match bands {
            Some(_) => cx.page.column,
            None => egui::Align::Center,
        },
    };
    let rebuilt = view.layout.as_ref().is_none_or(|l| l.inputs != inputs);
    if rebuilt {
        let new = match bands {
            Some(bands) => {
                let cut = |b: &Band| PageSize {
                    width: b.width,
                    height: b.height,
                };
                Layout::new(&bands.iter().map(cut).collect::<Vec<_>>(), inputs)
            }
            None => Layout::new(sizes, inputs),
        };
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
        // A strip glides to the page; a new layout or a turn lands there.
        let glide = motion::jump(&ctx);
        if !rebuilt && !view.mode.paged() && glide.duration.max > 0.0 {
            let until = now + f64::from(glide.duration.max) + 0.2;
            let end = view.layout.as_ref().map_or(Vec2::ZERO, |l| l.size) - viewport;
            let to = to.min(end.max(Vec2::ZERO));
            // Along the strip it glides with the keys' glide; across, it lands.
            let pick = |v: Vec2| {
                if view.mode == ViewMode::Horizontal {
                    v.x
                } else {
                    v.y
                }
            };
            view.aim = Some(Glide::new(pick(view.offset), pick(to), now, glide));
            view.jump = Some(Jump {
                to,
                started: false,
                until,
            });
        } else {
            view.scroll_to = Some((to, 2));
            view.jump = None;
        }
    }
    if let Some(wait) = view.zoom.sharpen(now) {
        ctx.request_repaint_after_secs(wait);
    }
    // Keys move along the strip: sideways in the horizontal one.
    let sideways = view.mode == ViewMode::Horizontal;
    let along = |d: f32| if sideways { vec2(d, 0.0) } else { vec2(0.0, d) };
    let len = if sideways { viewport.x } else { viewport.y };
    // Auto-scroll belongs to the focused pane, as in mareader.
    if !cx.active {
        view.drift = None;
    }
    let keyed = keyed_scroll(view, &ctx, len, now);
    let Some(layout) = &view.layout else {
        return;
    };
    let ppp = ctx.pixels_per_point();
    let wanted = Scale::covering(view.zoom.sharp() * ppp);
    let max_side = ctx.input(|i| i.max_texture_side) as f32;
    // Past the GPU's and hayro's limits, a page is drawn in tiles.
    let plan = |page: usize| {
        let size = sizes.get(page).copied().unwrap_or(PageSize {
            width: 1.0,
            height: 1.0,
        });
        let long = size.width.max(size.height);
        let cap = Scale::within(doc.fitted_scale(page, max_side / long).unwrap_or(1.0));
        let whole = |scale| PageKey {
            doc: view.doc,
            page,
            scale,
            part: Part::Whole,
        };
        if wanted <= cap {
            return Plan::Whole(whole(wanted));
        }
        let backdrop = Scale::within((BACKDROP_SIDE / long).min(cap.value()));
        let tiling = Tiling::new(size, wanted.value());
        Plan::Tiled {
            backdrop: whole(backdrop),
            tiling,
            scale: wanted,
        }
    };
    let key = |page: usize| plan(page).base();
    let resting = now - view.moved_at >= TILE_REST;
    let urgency = if cx.active {
        Urgency::ActivePane
    } else {
        Urgency::OtherPane
    };
    let mut t = cx.tokens;
    let reflow = doc.is_reflowable();
    // A book's pictures and emoji keep their colours unless themed.
    let (mut picture_tone, mut emoji_tone) = (PageTone::NONE, PageTone::NONE);
    if reflow {
        let mut text = t.book.inked(cx.page.ink);
        // Blend's paper, as the PDF beside it shows it.
        if let Some(raw) = cx.paper {
            text.paper = t.page.apply(raw.to_unit());
        }
        let themed = t.page.for_pictures(&text);
        let pick = |on: bool| if on { themed } else { PageTone::NONE };
        (picture_tone, emoji_tone) = (pick(cx.page.theme_pictures), pick(cx.page.theme_emojis));
        t.page = text;
    }
    // egui turns a plain wheel sideways only if that's the sole direction.
    let down = view.mode != ViewMode::Horizontal || layout.size.y > frame.height() + 0.5;
    ui.style_mut().always_scroll_the_only_direction = true;
    let mut area = ScrollArea::new([true, down]).id_salt(("pdf", view.doc, view.mode));
    if let Some((to, _)) = view.scroll_to {
        area = area.scroll_offset(to);
    }
    let jump = view.jump;
    let mut jump_started = false;
    // While a jump glides, only the pages it lands on are worth drawing.
    let landing = jump.map(|j| layout.visible(Rect::from_min_size(Pos2::ZERO + j.to, viewport)));
    let wanted = |page: usize| landing.as_ref().is_none_or(|r| r.contains(&page));
    let mut shown_pages = 0..0;
    let mut sheets = Vec::new();
    let mut paper_at = None;
    let mut follow = None;
    let doc_id = view.doc;
    let selection = &mut view.selection;
    let out = area
        .auto_shrink(false)
        // The view runs its own glides, so each delta lands this frame.
        .animated(false)
        .content_margin(0.0)
        // Dragging will select text, not scroll.
        .scroll_source(ScrollSource {
            drag: DragScroll::Never,
            ..Default::default()
        })
        .show_viewport(ui, |ui, viewport| {
            ui.set_min_size(layout.size);
            if let Some(points) = keyed {
                ui.scroll_with_delta_animation(-along(points), ScrollAnimation::none());
            }
            if let Some(j) = jump.filter(|j| !j.started) {
                let d = viewport.min.to_vec2() - j.to;
                let across = if sideways {
                    vec2(0.0, d.y)
                } else {
                    vec2(d.x, 0.0)
                };
                ui.scroll_with_delta_animation(across, ScrollAnimation::none());
                jump_started = true;
            }
            let origin = ui.max_rect().min.to_vec2();
            // One sheet under every page, so patterns run across seams.
            let anchor = Anchor {
                origin: ui.max_rect().min,
                zoom: layout.zoom(),
            };
            // Blend's paper under the shadows; a stream always has its own.
            let white = Rgb(255, 255, 255);
            let paper = match reflow {
                // A book's tone holds its paper already.
                true => (cx.paper.is_some() || bands.is_some()).then_some(white),
                false => cx.paper.or(bands.map(|_| white)),
            };
            if let (Some(paper), Some(pass)) = (paper, pass) {
                let tone = t.page.backdrop(paper.to_unit());
                let quad = Quad::backdrop(frame, cx.corner, tone, anchor);
                let shape = PagesCallback::shape(pass.clone(), frame, vec![quad.clone()]);
                ui.painter().add(shape);
                drawn.push(quad);
            }
            let shown = layout.visible(viewport);
            shown_pages = shown.clone();
            let mut quads = Vec::new();
            for page in shown.clone() {
                let Some(rect) = layout.rect(page) else {
                    continue;
                };
                let rect = rect.translate(origin).round_to_pixels(ppp);
                let (rect, clip) = match bands.and_then(|b| b.get(page)) {
                    Some(band) => streamed(rect, band, sizes.get(page), layout.zoom(), ppp),
                    None => (rect, ui.clip_rect()),
                };
                let clip = clip.intersect(ui.clip_rect());
                let plan = plan(page);
                let look = pages.lookup(plan.base());
                if wanted(page) && !matches!(look, Lookup::Sharp(_) | Lookup::Failed) {
                    demand.push((urgency, plan.base().into()));
                }
                if wanted(page) && !texts.touch(doc_id, page) {
                    demand.push((Urgency::Text, Want::Text { doc: doc_id, page }));
                }
                let round = (frame, cx.corner);
                let style = (&t, cx.page.shadow && bands.is_none());
                let first = quads.len();
                paint_page(ui, ((rect, clip), anchor, round), look, style, &mut quads);
                if let Plan::Tiled {
                    backdrop,
                    tiling,
                    scale,
                } = plan
                {
                    let key = PageKey { scale, ..backdrop };
                    let tiles = Tiles {
                        page: rect,
                        clip,
                        anchor,
                        tiling,
                        key,
                    };
                    let ask = resting && wanted(page);
                    tiles.paint(
                        ui,
                        pages,
                        ask.then_some((&mut *demand, urgency)),
                        (&mut quads, t.page),
                    );
                }
                let width = sizes.get(page).map_or(1.0, |s| s.width);
                if reflow {
                    let to_screen = |b: Bounds| {
                        let k = rect.width() / width;
                        Rect::from_min_max(
                            rect.min + vec2(b.x0, b.y0) * k,
                            rect.min + vec2(b.x1, b.y1) * k,
                        )
                    };
                    let shown = doc.pictures(page).into_iter().map(to_screen);
                    keep_pictures(&mut quads, first, shown, picture_tone);
                    let emojis = doc.emojis(page).into_iter().map(to_screen);
                    keep_pictures(&mut quads, first, emojis, emoji_tone);
                }
                sheets.push(Sheet {
                    page,
                    rect,
                    shown: rect.intersect(clip),
                    scale: rect.width() / width,
                });
            }
            paper_at = mareader_pdf::position(sheets.iter().map(|s| {
                let seen = s.shown;
                (s.page, if seen.is_positive() { seen.area() } else { 0.0 })
            }));
            // One callback draws every GPU page, over the shadows.
            for quad in &mut quads {
                quad.round_to(frame, cx.corner);
            }
            if let (Some(pass), false) = (pass, quads.is_empty()) {
                let shape = PagesCallback::shape(pass.clone(), ui.clip_rect(), quads.clone());
                ui.painter().add(shape);
                drawn.extend(quads);
            }
            let text = Pages {
                doc: doc_id,
                sheets: &sheets,
                texts,
                sideways: !down,
            };
            let placed = study::marks::place(&cx.marks, &text);
            // Shading sits under the reader's own marks.
            let shading = cx.shading.as_ref().zip(vocab.as_mut());
            let shaded = shading.map_or_else(Vec::new, |(look, v)| v.place(look, &text));
            let on_word = vocab::hovered(ui, &shaded).filter(|_| cx.picked.is_none());
            if let Some(look) = &cx.shading {
                let under_mark = |i: usize| {
                    let w = shaded.get(i);
                    w.is_some_and(|w| placed.iter().any(|m| m.contains(w.bounds().center())))
                };
                let lit = on_word.filter(|i| !under_mark(*i));
                vocab::paint(ui, &shaded, look, lit);
                let id = egui::Id::new(("shade-rest", doc_id));
                if let Some(i) = vocab::dwell(ui, id, &shaded, lit, look)
                    && let Some(w) = shaded.get(i)
                    && let Some(layer) = texts.get(doc_id, w.page)
                {
                    let word = study::words::text_of(&layer, w.chars.clone());
                    gloss.push(Gloss::Rest {
                        word,
                        at: w.bounds(),
                    });
                }
            }
            study::marks::paint(ui, &placed, cx.highlights.opacity);
            if let Some(picked) = &cx.picked {
                study::marks::ring(ui.painter(), &placed, picked, t.accent);
            }
            if let Some(lit) = &cx.found {
                crate::search::paint(ui, &text, lit, &t);
            }
            if let Some(sel) = selection.filter(|s| !s.is_empty()) {
                selection::paint(ui, &text, sel, t.accent.gamma_multiply(0.32));
            }
            let did = selection::interact(ui, ui.clip_rect(), &text, selection);
            follow = did.follow;
            let on_mark = |p: Option<Pos2>| p.and_then(|p| study::marks::hit(&placed, p));
            // A long press picks; the click its release makes does nothing.
            if study::marks::swallowed(ui, did.click.is_some()) {
                follow = None;
            } else if let Some(m) = on_mark(did.click) {
                follow = None;
                gloss.push(match cx.picked {
                    Some(_) => Gloss::Toggle(m.id),
                    None => Gloss::Open {
                        mark: m.id,
                        at: m.bounds(),
                    },
                });
            } else if let (Some(p), Some(look), None) = (did.click, &cx.shading, &cx.picked)
                && look.click
                && let Some(w) = vocab::hit(&shaded, p).and_then(|i| shaded.get(i))
            {
                follow = None;
                gloss.push(Gloss::Shaded {
                    page: w.page,
                    chars: w.chars.clone(),
                    at: w.bounds(),
                });
            }
            if let Some(id) = study::marks::long_press(ui, &placed) {
                gloss.push(Gloss::Pick(id));
            }
            if let (Some(m), Some(at), None) = (on_mark(did.context), did.context, &cx.picked) {
                gloss.push(Gloss::Menu { mark: m.id, at });
            }
            let on = reach::On {
                pages: &text,
                placed: &placed,
                shaded: &shaded,
                words: cx.shading.as_ref().is_some_and(|l| l.click),
                accent: t.accent,
            };
            let press = reach::show(ui, &on);
            let picking = cx.picked.is_some();
            if let Some(target) = at_stop(press, &placed, &shaded, picking, gloss) {
                follow = Some(target);
            }
            if cx.picked.is_some() {
                // A mark across two pages is placed on each.
                let mut page: Vec<MarkId> = Vec::new();
                for m in &placed {
                    if !page.contains(&m.id) {
                        page.push(m.id);
                    }
                }
                let all = cx.marks.iter().map(|m| m.id).collect();
                gloss.push(Gloss::Seen { page, all });
            }
            if let Some(m) = placed.iter().find(|m| Some(m.id) == cx.card) {
                gloss.push(Gloss::Anchor(Some(m.bounds())));
            } else if cx.card.is_some() {
                // A page whose text hasn't come may hold the mark.
                let shown = sheets.iter().filter(|s| s.shown.is_positive());
                if shown
                    .into_iter()
                    .any(|s| texts.get(doc_id, s.page).is_none())
                {
                    gloss.push(Gloss::Anchor(None));
                }
            }
            // A jump's landing renders while the glide is on its way.
            if let Some(landing) = &landing {
                for page in landing.clone().filter(|p| !shown.contains(p)) {
                    if !matches!(pages.lookup(key(page)), Lookup::Sharp(_) | Lookup::Failed) {
                        demand.push((urgency, key(page).into()));
                    }
                }
                return;
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
                    demand.push((Urgency::Prefetch, key(page).into()));
                }
            }
        });
    if let Some(j) = &mut view.jump {
        j.started |= jump_started;
        // Its glide ended, or a wheel or a key took over.
        let arrived = (out.state.offset - j.to).length() < 1.0;
        if arrived || view.aim.is_none() || now > j.until {
            view.jump = None;
        } else {
            ctx.request_repaint();
        }
    }
    // A jump keeps its page as the one read until it lands.
    if !view.mode.paged() && view.scroll_to.is_none() && view.jump.is_none() {
        let centre = Pos2::ZERO + out.state.offset + viewport / 2.0;
        view.current = layout.locate(centre).map_or(0, |(page, _)| page);
    }
    view.progress = progress(view, layout, &out, sizes.len());
    view.visible = shown_pages;
    view.sheets = sheets;
    // Nothing shown, as mid-jump: the backdrop holds its colour.
    view.paper_at = paper_at.or(view.paper_at);
    if view.offset != out.state.offset || view.viewport != viewport {
        view.moved_at = now;
    }
    if now - view.moved_at < TILE_REST {
        ctx.request_repaint_after_secs((TILE_REST - (now - view.moved_at)) as f32 + 0.01);
    }
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
    if cx.active {
        copy(&ctx, view, doc, texts);
        let room = Rect::from_min_max(frame.min + vec2(0.0, cx.top_inset), frame.max);
        let pressed = selection_bar(&ctx, view, texts, room, &cx.highlights);
        match pressed {
            Some(BarPress::Copy) => {
                if let Some(text) = selected_text(view, doc, texts) {
                    ctx.copy_text(text);
                    gloss.push(Gloss::Copied);
                }
            }
            Some(BarPress::Explain) => gloss.push(Gloss::Explain),
            Some(BarPress::LookUp) => gloss.push(Gloss::LookUp),
            Some(BarPress::Highlight(pen, color)) => gloss.push(Gloss::Highlight(pen, color)),
            None => {}
        }
    }
    match follow {
        Some(LinkTarget::Page(page)) => view.go_to(page, sizes.len()),
        // Checked when read; checked again before leaving the app.
        Some(LinkTarget::Web(url)) if web_address(url.as_bytes()).is_some() => {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        _ => {}
    }
}

/// A key at a focused stop does what a click there would.
fn at_stop(
    press: Option<Press>,
    placed: &[study::marks::Placed],
    shaded: &[vocab::Shaded],
    picking: bool,
    gloss: &mut Vec<Gloss>,
) -> Option<LinkTarget> {
    let mark = |i: usize| placed.get(i);
    match press? {
        Press::Go(Stop::Link(target)) => return Some(target),
        Press::Go(Stop::Mark(i)) => {
            let m = mark(i)?;
            gloss.push(match picking {
                true => Gloss::Toggle(m.id),
                false => Gloss::Open {
                    mark: m.id,
                    at: m.bounds(),
                },
            });
        }
        Press::Go(Stop::Word(i)) => {
            let w = shaded.get(i).filter(|_| !picking)?;
            gloss.push(Gloss::Shaded {
                page: w.page,
                chars: w.chars.clone(),
                at: w.bounds(),
            });
        }
        Press::Menu(i) => {
            let m = mark(i).filter(|_| !picking)?;
            let at = m.bounds().left_bottom();
            gloss.push(Gloss::Menu { mark: m.id, at });
        }
    }
    None
}

/// A key's glide along the strip. The view runs it, so quick presses add up.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Glide {
    from: f32,
    to: f32,
    start: f64,
    secs: f64,
    /// Where it put the strip last frame.
    at: f32,
}

impl Glide {
    fn new(from: f32, to: f32, now: f64, a: ScrollAnimation) -> Self {
        let secs = ((to - from).abs() / a.points_per_second).clamp(a.duration.min, a.duration.max);
        Self {
            from,
            to,
            start: now,
            secs: f64::from(secs),
            at: from,
        }
    }

    /// Moves the whole glide `d` along, as auto-scroll moves the strip.
    fn shift(&mut self, d: f32) {
        self.from += d;
        self.to += d;
        self.at += d;
    }

    /// This frame's position, and whether the glide is over.
    fn step(&mut self, now: f64) -> (f32, bool) {
        let t = ((now - self.start) / self.secs.max(1e-6)).clamp(0.0, 1.0) as f32;
        self.at = self.from + (self.to - self.from) * egui::emath::ease_in_ease_out(t);
        (self.at, t >= 1.0)
    }
}

/// This frame's move along the strip: the keys' and auto-scroll's together.
fn keyed_scroll(view: &mut PdfView, ctx: &egui::Context, len: f32, now: f64) -> Option<f32> {
    let keyed = steered(view, ctx, len, now);
    let drift = drifted(view, ctx, now);
    // A key's glide rides along, or it would read the drift as a wheel.
    if let (Some(d), Some(g)) = (drift, &mut view.aim) {
        g.shift(d);
    }
    match (keyed, drift) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
    }
}

/// Auto-scroll's step, at the same pace whatever the frame rate.
fn drifted(view: &mut PdfView, ctx: &egui::Context, now: f64) -> Option<f32> {
    let last = view.drift?;
    let size = view.layout.as_ref()?.size;
    let (here, end) = match view.mode {
        ViewMode::Horizontal => (view.offset.x, size.x - view.viewport.x),
        _ => (view.offset.y, size.y - view.viewport.y),
    };
    // Paged modes have nowhere to drift; at the strip's end it is done.
    if view.mode.paged() || here >= end - 0.5 {
        view.drift = None;
        return None;
    }
    view.drift = Some(now);
    ctx.request_repaint();
    // A held press, as a selection's drag, holds it still; so does a jump.
    if view.jump.is_some() || ctx.input(|i| i.pointer.any_down()) {
        return None;
    }
    let dt = (now - last).clamp(0.0, MAX_FRAME) as f32;
    Some((AUTO_SPEED * dt).min(end - here))
}

/// The keys' scroll along the strip, in points.
fn steered(view: &mut PdfView, ctx: &egui::Context, len: f32, now: f64) -> Option<f32> {
    if view.mode.paged() {
        view.hold = None;
        view.nudge = None;
        view.aim = None;
        return None;
    }
    let here = if view.mode == ViewMode::Horizontal {
        view.offset.x
    } else {
        view.offset.y
    };
    // A wheel or a drag moved the strip meanwhile: the reader wins.
    if view.aim.is_some_and(|g| (g.at - here).abs() > 1.0) {
        view.aim = None;
    }
    let mut keyed = None;
    if let Some(n) = view.nudge.take() {
        // A key pressed mid-glide adds to where that glide was going.
        let to = view.aim.map_or(here, |g| g.to) + n.points(len);
        let a = motion::jump(ctx);
        view.aim = (a.duration.max > 0.0).then(|| Glide::new(here, to, now, a));
        keyed = Some(to - here);
    }
    if let Some(g) = &mut view.aim {
        let (at, done) = g.step(now);
        keyed = Some(at - here);
        if done {
            view.aim = None;
        } else {
            ctx.request_repaint();
        }
    }
    let Some(mut hold) = view.hold else {
        return keyed;
    };
    if !ctx.input(|i| keymap::held(i, hold.arrow)) {
        view.hold = None;
        return keyed;
    }
    if now - hold.since >= HOLD_DELAY {
        view.aim = None;
        let dt = (now - hold.last).clamp(0.0, MAX_FRAME) as f32;
        keyed = Some(hold.arrow.sign() * HOLD_SPEED * dt);
    }
    hold.last = now;
    view.hold = Some(hold);
    ctx.request_repaint();
    keyed
}

/// ⌘C or Ctrl+C copies the selection, unless a text field has focus.
fn copy(ctx: &egui::Context, view: &PdfView, doc: &Document, texts: &TextLayers) {
    let asked = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
    if !asked || ctx.egui_wants_keyboard_input() {
        return;
    }
    if let Some(text) = selected_text(view, doc, texts) {
        ctx.copy_text(text);
    }
}

/// The selection's text; none if nothing is selected.
fn selected_text(view: &PdfView, doc: &Document, texts: &TextLayers) -> Option<String> {
    let sel = view.selection.filter(|s| !s.is_empty())?;
    // Pages scrolled out of the cache are read again, here.
    Some(sel.text(|page| {
        texts
            .get(view.doc, page)
            .or_else(|| doc.text_layer(page).ok().map(Arc::new))
    }))
}

/// The bar under a finished selection, while any of it is on screen.
fn selection_bar(
    ctx: &egui::Context,
    view: &PdfView,
    texts: &TextLayers,
    room: Rect,
    look: &HighlightSettings,
) -> Option<BarPress> {
    let sel = view.selection.filter(|s| !s.is_empty())?;
    let id = egui::Id::new(("study-bar", view.doc));
    // Hidden while a press elsewhere may be selecting; a press on it stays.
    let bar = egui::LayerId::new(egui::Order::Foreground, id);
    let (down, from) = ctx.input(|i| (i.pointer.any_down(), i.pointer.press_origin()));
    if down && from.and_then(|p| ctx.layer_id_at(p)) != Some(bar) {
        return None;
    }
    let mut shown = Rect::NOTHING;
    for sheet in &view.sheets {
        let Some(layer) = texts.get(view.doc, sheet.page) else {
            continue;
        };
        let Some(range) = sel.on_page(sheet.page, layer.text.len()) else {
            continue;
        };
        for b in layer.text.boxes(range) {
            let r = sheet.to_screen(b).intersect(sheet.shown).intersect(room);
            if r.is_positive() {
                shown = shown.union(r);
            }
        }
    }
    if !shown.is_positive() {
        return None;
    }
    study::show_bar(ctx, id, shown, room, look)
}

/// The strips' scroll along their axis; the paged modes' page.
fn progress(view: &PdfView, layout: &Layout, out: &ScrollOutput<()>, count: usize) -> f32 {
    let room = (out.content_size - out.inner_rect.size()).max(Vec2::ZERO);
    let (done, total) = match view.mode {
        ViewMode::Vertical => (out.state.offset.y, room.y),
        ViewMode::Horizontal => (out.state.offset.x, room.x),
        ViewMode::Single | ViewMode::Spread => {
            let last = layout.pages().end.max(1);
            return last as f32 / count.max(1) as f32;
        }
    };
    if total <= 0.5 {
        1.0
    } else {
        (done / total).clamp(0.0, 1.0)
    }
}

/// A tiled page's grid, drawn over its backdrop where tiles have arrived.
struct Tiles {
    /// What of the page shows: a stream cuts it to its band.
    clip: Rect,
    page: Rect,
    anchor: Anchor,
    tiling: Tiling,
    /// The tiles' scale; its part is set per tile.
    key: PageKey,
}

impl Tiles {
    /// Draws the tiles on screen, asking for missing ones if `ask`.
    fn paint(
        &self,
        ui: &Ui,
        pages: &mut PageCache,
        mut ask: Option<(&mut Demand, Urgency)>,
        (quads, tone): (&mut Vec<Quad>, PageTone),
    ) {
        let t = &self.tiling;
        let size = vec2(t.width as f32, t.height as f32);
        let to_screen = self.page.size() / size;
        let clip = self.clip;
        for row in 0..t.rows() {
            for col in 0..t.cols() {
                let Some([x, y, w, h]) = t.rect(col, row) else {
                    continue;
                };
                let at = vec2(x as f32, y as f32) * to_screen;
                let rect =
                    Rect::from_min_size(self.page.min + at, vec2(w as f32, h as f32) * to_screen);
                if !rect.intersects(clip) {
                    continue;
                }
                let (Ok(col), Ok(row)) = (u8::try_from(col), u8::try_from(row)) else {
                    continue;
                };
                let key = PageKey {
                    part: Part::Tile { col, row },
                    ..self.key
                };
                match pages.lookup_tile(key) {
                    Some(PageTexture::Gpu(page)) => {
                        let quad = Quad::page((rect, clip), Some(page), tone, self.anchor);
                        quads.push(quad);
                    }
                    Some(PageTexture::Egui(h)) => {
                        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                        let painter = ui.painter().with_clip_rect(clip);
                        painter.image(h.id(), rect, uv, Color32::WHITE);
                    }
                    None => {
                        if let Some((demand, urgency)) = ask.as_mut()
                            && !pages.touch(key)
                        {
                            demand.push((*urgency, key.into()));
                        }
                    }
                }
            }
        }
    }
}

/// Glyphs may overhang the column a little: a hung hyphen, an italic.
const STREAM_SLACK: f32 = 24.0;
/// What a revealed hit keeps clear at the top: the find bar and its air.
const FIND_CLEARANCE: f32 = 72.0;
/// Air a revealed hit keeps from the other edges.
const SEEK_MARGIN: f32 = 24.0;

/// The whole page placed so `band` fills `cut`, and a clip to the band.
fn streamed(cut: Rect, band: &Band, size: Option<&PageSize>, zoom: f32, ppp: f32) -> (Rect, Rect) {
    let size = size.map_or(vec2(band.width, band.height), |s| vec2(s.width, s.height));
    let min = cut.min - vec2(band.x, band.y) * zoom;
    let page = Rect::from_min_size(min, size * zoom).round_to_pixels(ppp);
    (page, cut.expand2(vec2(STREAM_SLACK * zoom, 0.0)))
}

/// What a page shows before it is drawn: a blank page, themed.
fn blank(t: &Tokens) -> Color32 {
    let [r, g, b] = t.page.blank().map(|c| (c * 255.0).round() as u8);
    Color32::from_rgb(r, g, b)
}

/// Painters for the parts of `clip` clear of `frame`'s round corners.
fn clear_of_corners(ui: &Ui, clip: Rect, (frame, radius): (Rect, f32)) -> Vec<egui::Painter> {
    if radius <= 0.0 {
        return vec![ui.painter().with_clip_rect(clip)];
    }
    let (f, r) = (frame, radius);
    let (top, bottom) = (f.top() + r, f.bottom() - r);
    let (left, right) = (f.left() + r, f.right() - r);
    [
        Rect::from_min_max(pos2(left, f.top()), pos2(right, top)),
        Rect::from_min_max(pos2(f.left(), top), pos2(f.right(), bottom)),
        Rect::from_min_max(pos2(left, bottom), pos2(right, f.bottom())),
    ]
    .into_iter()
    .map(|band| band.intersect(clip))
    .filter(|band| band.is_positive())
    .map(|band| ui.painter().with_clip_rect(band))
    .collect()
}

/// Draws `quads` from `first` again over `pictures` in `tone`, undimmed.
fn keep_pictures(
    quads: &mut Vec<Quad>,
    first: usize,
    pictures: impl Iterator<Item = Rect>,
    tone: PageTone,
) {
    let page: Vec<Quad> = quads.get(first..).unwrap_or_default().to_vec();
    for picture in pictures {
        for q in page.iter().filter(|q| q.texture.is_some()) {
            let clip = q.clip.intersect(picture);
            if clip.is_positive() {
                quads.push(Quad {
                    clip,
                    tone,
                    ..q.clone()
                });
            }
        }
    }
}

/// Paints a page, or queues it in `quads`; nothing crosses `round`'s
/// corners.
pub(crate) fn paint_page(
    ui: &Ui,
    ((rect, clip), anchor, round): ((Rect, Rect), Anchor, (Rect, f32)),
    look: Lookup,
    (t, shadow): (&Tokens, bool),
    quads: &mut Vec<Quad>,
) {
    // Shadows reach past the page; a cut page casts none.
    let painter = ui.painter().with_clip_rect(clip);
    let shade = clear_of_corners(ui, ui.clip_rect(), round);
    let bands = clear_of_corners(ui, clip, round);
    // Themed pages match the gutter's paper; only this outlines them.
    if shadow {
        let lift = |offset, blur, alpha| egui::Shadow {
            offset: [0, offset],
            blur,
            spread: 0,
            color: Color32::from_black_alpha(alpha),
        };
        // mareader's `0 1px 2px` and `0 8px 24px -6px` box shadows.
        for band in &shade {
            band.add(lift(1, 2, 31).as_shape(rect, CornerRadius::ZERO));
            band.add(lift(8, 24, 56).as_shape(rect.shrink(6.0), CornerRadius::ZERO));
        }
    }
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    match look {
        Lookup::Sharp(PageTexture::Gpu(page)) | Lookup::Stale(PageTexture::Gpu(page)) => {
            quads.push(Quad::page((rect, clip), Some(page), t.page, anchor));
        }
        // Without a GPU there is no page pass, so pages stay unthemed.
        Lookup::Sharp(PageTexture::Egui(h)) | Lookup::Stale(PageTexture::Egui(h)) => {
            painter.image(h.id(), rect, uv, Color32::WHITE);
        }
        // A blank page, themed and textured; the fill is the no-GPU fallback.
        Lookup::Missing => {
            for band in &bands {
                band.rect_filled(rect, 0.0, blank(t));
            }
            quads.push(Quad::page((rect, clip), None, t.page, anchor));
        }
        Lookup::Failed => {
            for band in &bands {
                band.rect_filled(rect, 0.0, blank(t));
            }
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

    fn column(zoom: f32) -> Layout {
        let inputs = Inputs {
            mode: ViewMode::Vertical,
            frame: vec2(644.0, 800.0),
            inset: 0.0,
            zoom,
            focus: 0,
            gap: GAP,
            margin: MARGIN,
            align: egui::Align::Center,
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
        view.arrow(Arrow::Right, 20, 0.0);
        assert_eq!((view.focus, view.current), (8, 8));
        view.set_mode(ViewMode::Horizontal, true, 0.0);
        assert_eq!((view.zoom.fit, view.reveal), (None, Some(8)));
        // Along the strip, the arrow is the scroll's, not a turn.
        view.arrow(Arrow::Right, 20, 0.0);
        assert_eq!(view.current, 8);
        view.arrow(Arrow::Up, 20, 0.0);
        assert_eq!(view.current, 8);
        // Across the column, left and right still turn.
        view.set_mode(ViewMode::Vertical, true, 0.0);
        view.arrow(Arrow::Left, 20, 0.0);
        assert_eq!((view.current, view.reveal), (7, Some(7)));
        Ok(())
    }
}
