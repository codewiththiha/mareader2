//! The library page: every book as a grid of covers or a list.

use egui::epaint::Shadow;
use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align2, Color32, CornerRadius, FontId, Painter, Rect, Sense, Stroke, StrokeKind, TextFormat,
    Ui, pos2, vec2,
};
use mareader_library::{Book, COLUMNS_MAX, COLUMNS_MIN, CoverFit, Layout, Library, View};

use super::LibraryState;
use super::covers::{self, Cover, Covers};
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::motion;
use crate::theme::{self, Tokens};

const ROW_H: f32 = 60.0;
/// How far a list row's content sits in from its edges.
const ROW_INSET: f32 = 12.0;
/// The list reads best narrow; the grid takes the window.
const LIST_WIDTH: f32 = 880.0;
const GRID_WIDTH: f32 = 1_600.0;
const MARGIN: f32 = 24.0;
const GAP: f32 = 20.0;
/// The narrowest card an automatic grid makes.
const CARD_MIN: f32 = 150.0;
/// A cover's box: A4, height over width.
const A4: f32 = 297.0 / 210.0;
/// Covers stranger than these are trimmed to them.
const SHAPES: std::ops::RangeInclusive<f32> = 0.55..=1.8;
/// Title, detail and progress under a card's cover.
const CARD_TEXT: f32 = 54.0;

/// The page over `ui`'s whole rect; `top` clears the title bar.
pub fn show(ui: &mut Ui, library: &mut LibraryState, view: &View, top: f32, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, t.paper);
    let loading = library.loading();
    let (model, covers) = library.parts();
    if model.is_empty() {
        if !loading {
            empty_state(ui, full, &t, out);
        }
        return;
    }
    let cap = match view.layout {
        Layout::Grid => GRID_WIDTH,
        Layout::List => LIST_WIDTH,
    };
    let width = (full.width() - 2.0 * MARGIN).min(cap);
    let left = full.center().x - width / 2.0;
    let body = Rect::from_min_max(
        pos2(left, top.max(full.top()) + 20.0),
        pos2(left + width, full.bottom()),
    );
    let auto = auto_columns(width);
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
    header(&mut ui, model.len(), view, auto, &t, out);
    ui.add_space(12.0);
    let books = view.sort(model.books());
    let mut cx = Cx {
        model,
        covers,
        fit: view.cover,
        t: &t,
        out,
    };
    // Rows are spaced by their own height; egui's gap would add to it.
    ui.spacing_mut().item_spacing.y = 0.0;
    match view.layout {
        Layout::Grid => grid(&mut ui, &books, view.columns.unwrap_or(auto), &mut cx),
        Layout::List => list(&mut ui, &books, &mut cx),
    }
}

/// What every card and row draws with.
struct Cx<'a> {
    model: &'a Library,
    covers: &'a mut Covers,
    fit: CoverFit,
    t: &'a Tokens,
    out: &'a mut Commands,
}

/// As many cards as fit at least [`CARD_MIN`] wide.
fn auto_columns(width: f32) -> u8 {
    let n = ((width + GAP) / (CARD_MIN + GAP)).floor();
    (n.clamp(0.0, 255.0) as u8).clamp(COLUMNS_MIN, COLUMNS_MAX)
}

fn header(ui: &mut Ui, count: usize, view: &View, auto: u8, t: &Tokens, out: &mut Commands) {
    // Lined up with the list rows' text, which sits in from their edges.
    let inset = match view.layout {
        Layout::Grid => 0.0,
        Layout::List => ROW_INSET,
    };
    ui.horizontal(|ui| {
        ui.add_space(inset);
        ui.label(
            egui::RichText::new("Library")
                .font(theme::medium(22.0))
                .color(t.ink),
        );
        let books = if count == 1 {
            "1 book".to_owned()
        } else {
            format!("{count} books")
        };
        ui.label(egui::RichText::new(books).color(t.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(inset);
            if open_button(ui, t).clicked() {
                out.push(Command::ShowOpenDialog { beside: false });
            }
            super::menu::button(ui, view, auto, out);
        });
    });
}

fn open_button(ui: &mut Ui, t: &Tokens) -> egui::Response {
    let button = egui::Button::image_and_text(Icon::Open.image(16.0), "Open…")
        .image_tint_follows_text_color(true)
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius)
        .min_size(vec2(96.0, 32.0));
    ui.add(button)
}

