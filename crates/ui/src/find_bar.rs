//! The floating find bar: query, count, steps and the results list.

use egui::{Context, Frame, Id, Key, Margin, Order, Rect, RichText, Sense, TextEdit, Ui, pos2};
use mareader_search::{from_page, step};
use mareader_workspace::PaneId;

use crate::icons::Icon;
use crate::search::Found;
use crate::theme::Tokens;
use crate::widgets;

pub const PLACEHOLDER: &str = "Search in document…";
const WIDTH: f32 = 560.0;
/// From the pane's right edge, and below the title bar.
const INSET: f32 = 16.0;
/// Seconds after the last key before a query is asked, as mareader waits.
const SETTLE: f64 = 0.18;
/// Frames a hit's page has to bring its text before the reveal gives up.
const SEEK_FRAMES: u8 = 90;
const ROW_H: f32 = 44.0;
const LIST_MAX_H: f32 = 288.0;

/// One bar, on the pane it was opened over.
#[derive(Debug, Default)]
pub struct FindBar {
    pane: Option<PaneId>,
    /// Kept while closed, as mareader resumes its last query.
    query: String,
    focus: bool,
    /// When the query last changed, until it is asked.
    typed: Option<f64>,
    /// The query last asked, and of which document.
    asked: Option<(String, usize)>,
    /// The current hit.
    active: Option<usize>,
    /// An Enter waiting for its query's hits: forward or back.
    waiting: Option<bool>,
    list: bool,
    /// The hit being brought into view, and the frames left to try.
    seek: Option<(usize, u8)>,
    /// Where the bar was drawn last frame.
    rect: Option<Rect>,
}

impl FindBar {
    pub fn open(&mut self, pane: PaneId) {
        if self.pane != Some(pane) {
            self.active = None;
            self.asked = None;
        }
        self.pane = Some(pane);
        self.focus = true;
    }

    pub fn close(&mut self) {
        self.pane = None;
        self.waiting = None;
        self.seek = None;
        self.list = false;
    }

    pub fn pane(&self) -> Option<PaneId> {
        self.pane
    }

    /// Where the bar was drawn last frame, if open.
    pub fn rect(&self) -> Option<Rect> {
        self.rect.filter(|_| self.pane.is_some())
    }

