//! The library page: every book, last opened first, one row each.

use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, TextFormat, Ui, pos2, vec2};
use mareader_library::{Book, Library};

use super::LibraryState;
use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::{self, Tokens};

const ROW_H: f32 = 56.0;
const MAX_WIDTH: f32 = 880.0;

/// The page over `ui`'s whole rect; `top` clears the title bar.
pub fn show(ui: &mut Ui, library: &LibraryState, top: f32, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, t.paper);
    let width = (full.width() - 48.0).min(MAX_WIDTH);
    let left = full.center().x - width / 2.0;
    let body = Rect::from_min_max(
        pos2(left, top.max(full.top()) + 20.0),
        pos2(left + width, full.bottom()),
    );
    let model = library.model();
    if model.is_empty() {
        if !library.loading() {
            empty_state(ui, full, &t, out);
        }
        return;
    }
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
    header(&mut ui, model.len(), &t, out);
    ui.add_space(8.0);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(&mut ui, |ui| {
            for book in model.recent() {
                row(ui, model, book, &t, out);
            }
            ui.add_space(24.0);
        });
}

fn header(ui: &mut Ui, count: usize, t: &Tokens, out: &mut Commands) {
    ui.horizontal(|ui| {
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
            if open_button(ui, t).clicked() {
                out.push(Command::ShowOpenDialog { beside: false });
            }
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

fn row(ui: &mut Ui, model: &Library, book: &Book, t: &Tokens, out: &mut Commands) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let title = book.title.value.as_str();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    if resp.clicked() {
        out.push(Command::OpenBook(book.id));
    }
    let missing = book.missing();
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, t.radius, t.hover);
    }
    // A missing book stays listed, faded, until found or removed.
    let fade = |c: Color32| if missing { c.gamma_multiply(0.5) } else { c };
    let badge = Rect::from_min_size(rect.left_center() + vec2(12.0, -11.0), vec2(44.0, 22.0));
    p.rect_filled(badge, t.radius, fade(t.accent_soft));
    let label = book.format.label();
    p.text(
        badge.center(),
        Align2::CENTER_CENTER,
        label,
        theme::medium(11.0),
        fade(t.accent),
    );
    let when = ago(super::wall_ms().saturating_sub(book.opened.value));
    let when = p.layout_no_wrap(when, FontId::proportional(12.0), t.muted);
    let right = rect.right() - 12.0;
    p.galley(
        pos2(right - when.size().x, rect.center().y - when.size().y / 2.0),
        when,
        t.muted,
    );
    let x = badge.right() + 14.0;
    let wide = right - x - 96.0;
    let line = |text: &str, font: FontId, color: Color32, y: f32| {
        let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
        job.wrap = TextWrapping::truncate_at_width(wide.max(40.0));
        let g = p.layout_job(job);
        p.galley(pos2(x, y - g.size().y / 2.0), g, color);
    };
    line(
        title,
        theme::medium(14.0),
        fade(t.ink),
        rect.center().y - 9.0,
    );
    let detail = detail(model, book);
    line(
        &detail,
        FontId::proportional(12.0),
        fade(t.muted),
        rect.center().y + 10.0,
    );
    p.hline(
        rect.x_range().shrink(12.0),
        rect.bottom(),
        Stroke::new(1.0, t.line),
    );
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
    use super::ago;

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
}