fn grid(ui: &mut Ui, books: &[&Book], columns: u8, cx: &mut Cx<'_>) {
    let cols = usize::from(columns.max(1));
    let width = ui.available_width();
    let card = ((width - GAP * (cols - 1) as f32) / cols as f32).max(1.0);
    let height = card * A4 + CARD_TEXT;
    let rows = books.len().div_ceil(cols);
    egui::ScrollArea::vertical()
        .id_salt("library-grid")
        .auto_shrink(false)
        .show_rows(ui, height + GAP, rows, |ui, range| {
            for row in range.clone() {
                let (line, _) = ui.allocate_exact_size(vec2(width, height + GAP), Sense::hover());
                for col in 0..cols {
                    let Some(book) = books.get(row * cols + col) else {
                        break;
                    };
                    let x = line.left() + col as f32 * (card + GAP);
                    let rect = Rect::from_min_size(pos2(x, line.top()), vec2(card, height));
                    grid_card(ui, rect, book, cx);
                }
            }
            prefetch(cx.covers, books, range, cols);
        });
}

fn list(ui: &mut Ui, books: &[&Book], cx: &mut Cx<'_>) {
    egui::ScrollArea::vertical()
        .id_salt("library-list")
        .auto_shrink(false)
        .show_rows(ui, ROW_H, books.len(), |ui, range| {
            for book in books.get(range.clone()).unwrap_or_default() {
                list_row(ui, book, cx);
            }
            prefetch(cx.covers, books, range, 1);
        });
}

/// Asks for the covers a screen below, then above, what shows.
fn prefetch(covers: &mut Covers, books: &[&Book], rows: std::ops::Range<usize>, cols: usize) {
    let screen = rows.len().max(1);
    let below = rows.end..rows.end + screen;
    let above = (rows.start.saturating_sub(screen)..rows.start).rev();
    for row in below.chain(above) {
        for book in books.iter().skip(row * cols).take(cols) {
            covers.prefetch(book);
        }
    }
}

/// A clickable card or row, named by the book's title.
fn hit(ui: &mut Ui, rect: Rect, book: &Book, cx: &mut Cx<'_>) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(book.id), Sense::click());
    let title = book.title.value.as_str();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    if resp.clicked() {
        cx.out.push(Command::OpenBook(book.id));
    }
    resp
}

fn grid_card(ui: &mut Ui, rect: Rect, book: &Book, cx: &mut Cx<'_>) {
    let resp = hit(ui, rect, book, cx);
    let t = cx.t;
    // A missing book stays shown, faded, until found or removed.
    let missing = book.missing();
    let fade = if missing { 0.5 } else { 1.0 };
    if resp.hovered() {
        let pad = rect.expand2(vec2(8.0, 8.0));
        ui.painter().rect_filled(pad, t.radius_lg, t.hover);
    }
    let bounds = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width() * A4));
    cover(ui, bounds, book, cx, fade, false);
    let p = ui.painter();
    if missing {
        let tag = p.layout_no_wrap("Missing".into(), theme::medium(11.0), t.ink);
        let pill = Rect::from_min_size(
            pos2(bounds.right() - tag.size().x - 20.0, bounds.top() + 8.0),
            tag.size() + vec2(12.0, 6.0),
        );
        p.rect_filled(pill, t.radius, t.surface);
        p.galley(pill.min + vec2(6.0, 3.0), tag, t.ink);
    }
    let width = rect.width();
    let mut y = bounds.bottom() + 8.0;
    let title = book.title.value.as_str();
    y += line(
        p,
        title,
        theme::medium(13.0),
        faded(t.ink, fade),
        (rect.left(), y),
        width,
    );
    let detail = card_detail(cx.model, book);
    let small = FontId::proportional(12.0);
    y += line(
        p,
        &detail,
        small,
        faded(t.muted, fade),
        (rect.left(), y + 1.0),
        width,
    );
    if let Some(done) = cx.model.position(book.id).and_then(|p| p.fraction()) {
        let track = Rect::from_min_size(pos2(rect.left(), y + 7.0), vec2(width, 3.0));
        p.rect_filled(track, 1.5, faded(t.line, fade));
        let read = Rect::from_min_size(track.min, vec2(width * done, 3.0));
        p.rect_filled(read, 1.5, faded(t.accent, fade));
    }
}