    pub fn is_open(&self) -> bool {
        self.pane.is_some()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    /// Replaces the query, as if typed.
    pub fn set_query(&mut self, query: &str, now: f64) {
        if self.query != query {
            self.query = query.to_owned();
            self.typed = Some(now);
        }
    }

    /// A query asked at once, landing on its first hit: for shots.
    pub fn seed(&mut self, query: &str) {
        self.query = query.to_owned();
        self.typed = None;
        self.waiting = Some(true);
    }

    /// Whether to ask now: settled after typing, or new to this document.
    pub fn due(&self, now: f64, key: usize) -> bool {
        if self.query.trim().is_empty() {
            return false;
        }
        let fresh = self.asked.as_ref() != Some(&(self.query.clone(), key));
        fresh && self.typed.is_none_or(|t| now - t >= SETTLE)
    }

    /// The query was asked of document `key`: its hits start anew.
    pub fn asked(&mut self, key: usize) {
        self.asked = Some((self.query.clone(), key));
        self.typed = None;
        self.active = None;
        self.seek = None;
    }

    /// The hit to bring into view and how many tries are left, if any.
    pub fn seeking(&mut self) -> Option<usize> {
        let (hit, left) = self.seek?;
        self.seek = left.checked_sub(1).map(|l| (hit, l));
        Some(hit)
    }

    pub fn sought(&mut self) {
        self.seek = None;
    }

    fn activate(&mut self, hit: usize) {
        self.active = Some(hit);
        self.seek = Some((hit, SEEK_FRAMES));
    }

    /// A step from the keys or buttons: through the hits, or once they come.
    fn step(&mut self, found: Option<&Found>, forward: bool, now: f64) {
        match found.filter(|f| !f.hits.is_empty() && self.active.is_some()) {
            Some(f) => {
                if let Some(next) = step(f.hits.len(), self.active, forward) {
                    self.activate(next);
                }
            }
            None => {
                self.waiting = Some(forward);
                // Asked at once, not after the pause.
                self.typed = self.typed.map(|_| now - SETTLE);
            }
        }
    }

    /// A waiting step lands once there's a hit from `page` on, or all are in.
    pub fn settle(&mut self, found: Option<&Found>, page: usize) {
        let (Some(forward), Some(f)) = (self.waiting, found) else {
            return;
        };
        let pages: Vec<usize> = f.hits.iter().map(|h| h.page).collect();
        let ahead = pages.last().is_some_and(|&p| p >= page);
        if !(f.done() || forward && ahead) {
            return;
        }
        self.waiting = None;
        let Some(first) = from_page(&pages, page) else {
            return;
        };
        let hit = if forward {
            Some(first)
        } else {
            step(pages.len(), Some(first), false)
        };
        if let Some(hit) = hit {
            self.activate(hit);
        }
    }

    /// Top right of its pane; closes on a press elsewhere in a pane.
    pub fn show(
        &mut self,
        ctx: &Context,
        panes: &[(PaneId, Rect)],
        top: f32,
        found: Option<&Found>,
    ) {
        let Some(pane) = self.pane else {
            return;
        };
        let Some(rect) = panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r) else {
            self.close();
            return;
        };
        let now = ctx.input(|i| i.time);
        if let Some(t) = self.typed {
            ctx.request_repaint_after_secs((SETTLE - (now - t)).max(0.0) as f32);
        }
        let t = Tokens::get(ctx);
        let width = WIDTH.min(rect.width() * 0.9);
        let at = pos2(rect.right() - INSET - width, top.max(rect.top()) + 12.0);
        let frame = Frame::new()
            .fill(t.surface)
            .stroke(egui::Stroke::new(1.0, t.line))
            .corner_radius(t.radius_lg)
            .shadow(ctx.global_style().visuals.window_shadow)
            .inner_margin(Margin::same(6));
        let mut close = false;
        let area = egui::Area::new(Id::new("find-bar"))
            .fade_in(false)
            .order(Order::Foreground)
            .fixed_pos(at)
            .constrain_to(rect)
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.set_width(width - 12.0);
                    close = self.row(ui, &t, found, now);
                    if self.list {
                        ui.separator();
                        self.results(ui, &t, found);
                    }
                });
            });
        let bar = area.response.rect;
        self.rect = Some(bar);
        // Presses on chrome, such as the Search buttons, leave it be.
        let on_pane = |p: egui::Pos2| p.y > top && panes.iter().any(|(_, r)| r.contains(p));
        let pressed = ctx.input(|i| {
            let at = i.pointer.press_origin().filter(|_| i.pointer.any_pressed());
            at.is_some_and(|p| !bar.contains(p) && on_pane(p))
        });
        if close || pressed {
            self.close();
        }
    }

    /// The field, the count and the buttons; true when closed.
    fn row(&mut self, ui: &mut Ui, t: &Tokens, found: Option<&Found>, now: f64) -> bool {
        let mut close = false;
        ui.horizontal(|ui| {
            ui.add(Icon::Search.image(16.0).tint(t.muted));
            // Right to left: close, the list, the steps, the count, the field.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = widgets::small_icon_button(ui, Icon::Close, "Close search").clicked();
                let (icon, name) = if self.list {
                    (Icon::ChevronUp, "Hide results")
                } else {
                    (Icon::List, "Show results")
                };
                if widgets::small_icon_button(ui, icon, name).clicked() {
                    self.list = !self.list;
                }
                let next = widgets::small_icon_button(ui, Icon::ChevronDown, "Next match");
                let back = widgets::small_icon_button(ui, Icon::ChevronUp, "Previous match");
                let count = RichText::new(self.count(found))
                    .size(t.text_small)
                    .color(t.muted);
                ui.label(count);
                let before = self.query.clone();
                let field = TextEdit::singleline(&mut self.query)
                    .hint_text(PLACEHOLDER)
                    .desired_width(f32::INFINITY);
                let r = ui.add(field);
                if self.query != before {
                    self.typed = Some(now);
                    self.waiting = None;
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if std::mem::take(&mut self.focus) || enter {
                    r.request_focus();
                }
                let shift = ui.input(|i| i.modifiers.shift);
                if enter || next.clicked() || back.clicked() {
                    let forward = if enter { !shift } else { next.clicked() };
                    self.step(found, forward, now);
                }
            });
        });
        close
    }

    /// "3/57", "57" before a step, "0/0"; "…" while pages are still read.
    fn count(&self, found: Option<&Found>) -> String {
        if self.query.trim().is_empty() {
            return String::new();
        }
        let Some(f) = found else {
            return "…".into();
        };
        let n = f.hits.len();
        let more = if f.more { "+" } else { "" };
        let still = if f.done() { "" } else { "…" };
        match self.active.filter(|&i| i < n) {
            _ if n == 0 && f.done() => "0/0".into(),
            Some(i) => format!("{}/{n}{more}{still}", i + 1),
            None => format!("{n}{more}{still}"),
        }
    }

    /// Every hit with its page and snippet; a click goes to it.
    fn results(&mut self, ui: &mut Ui, t: &Tokens, found: Option<&Found>) {
        let hits = found.map_or(&[][..], |f| f.hits.as_slice());
        if hits.is_empty() {
            let none = if found.is_some_and(Found::done) {
                "No results"
            } else {
                "Searching…"
            };
            ui.label(RichText::new(none).color(t.muted));
            return;
        }
        let mut picked = None;
        // Inside an area the list would shrink to egui's 64 pt floor.
        let spacing = ui.spacing().item_spacing.y;
        let tall = hits.len() as f32 * (ROW_H + spacing) - spacing;
        egui::ScrollArea::vertical()
            .max_height(LIST_MAX_H)
            .min_scrolled_height(tall.min(LIST_MAX_H))
            .auto_shrink([false, true])
            .show_rows(ui, ROW_H, hits.len(), |ui, rows| {
                for i in rows {
                    let Some(hit) = hits.get(i) else {
                        continue;
                    };
                    let on = self.active == Some(i);
                    if result_row(ui, t, (hit.page, &hit.snippet), on).clicked() {
                        picked = Some(i);
                    }
                }
            });
        if let Some(i) = picked {
            self.activate(i);
            self.list = false;
        }
    }
}

/// A page badge over the snippet; the current hit's row is tinted.
fn result_row(ui: &mut Ui, t: &Tokens, (page, snippet): (usize, &str), on: bool) -> egui::Response {
    let size = egui::vec2(ui.available_width(), ROW_H);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let label = format!("Page {}: {snippet}", page + 1);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &label));
    let painter = ui.painter_at(rect);
    if on || r.hovered() {
        let fill = if on { t.accent_soft } else { t.line };
        painter.rect_filled(rect, t.radius, fill);
    }
    let left = rect.left() + 10.0;
    let badge = format!("p. {}", page + 1);
    let small = egui::FontId::proportional(t.text_small - 2.0);
    painter.text(
        pos2(left, rect.top() + 12.0),
        egui::Align2::LEFT_CENTER,
        badge,
        small,
        t.muted,
    );
    let ink = if on { t.ink } else { t.muted };
    let body = egui::FontId::proportional(t.text_small + 1.0);
    let line = ui.fonts_mut(|f| {
        let mut job = egui::text::LayoutJob::simple_singleline(snippet.to_owned(), body, ink);
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 20.0);
        f.layout_job(job)
    });
    painter.galley(pos2(left, rect.top() + 21.0), line, ink);
    r
}
