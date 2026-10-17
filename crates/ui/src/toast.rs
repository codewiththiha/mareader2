//! Toasts: one at a time, bottom centre, gone after a few seconds.

use egui::{
    Align, Align2, Context, Frame, Id, Layout, Margin, Order, Rect, RichText, Stroke, pos2,
};

use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::{motion, widgets};

/// How long a toast stays up.
pub const SHOW_FOR: f64 = 3.5;
/// Time left after the pointer leaves a hovered toast.
pub const HOVER_GRACE: f64 = 1.5;
pub const FADE: f32 = 0.2;
const MAX_WIDTH: f32 = 480.0;
// Clears the page bar and the library's selection bar beneath.
const MARGIN: f32 = 68.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// A result or a note: a check mark.
    Info,
    /// Something failed: an alert in the danger colour.
    Error,
    /// It worked, but not wholly: an alert in the accent colour.
    Warning,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub message: String,
    pub tone: Tone,
    /// An Undo button, and what it sends; it keeps the toast up longer.
    pub undo: Option<Box<Command>>,
}

impl Toast {
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            tone: Tone::Info,
            undo: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            tone: Tone::Error,
            undo: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            tone: Tone::Warning,
            undo: None,
        }
    }

    /// With an Undo button that sends `command`.
    pub fn undo(self, command: Command) -> Self {
        Self {
            undo: Some(Box::new(command)),
            ..self
        }
    }

    fn lasts(&self) -> f64 {
        match self.undo {
            Some(_) => crate::study::UNDO_FOR,
            None => SHOW_FOR,
        }
    }
}

/// Clocks start on the first frame shown, so startup toasts get full time.
struct Shown {
    id: u64,
    toast: Toast,
    born: Option<f64>,
    until: Option<f64>,
}

/// The toast slot. A new toast replaces the current one at once.
#[derive(Default)]
pub struct Toasts {
    shown: Option<Shown>,
    next: u64,
}

impl Toasts {
    pub fn push(&mut self, toast: Toast) {
        self.next += 1;
        self.shown = Some(Shown {
            id: self.next,
            toast,
            born: None,
            until: None,
        });
    }

    /// Starts the fade out; the toast leaves when it ends.
    pub fn dismiss(&mut self, now: f64) {
        if let Some(s) = &mut self.shown {
            s.until = Some(s.until.map_or(now, |u| u.min(now)));
        }
    }

    pub fn current(&self) -> Option<&Toast> {
        self.shown.as_ref().map(|s| &s.toast)
    }

    /// Centred along the bottom of `area`, above every other layer.
    pub fn show(&mut self, ctx: &Context, area: Rect, out: &mut Commands) {
        let Some(s) = &mut self.shown else {
            return;
        };
        let now = ctx.input(|i| i.time);
        let born = *s.born.get_or_insert(now);
        let until = *s.until.get_or_insert(now + s.toast.lasts());
        let fade = f64::from(motion::secs(ctx, FADE));
        let opacity = if fade > 0.0 {
            ((now - born) / fade)
                .min((until + fade - now) / fade)
                .clamp(0.0, 1.0)
        } else if now < until {
            1.0
        } else {
            0.0
        };
        if now >= until && opacity <= 0.0 {
            self.shown = None;
            return;
        }
        let live = now < until;
        if opacity < 1.0 {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after_secs((until - now) as f32);
        }
        let rise = (1.0 - opacity as f32) * 8.0;
        let anchor = pos2(area.center().x, area.bottom() - MARGIN + rise);
        let response = egui::Area::new(Id::new(("toast", s.id)))
            .fade_in(false)
            .order(Order::Tooltip)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(anchor)
            .interactable(live)
            .show(ctx, |ui| {
                ui.set_opacity(opacity as f32);
                panel(ui, &s.toast, area.width(), out);
            })
            .response;
        // Reading a toast keeps it up.
        if live && response.contains_pointer() {
            s.until = Some(until.max(now + HOVER_GRACE));
        }
    }
}

fn panel(ui: &mut egui::Ui, toast: &Toast, room: f32, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let (icon, tint, edge) = match toast.tone {
        Tone::Info => (Icon::Done, t.accent, t.line),
        Tone::Error => (Icon::Alert, t.danger, t.danger.gamma_multiply(0.5)),
        Tone::Warning => (Icon::Alert, t.accent, t.accent.gamma_multiply(0.5)),
    };
    let width = MAX_WIDTH.min(room * 0.9);
    Frame::popup(ui.style())
        .fill(t.surface)
        .stroke(Stroke::new(1.0, edge))
        .inner_margin(Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_max_width(width);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = t.gap;
                let (rect, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(16.0), egui::Sense::hover());
                icon.paint(ui, rect, tint);
                let undo = if toast.undo.is_some() {
                    56.0 + t.gap
                } else {
                    0.0
                };
                let text = width - 16.0 - 24.0 - 2.0 * t.gap - undo;
                ui.scope(|ui| {
                    ui.set_max_width(text);
                    ui.add(egui::Label::new(RichText::new(&toast.message).color(t.ink)).wrap());
                });
                if let Some(command) = &toast.undo
                    && ui.button(RichText::new("Undo").color(t.accent)).clicked()
                {
                    out.push((**command).clone());
                }
                let close = widgets::small_icon_button(ui, Icon::Close, "Dismiss");
                if close.clicked() {
                    out.push(Command::DismissToast);
                }
            });
        });
}
