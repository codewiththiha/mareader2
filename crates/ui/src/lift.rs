//! A lifted pane: it follows the pointer, and where it would land lights.

use egui::{
    Align2, Area, Color32, Context, CursorIcon, Frame, Id, Key, LayerId, Margin, Modifiers, Order,
    Rect, RichText, Stroke, StrokeKind, pos2, vec2,
};
use mareader_workspace::{Bounds, LiftTarget, PaneId, lift_target};

use crate::theme::{self, Tokens};

/// What the lifted pane did this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifted {
    Held,
    /// Let go off any target, or Escape: it stays put.
    Cancelled,
    Dropped(LiftTarget),
}

/// Draws the lift over the panes and reads the pointer's release.
pub fn show(ctx: &Context, pane: PaneId, panes: &[(PaneId, Rect)], title: &str) -> Lifted {
    let t = Tokens::get(ctx);
    let boxes: Vec<(PaneId, Bounds)> = panes.iter().map(|(id, r)| (*id, bounds(*r))).collect();
    let pointer = ctx.pointer_latest_pos();
    let target = pointer.and_then(|p| lift_target(pane, (p.x, p.y), &boxes));
    ctx.set_cursor_icon(CursorIcon::Grabbing);
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("lift")));
    // The lifted pane fades where it was.
    if let Some((_, from)) = panes.iter().find(|(id, _)| *id == pane) {
        painter.rect_filled(*from, 0.0, Color32::from_black_alpha(70));
        let stroke = Stroke::new(2.0, t.accent.gamma_multiply(0.6));
        painter.rect_stroke(from.shrink(4.0), 10.0, stroke, StrokeKind::Inside);
    }
    let lit = target.and_then(|target| {
        let (_, rect) = panes.iter().find(|(id, _)| *id == target.pane())?;
        let b = target.predicted(bounds(*rect));
        let lit = Rect::from_min_size(pos2(b.x, b.y), vec2(b.w, b.h)).shrink(6.0);
        painter.rect_filled(lit, 10.0, t.accent.gamma_multiply(0.14));
        painter.rect_stroke(lit, 10.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
        Some((lit, target.describe()))
    });
    if let Some((lit, words)) = lit {
        chip(
            ctx,
            Id::new("lift-target"),
            (lit.center(), Align2::CENTER_CENTER),
            words,
            &t,
        );
    }
    if let Some(p) = pointer {
        let carried = format!("Moving {title}");
        chip(
            ctx,
            Id::new("lift-carried"),
            (p + vec2(14.0, 16.0), Align2::LEFT_TOP),
            &carried,
            &t,
        );
    }
    let escape = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
    let released = !ctx.input(|i| i.pointer.any_down());
    match (escape, released, target) {
        (true, ..) | (false, true, None) => Lifted::Cancelled,
        (false, true, Some(target)) => Lifted::Dropped(target),
        (false, false, _) => Lifted::Held,
    }
}

/// A pill of words, read aloud too; it never takes the pointer.
fn chip(ctx: &Context, id: Id, (at, pivot): (egui::Pos2, Align2), words: &str, t: &Tokens) {
    Area::new(id)
        .order(Order::Tooltip)
        .pivot(pivot)
        .fixed_pos(at)
        .interactable(false)
        .show(ctx, |ui| {
            let frame = Frame::new()
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(14.0)
                .inner_margin(Margin::symmetric(10, 5));
            frame.show(ui, |ui| {
                let text = RichText::new(words).font(theme::medium(12.5)).color(t.ink);
                ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
            });
        });
}

fn bounds(r: Rect) -> Bounds {
    Bounds::new(r.left(), r.top(), r.width(), r.height())
}
