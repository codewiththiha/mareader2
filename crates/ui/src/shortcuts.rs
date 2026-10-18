//! The keyboard shortcut cheat sheet: a modal in the middle of the window.

use egui::{
    Align, Color32, Context, CornerRadius, FontId, Frame, Id, Label, Layout, Margin, Modal,
    RichText, ScrollArea, Stroke, TextEdit, Ui, vec2,
};

use crate::command::Command;
use crate::icons::Icon;
use crate::keymap::{self, Entry, Section};
use crate::platform::Platform;
use crate::theme::Tokens;
use crate::widgets;

const OPEN: &str = "shortcut-sheet-open";
const MODAL: &str = "shortcut-sheet";
const FILTER: &str = "shortcut-sheet-filter";
const WIDTH: f32 = 720.0;
/// Narrower than this, the groups stack in one column.
const TWO_UP: f32 = 560.0;
const COLUMN_GAP: f32 = 32.0;
/// A key cap's padding either side of its text.
const CAP_PAD: f32 = 6.0;

/// What the filter holds, and whether it still wants focus.
#[derive(Clone, Default)]
struct Typed {
    text: String,
    focus: bool,
}

pub fn is_open(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(Id::new(OPEN)))
        .unwrap_or(false)
}

/// Opens the sheet, or shuts it while it is up: its key toggles.
pub fn toggle(ctx: &Context) {
    let open = !is_open(ctx);
    ctx.data_mut(|d| {
        d.insert_temp(Id::new(OPEN), open);
        let typed = Typed {
            text: String::new(),
            focus: open,
        };
        d.insert_temp(Id::new(FILTER), typed);
    });
}

fn close(ctx: &Context) {
    ctx.data_mut(|d| {
        d.insert_temp(Id::new(OPEN), false);
        d.remove::<Typed>(Id::new(FILTER));
    });
}

/// Draws the sheet over everything while it is open.
pub fn show(ctx: &Context, platform: Platform) {
    if !is_open(ctx) {
        return;
    }
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let width = WIDTH.min(screen.width() - 48.0).max(280.0);
    let body_height = (screen.height() * 0.8 - 120.0).max(160.0);
    let frame = Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius_lg)
        .inner_margin(Margin::same(20))
        .shadow(ctx.global_style().visuals.window_shadow);
    let backdrop = Color32::from_black_alpha(if t.dark { 110 } else { 70 });
    let id = Id::new(FILTER);
    let mut typed = ctx.data(|d| d.get_temp::<Typed>(id)).unwrap_or_default();
    let mut shut = false;
    let modal = Modal::new(Id::new(MODAL))
        .frame(frame)
        .backdrop_color(backdrop)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 12.0;
            shut = header(ui, platform);
            let field = TextEdit::singleline(&mut typed.text)
                .hint_text("Filter shortcuts")
                .desired_width(f32::INFINITY);
            let field = ui.add(field);
            widgets::name(&field, "Filter shortcuts");
            if std::mem::take(&mut typed.focus) {
                field.request_focus();
            }
            let sections = matching(keymap::sheet(platform), &typed.text);
            let body = ScrollArea::vertical()
                .max_height(body_height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if sections.is_empty() {
                        let none = format!("No shortcut matches “{}”.", typed.text.trim());
                        ui.label(RichText::new(none).color(t.muted));
                    } else {
                        columns(ui, &sections, width >= TWO_UP);
                    }
                });
            widgets::scroll_fades(ui, &body, t.surface);
        });
    if shut || modal.should_close() {
        close(ctx);
    } else {
        ctx.data_mut(|d| d.insert_temp(id, typed));
    }
}

/// The title, its own key as a hint, and a close button.
fn header(ui: &mut Ui, platform: Platform) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.add(Icon::Keyboard.image(18.0).tint(t.ink));
        let title = RichText::new("Keyboard shortcuts").size(t.text + 3.0);
        ui.label(title.color(t.ink).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = "Close keyboard shortcuts";
            let shut = widgets::small_icon_button(ui, Icon::Close, label).clicked();
            if let Some(keys) = keymap::hint(platform, &Command::ShowShortcuts) {
                let hint = format!("{keys} opens this anywhere");
                ui.label(RichText::new(hint).size(t.text_small).color(t.muted));
            }
            shut
        })
        .inner
    })
    .inner
}