fn list_row(ui: &mut Ui, book: &Book, cx: &mut Cx<'_>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let resp = hit(ui, rect, book, cx);
    let t = cx.t;
    let missing = book.missing();
    let fade = if missing { 0.5 } else { 1.0 };
    if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    let thumb = Rect::from_center_size(
        pos2(rect.left() + ROW_INSET + 17.0, rect.center().y),
        vec2(34.0, 34.0 * A4),
    );
    cover(ui, thumb, book, cx, fade, true);
    let p = ui.painter();
    let when = ago(super::wall_ms().saturating_sub(book.opened.value));
    let when = p.layout_no_wrap(when, FontId::proportional(12.0), faded(t.muted, fade));
    let right = rect.right() - ROW_INSET;
    let at = pos2(right - when.size().x, rect.center().y - when.size().y / 2.0);
    p.galley(at, when, t.muted);
    let x = thumb.right() + 14.0;
    let wide = (right - x - 96.0).max(40.0);
    let title = book.title.value.as_str();
    let ink = faded(t.ink, fade);
    let center = |y: f32, h: f32| y - h / 2.0;
    let job = |text: &str, font: FontId, color: Color32| {
        let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
        job.wrap = TextWrapping::truncate_at_width(wide);
        p.layout_job(job)
    };
    let g = job(title, theme::medium(14.0), ink);
    p.galley(pos2(x, center(rect.center().y - 9.0, g.size().y)), g, ink);
    let detail = format!("{} · {}", book.format.label(), detail(cx.model, book));
    let muted = faded(t.muted, fade);
    let g = job(&detail, FontId::proportional(12.0), muted);
    p.galley(
        pos2(x, center(rect.center().y + 10.0, g.size().y)),
        g,
        muted,
    );
    p.hline(
        rect.x_range().shrink(ROW_INSET),
        rect.bottom(),
        Stroke::new(1.0, t.line),
    );
}

/// One truncated line at `at`; returns its height.
fn line(p: &Painter, text: &str, font: FontId, color: Color32, at: (f32, f32), w: f32) -> f32 {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    job.wrap = TextWrapping::truncate_at_width(w.max(8.0));
    let g = p.layout_job(job);
    let h = g.size().y;
    p.galley(pos2(at.0, at.1), g, color);
    h
}

fn faded(c: Color32, fade: f32) -> Color32 {
    c.gamma_multiply(fade)
}

/// The cover in `bounds`: art if read, else a placeholder or a drawn one.
fn cover(ui: &Ui, bounds: Rect, book: &Book, cx: &mut Cx<'_>, fade: f32, small: bool) {
    let t = cx.t;
    let radius = if small { 3.0 } else { t.radius.min(6.0) };
    let p = ui.painter();
    match cx.covers.get(ui.ctx(), book, motion::enabled(ui.ctx())) {
        Cover::Art { texture, alpha } => {
            let shape = texture.size.y / texture.size.x;
            let size = vec2(1.0, shape.clamp(*SHAPES.start(), *SHAPES.end()));
            let rect = match cx.fit {
                CoverFit::Fit if !small => covers::fit(bounds, size),
                _ => bounds,
            };
            if alpha < 1.0 {
                p.rect_filled(rect, radius, faded(t.hover, fade));
            }
            let shadow = Shadow {
                offset: [0, 2],
                blur: if small { 3 } else { 10 },
                spread: 0,
                color: faded(t.shadow, fade * alpha),
            };
            p.add(shadow.as_shape(rect, CornerRadius::from(radius)));
            egui::Image::from_texture(texture)
                .uv(covers::crop(rect, texture.size))
                .corner_radius(radius)
                .tint(Color32::WHITE.gamma_multiply(fade * alpha))
                .paint_at(ui, rect);
            let edge = Stroke::new(1.0, faded(t.line, 0.6 * fade * alpha));
            p.rect_stroke(rect, radius, edge, StrokeKind::Inside);
        }
        Cover::Waiting => {
            p.rect_filled(bounds, radius, faded(t.hover, fade));
        }
        Cover::Drawn => drawn(p, bounds, book, t, fade, small),
    }
}

/// A cover for a book without art: a tinted card with its names.
fn drawn(p: &Painter, rect: Rect, book: &Book, t: &Tokens, fade: f32, small: bool) {
    let radius = if small { 3.0 } else { t.radius.min(6.0) };
    // A steady hue per book, softened toward the theme's surface.
    let mixed = book
        .content
        .0
        .wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835);
    let hue = ((mixed >> 96) % 360) as f32 / 360.0;
    let (s, v) = if t.dark { (0.35, 0.36) } else { (0.24, 0.9) };
    let tone = Color32::from(egui::ecolor::Hsva::new(hue, s, v, 1.0));
    let paper = tone.lerp_to_gamma(t.surface, 0.3);
    p.rect_filled(rect, radius, faded(paper, fade));
    let spine = Rect::from_min_size(rect.min, vec2(rect.width() * 0.06, rect.height()));
    let corners = CornerRadius {
        nw: radius as u8,
        sw: radius as u8,
        ne: 0,
        se: 0,
    };
    p.rect_filled(spine, corners, faded(t.ink, 0.08 * fade));
    p.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, faded(t.line, fade)),
        StrokeKind::Inside,
    );
    if small {
        return;
    }
    let pad = rect.width() * 0.12;
    let left = spine.right() + pad * 0.8;
    let width = rect.right() - pad - left;
    let size = (rect.width() / 9.0).clamp(11.0, 22.0);
    let block = |text: &str, font: FontId, color: Color32, rows: usize| {
        let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
        job.wrap = TextWrapping {
            max_width: width.max(8.0),
            max_rows: rows,
            break_anywhere: false,
            overflow_character: Some('…'),
        };
        p.layout_job(job)
    };
    let ink = faded(t.ink, fade);
    let title = block(&book.title.value, theme::medium(size), ink, 5);
    let mut y = rect.top() + pad * 1.2;
    let title_h = title.size().y;
    p.galley(pos2(left, y), title, ink);
    y += title_h + size * 0.6;
    let rule = Rect::from_min_size(pos2(left, y), vec2(width.min(size * 2.0), 2.0));
    p.rect_filled(rule, 1.0, faded(t.accent, fade));
    y += size * 0.6;
    if let Some(author) = book.author.value.as_deref() {
        let muted = faded(t.muted, fade);
        let g = block(author, FontId::proportional(size * 0.72), muted, 2);
        p.galley(pos2(left, y), g, muted);
    }
    p.text(
        pos2(left, rect.bottom() - pad),
        Align2::LEFT_BOTTOM,
        book.format.label(),
        theme::medium((size * 0.55).max(9.0)),
        faded(t.muted, fade),
    );
}

