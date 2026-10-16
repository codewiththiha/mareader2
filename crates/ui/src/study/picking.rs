//! Picking several marks at once, to remove them together.

use egui::{Align2, Context, Frame, Id, Margin, Order, Rect, RichText, Stroke, vec2};
use mareader_library::MarkId;
use mareader_workspace::PaneId;

use super::Action;
use crate::command::{Command, Commands};
use crate::theme::Tokens;

/// Below the pane's top, clear of the title bar.
const DOWN: f32 = 56.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Picking {
    pub pane: PaneId,
    pub ids: Vec<MarkId>,
    /// The marks on screen last frame, and every mark of the book.
    pub page: Vec<MarkId>,
    pub all: Vec<MarkId>,
}

impl Picking {
    pub fn new(pane: PaneId, first: MarkId) -> Self {
        Self {
            pane,
            ids: vec![first],
            page: Vec::new(),
            all: Vec::new(),
        }
    }

    pub fn toggle(&mut self, id: MarkId) {
        match self.ids.iter().position(|m| *m == id) {
            Some(i) => {
                self.ids.remove(i);
            }
            None => self.ids.push(id),
        }
    }

    /// Adds `ids` not picked yet, in order.
    pub fn add(&mut self, ids: &[MarkId]) {
        for id in ids {
            if !self.ids.contains(id) {
                self.ids.push(*id);
            }
        }
    }
}

/// The bar over the pane: the count, ways to pick more, Remove, Done.
pub fn bar(ctx: &Context, picking: &Picking, room: Rect, t: &Tokens, out: &mut Commands) {
    let n = picking.ids.len();
    let mut act = |a| out.push(Command::Study(a));
    egui::Area::new(Id::new("study-picking"))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(room.center_top() + vec2(0.0, DOWN))
        .constrain_to(room)
        .show(ctx, |ui| {
            Frame::popup(ui.style())
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius_lg)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{n} selected")).color(t.ink));
                        ui.separator();
                        if ui.button("This page").clicked() {
                            act(Action::PickPage);
                        }
                        if ui.button("All").clicked() {
                            act(Action::PickAll);
                        }
                        let remove = RichText::new(format!("Remove ({n})")).color(t.danger);
                        let button = ui.add_enabled(n > 0, egui::Button::new(remove));
                        if button.clicked() {
                            act(Action::Remove(picking.ids.clone()));
                        }
                        if ui.button("Done").clicked() {
                            act(Action::DonePicking);
                        }
                    });
                });
        });
}
