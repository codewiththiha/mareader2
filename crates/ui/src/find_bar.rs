//! The floating find bar: its shell, until search arrives in M7.

use egui::{Context, Frame, Id, Margin, Order, Rect, RichText, TextEdit, pos2};
use mareader_workspace::PaneId;

use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

pub const PLACEHOLDER: &str = "Search in document…";
pub const LATER: &str = "Search arrives with M7";
const WIDTH: f32 = 560.0;
/// From the pane's right edge, and below the title bar.
const INSET: f32 = 16.0;

/// One bar, on the pane it was opened over.
#[derive(Debug, Default)]
pub struct FindBar {
    pane: Option<PaneId>,
    /// Kept while closed, as mareader resumes its last query.
    query: String,
    focus: bool,
}

impl FindBar {
    pub fn open(&mut self, pane: PaneId) {
        self.pane = Some(pane);
        self.focus = true;
    }

    pub fn close(&mut self) {
        self.pane = None;
    }

    pub fn pane(&self) -> Option<PaneId> {
        self.pane
    }

    pub fn is_open(&self) -> bool {
        self.pane.is_some()
    }

    /// Top right of its pane; closes on a press elsewhere in a pane.
    pub fn show(&mut self, ctx: &Context, panes: &[(PaneId, Rect)], top: f32) {
        let Some(pane) = self.pane else {
            return;
        };
        let Some(rect) = panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r) else {
            self.close();
            return;
        };
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
                    ui.horizontal(|ui| {
                        ui.add(Icon::Search.image(16.0).tint(t.muted));
                        let later = RichText::new(LATER).size(t.text_small).color(t.muted);
                        // Right to left: close, the note, then the field fills.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            close = widgets::small_icon_button(ui, Icon::Close, "Close search")
                                .clicked();
                            ui.label(later);
                            let field = TextEdit::singleline(&mut self.query)
                                .hint_text(PLACEHOLDER)
                                .desired_width(f32::INFINITY);
                            let r = ui.add(field);
                            if std::mem::take(&mut self.focus) {
                                r.request_focus();
                            }
                        });
                    });
                });
            });
        let bar = area.response.rect;
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
}