/// Under a card's title: the author, else how far through.
fn card_detail(model: &Library, book: &Book) -> String {
    if book.missing() {
        return "Moved or deleted".into();
    }
    let author = book.author.value.as_deref().map(str::trim);
    if let Some(author) = author.filter(|a| !a.is_empty()) {
        return author.to_owned();
    }
    match model.position(book.id) {
        Some(p) if p.pages > 0 => format!("Page {} of {}", p.page + 1, p.pages),
        _ => "Not started".into(),
    }
}

/// "Page 12 of 300 · 4%", or why it can't open.
fn detail(model: &Library, book: &Book) -> String {
    if book.missing() {
        return "Missing: moved or deleted".into();
    }
    let author = book.author.value.as_deref();
    let place = model.position(book.id).and_then(|p| {
        let pct = (p.fraction()? * 100.0).round();
        Some(format!("Page {} of {} · {pct}%", p.page + 1, p.pages))
    });
    let parts: Vec<&str> = [author, place.as_deref()].into_iter().flatten().collect();
    match parts.is_empty() {
        true => "Not started".into(),
        false => parts.join(" · "),
    }
}

/// "5 min ago": how long since, in the largest whole unit.
fn ago(ms: u64) -> String {
    let min = ms / 60_000;
    let (h, d) = (min / 60, min / 1_440);
    match () {
        _ if min < 1 => "Just now".into(),
        _ if h < 1 => format!("{min} min ago"),
        _ if d < 1 => format!("{h} h ago"),
        _ if d < 2 => "Yesterday".into(),
        _ if d < 14 => format!("{d} days ago"),
        _ if d < 60 => format!("{} weeks ago", d / 7),
        _ if d < 730 => format!("{} months ago", d / 30),
        _ => format!("{} years ago", d / 365),
    }
}

fn empty_state(ui: &mut Ui, rect: Rect, t: &Tokens, out: &mut Commands) {
    let at = Rect::from_center_size(rect.center(), vec2(rect.width(), 200.0));
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(at));
    ui.vertical_centered(|ui| {
        ui.add(Icon::Library.image(40.0).tint(t.muted));
        ui.add_space(10.0);
        let title = egui::RichText::new("Your library is empty").font(theme::medium(16.0));
        ui.label(title.color(t.ink));
        let hint = "Books you open appear here, ready to resume.";
        ui.label(egui::RichText::new(hint).size(13.0).color(t.muted));
        ui.add_space(14.0);
        if open_button(ui, t).clicked() {
            out.push(Command::ShowOpenDialog { beside: false });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{CARD_MIN, GAP, ago, auto_columns};

    #[test]
    fn times_read_in_the_largest_whole_unit() {
        let min = 60_000;
        assert_eq!(ago(10_000), "Just now");
        assert_eq!(ago(5 * min), "5 min ago");
        assert_eq!(ago(3 * 60 * min), "3 h ago");
        assert_eq!(ago(30 * 60 * min), "Yesterday");
        assert_eq!(ago(4 * 1_440 * min), "4 days ago");
        assert_eq!(ago(800 * 1_440 * min), "2 years ago");
    }

    #[test]
    fn automatic_columns_keep_cards_wide_within_the_limits() {
        assert_eq!(auto_columns(100.0), 2);
        assert_eq!(auto_columns(4.0 * CARD_MIN + 3.0 * GAP), 4);
        assert_eq!(auto_columns(4.0 * CARD_MIN + 3.0 * GAP - 1.0), 3);
        assert_eq!(auto_columns(10_000.0), 10);
        assert_eq!(auto_columns(f32::NAN), 2);
    }
}
