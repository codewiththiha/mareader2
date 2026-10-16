//! Each pane's counter, progress line and hover-revealed page bar.

use egui::{
    Align, Context, CornerRadius, FontId, Id, Key, LayerId, Layout, Order, Rect, RichText, Slider,
    Stroke, TextEdit, UiBuilder, pos2, vec2,
};
use mareader_settings::{LayoutSettings, PageIndicatorStyle};
use mareader_workspace::PaneId;

use crate::backdrop::Backdrop;
use crate::command::{Command, Commands};
use crate::documents::Reading;
use crate::gpu::{Look, Reach};
use crate::icons::Icon;
use crate::motion;
use crate::theme::Tokens;
use crate::titlebar::{FADE, HIDE_GRACE};
use crate::widgets;

pub const BAR_H: f32 = 40.0;
/// The strip along a pane's bottom edge that brings the bar up.
pub const EDGE: f32 = 8.0;
/// Narrower than this, the bar keeps its buttons and drops the slider.
const SLIDER_MIN_PANE: f32 = 260.0;

/// One pane's bar between frames.
#[derive(Debug, Default, Clone)]
pub struct PageBar {
    shown: bool,
    left_at: Option<f64>,
    /// The page number being typed, while the entry has focus.
    typing: Option<String>,
}

impl PageBar {
    /// Shows the bar as if hovered; it hides after the usual grace.
    pub fn reveal(&mut self) {
        self.shown = true;
    }

    fn update_hover(&mut self, ctx: &Context, pane: Rect, held: bool) {
        let now = ctx.input(|i| i.time);
        let reach = if self.shown { BAR_H } else { EDGE };
        let band = Rect::from_min_max(pos2(pane.left(), pane.bottom() - reach), pane.max);
        let inside = ctx.pointer_hover_pos().is_some_and(|p| band.contains(p));
        if inside || held {
            self.shown = true;
            self.left_at = None;
        } else if self.shown {
            let waited = now - *self.left_at.get_or_insert(now);
            if waited >= HIDE_GRACE {
                self.shown = false;
                self.left_at = None;
            } else {
                ctx.request_repaint_after_secs((HIDE_GRACE - waited) as f32);
            }
        }
    }
}

/// The counter's text: "3 / 20", or "15%" through the pages.
pub fn counter(reading: &Reading, style: PageIndicatorStyle) -> String {
    let (page, count) = (reading.current + 1, reading.count().max(1));
    match style {
        PageIndicatorStyle::PageNumber => format!("{page} / {count}"),
        PageIndicatorStyle::Percentage => {
            format!("{}%", (page as f32 / count as f32 * 100.0).round())
        }
    }
}

/// How the bars look this frame, and which glass slot this one takes.
pub struct BarLook<'a> {
    pub layout: &'a LayoutSettings,
    pub backdrop: &'a Backdrop,
    pub slot: usize,
}