/// The groups holding the query; a group named by it stays whole.
fn matching(sections: Vec<Section>, query: &str) -> Vec<Section> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return sections;
    }
    let hit = |e: &Entry| {
        e.label.to_lowercase().contains(&q) || e.keys.iter().any(|k| k.to_lowercase() == q)
    };
    sections
        .into_iter()
        .filter_map(|mut s| {
            if !s.name.to_lowercase().contains(&q) {
                s.entries.retain(hit);
            }
            (!s.entries.is_empty()).then_some(s)
        })
        .collect()
}

/// Two balanced columns, groups kept whole and in order; one when narrow.
fn columns(ui: &mut Ui, sections: &[Section], two: bool) {
    if !two {
        for s in sections {
            section(ui, s);
        }
        return;
    }
    // A heading weighs about two rows.
    let weight = |s: &Section| s.entries.len() + 2;
    let total: usize = sections.iter().map(weight).sum();
    let mut seen = 0;
    let split = sections
        .iter()
        .position(|s| {
            let past = seen * 2 >= total;
            seen += weight(s);
            past
        })
        .unwrap_or(sections.len());
    let (left, right) = sections.split_at(split);
    ui.spacing_mut().item_spacing.x = COLUMN_GAP;
    ui.columns(2, |cols| {
        for (col, part) in cols.iter_mut().zip([left, right]) {
            col.spacing_mut().item_spacing = vec2(8.0, 6.0);
            for s in part {
                section(col, s);
            }
        }
    });
}

fn section(ui: &mut Ui, s: &Section) {
    let t = Tokens::get(ui.ctx());
    let name = RichText::new(s.name.to_uppercase()).size(t.text_small - 1.0);
    ui.label(name.color(t.muted).strong());
    for e in &s.entries {
        row(ui, e);
    }
    ui.add_space(10.0);
}

/// The action on the left, wrapping if long; its keys on the right.
fn row(ui: &mut Ui, e: &Entry) {
    let t = Tokens::get(ui.ctx());
    let font = FontId::proportional(t.text_small);
    let gap = 4.0;
    let caps: f32 = e.keys.iter().map(|k| cap_width(ui, k, &font) + gap).sum();
    ui.horizontal(|ui| {
        let room = (ui.available_width() - caps - 8.0).max(60.0);
        let layout = Layout::left_to_right(Align::Center);
        ui.allocate_ui_with_layout(vec2(room, 20.0), layout, |ui| {
            ui.set_width(room);
            ui.add(Label::new(RichText::new(e.label).color(t.ink)).wrap());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for k in e.keys.iter().rev() {
                keycap(ui, k);
            }
        });
    });
}

fn cap_width(ui: &Ui, keys: &str, font: &FontId) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(keys.to_owned(), font.clone(), Color32::WHITE);
    galley.size().x + 2.0 * CAP_PAD + 2.0
}

/// One shortcut as a key cap, a heavier edge along its foot.
fn keycap(ui: &mut Ui, keys: &str) {
    let t = Tokens::get(ui.ctx());
    let cap = Frame::new()
        .fill(t.paper)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(CAP_PAD as i8, 2))
        .show(ui, |ui| {
            let text = RichText::new(keys).size(t.text_small).color(t.ink);
            ui.add(Label::new(text).extend());
        });
    let r = cap.response.rect;
    let foot = Stroke::new(1.5, t.line);
    ui.painter()
        .hline(r.x_range().shrink(3.0), r.bottom() - 0.75, foot);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_keeps_matching_actions_and_named_groups() {
        let all = keymap::sheet(Platform::Gnome);
        let count = |s: &[Section]| s.iter().map(|s| s.entries.len()).sum::<usize>();
        let full = count(&all);
        let panes = matching(keymap::sheet(Platform::Gnome), "pane");
        assert!(panes.iter().all(|s| s.name == "Panes"), "only pane actions");
        let find = matching(keymap::sheet(Platform::Gnome), "find");
        assert!(
            find.iter()
                .any(|s| s.name == "Find" && s.entries.len() == 3)
        );
        let f11 = matching(keymap::sheet(Platform::Gnome), "f11");
        assert_eq!(count(&f11), 1, "a key finds its action");
        assert_eq!(count(&matching(all, "  ")), full);
    }
}