/// Draws `pane`'s counter, progress line and bar over its document.
pub fn show(
    ctx: &Context,
    pane: Rect,
    reading: &Reading,
    look: &BarLook<'_>,
    bar: &mut PageBar,
    out: &mut Commands,
) {
    let layout = look.layout;
    let entry_id = Id::new(("page-entry", reading.pane));
    let typing = ctx.memory(|m| m.has_focus(entry_id));
    let home = Rect::from_min_max(pos2(pane.left(), pane.bottom() - BAR_H), pane.max);
    let dragging = bar.shown
        && ctx.input(|i| {
            let origin = i.pointer.press_origin();
            i.pointer.any_down() && origin.is_some_and(|p| home.contains(p))
        });
    let tabbing = crate::focus::within(ctx, home);
    bar.update_hover(ctx, pane, typing || dragging || tabbing);
    let id = Id::new(("page-bar", reading.pane));
    let fade = ctx.animate_bool_with_time(id, bar.shown, motion::secs(ctx, FADE));
    let t = Tokens::get(ctx);
    let layer = LayerId::new(Order::Middle, id.with("marks"));
    let painter = ctx.layer_painter(layer).with_clip_rect(pane);
    if layout.progress_bar {
        let width = pane.width() * reading.progress.clamp(0.0, 1.0);
        let line = Rect::from_min_size(pos2(pane.left(), pane.bottom() - 2.0), vec2(width, 2.0));
        painter.rect_filled(line, CornerRadius::ZERO, t.accent.gamma_multiply(0.8));
    }
    // The counter gives way to the bar rather than sit under it.
    if layout.page_indicator && fade < 1.0 {
        let text = counter(reading, layout.page_indicator_style);
        let white = egui::Color32::from_white_alpha(230).gamma_multiply(1.0 - fade);
        let galley = painter.layout_no_wrap(text, FontId::proportional(11.0), white);
        let size = galley.size() + vec2(16.0, 4.0);
        let badge = Rect::from_min_size(pane.max - size - vec2(12.0, 12.0), size);
        let fill = egui::Color32::from_black_alpha(150).gamma_multiply(1.0 - fade);
        painter.rect_filled(badge, CornerRadius::same(6), fill);
        painter.galley(badge.center() - galley.size() / 2.0, galley, white);
    }
    if fade <= 0.0 {
        return;
    }
    // It rises a little as it fades in, as in mareader.
    let rise = (1.0 - fade) * 12.0;
    let rect = home.translate(vec2(0.0, rise));
    egui::Area::new(id)
        .fade_in(false)
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false)
        .interactable(bar.shown)
        .show(ctx, |ui| {
            ui.set_opacity(fade);
            ui.set_min_size(rect.size());
            ui.set_clip_rect(pane);
            let glass = Look {
                slot: look.slot,
                reach: Reach::Above,
                opacity: fade,
            };
            look.backdrop.paint(ui, rect, glass);
            ui.painter()
                .hline(rect.x_range(), rect.top(), Stroke::new(1.0, t.line));
            let row = rect.shrink2(vec2(10.0, 0.0));
            let builder = UiBuilder::new()
                .max_rect(row)
                .layout(Layout::left_to_right(Align::Center));
            ui.scope_builder(builder, |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                contents(ui, reading, bar, entry_id, pane.width(), out);
            });
            if ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(rect) {
                out.push(Command::FocusPane(reading.pane));
            }
        });
}

fn contents(
    ui: &mut egui::Ui,
    reading: &Reading,
    bar: &mut PageBar,
    entry_id: Id,
    pane_width: f32,
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let pane: PaneId = reading.pane;
    let count = reading.count();
    if widgets::icon_button(ui, Icon::ChevronLeft, "Previous page", false).clicked() {
        out.push(Command::Turn {
            pane,
            forward: false,
        });
    }
    let mut text = bar
        .typing
        .clone()
        .unwrap_or_else(|| (reading.current + 1).to_string());
    let entry = TextEdit::singleline(&mut text)
        .id(entry_id)
        .desired_width(44.0)
        .horizontal_align(Align::Center)
        .font(FontId::proportional(t.text))
        .frame(egui::Frame::NONE);
    let r = ui.add(entry);
    widgets::name(&r, "Page number");
    // A bordered field, as on the bar's glass egui's own frame vanishes.
    let edge = if r.has_focus() { t.accent } else { t.line };
    ui.painter().rect_stroke(
        r.rect.expand2(vec2(2.0, 4.0)),
        CornerRadius::same(6),
        Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    if r.has_focus() {
        bar.typing = Some(text.clone());
    }
    if r.lost_focus() {
        // A number jumps; anything else snaps back to the page shown.
        let entered = ui.input(|i| i.key_pressed(Key::Enter));
        if let (true, Ok(n @ 1..)) = (entered, text.trim().parse::<usize>()) {
            out.push(Command::GoTo { pane, page: n - 1 });
        }
        bar.typing = None;
    }
    ui.label(RichText::new(format!("/ {count}")).color(t.muted));
    if widgets::icon_button(ui, Icon::ChevronRight, "Next page", false).clicked() {
        out.push(Command::Turn {
            pane,
            forward: true,
        });
    }
    if pane_width < SLIDER_MIN_PANE || count < 2 {
        return;
    }
    let mut page = reading.current + 1;
    ui.spacing_mut().slider_width = (ui.available_width() - 4.0).max(40.0);
    let slider = Slider::new(&mut page, 1..=count)
        .show_value(false)
        .trailing_fill(true);
    let r = ui.add(slider);
    widgets::name(&r, "Page");
    if r.changed() {
        out.push(Command::GoTo {
            pane,
            page: page - 1,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counter_reads_as_pages_or_a_percentage() -> Result<(), String> {
        let reading = crate::testing::reading(20, 2)?;
        assert_eq!(counter(&reading, PageIndicatorStyle::PageNumber), "3 / 20");
        assert_eq!(counter(&reading, PageIndicatorStyle::Percentage), "15%");
        Ok(())
    }
}
