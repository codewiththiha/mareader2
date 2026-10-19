//! The page's menu and a bubble's: what to ask, and what to do after.

use egui::{
    Button, Context, Frame, Id, Key, Margin, Modifiers, Order, Rect, RichText, ScrollArea, Sense,
    Stroke, TextEdit, Ui, vec2,
};
use mareader_library::Library;
use mareader_settings::{AiSettings, LANGUAGES, PageTask, language_name};
use mareader_workspace::PaneId;

use crate::command::{Command, Commands};
use crate::icons::Icon;
use crate::settings_window::Tab;
use crate::theme::Tokens;
use crate::widgets;

use super::{Action, BubbleMenu, Menu, PageAi, Slot, label};

/// The language search field, for screen readers and tests.
pub const FIND: &str = "Find a language";

const WIDTH: f32 = 236.0;
/// The language list's height before it scrolls.
const LIST_H: f32 = 260.0;

impl PageAi {
    /// After the panes: the open menu, over them.
    pub fn show(
        &mut self,
        ctx: &Context,
        ai: &AiSettings,
        main: &Library,
        panes: &[(PaneId, Rect)],
        out: &mut Commands,
    ) {
        let pressed = ctx.input(|i| {
            let p = i.pointer.press_origin();
            p.filter(|_| i.pointer.any_pressed())
        });
        let outside = |r: Option<Rect>| pressed.is_some_and(|p| !r.is_some_and(|r| r.contains(p)));
        let gone = |pane: PaneId| !panes.iter().any(|(p, _)| *p == pane);
        if self.menu.is_some_and(|m| gone(m.pane))
            || (self.menu.is_some() && outside(self.menu_rect))
        {
            self.menu = None;
        }
        let bubble = self.bubble_menu;
        if bubble.is_some_and(|m| gone(m.pane)) || (bubble.is_some() && outside(self.bubble_rect)) {
            self.bubble_menu = None;
        }
        let open = self.menu.is_some() || self.bubble_menu.is_some();
        if open && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.escape();
        }
        let t = Tokens::get(ctx);
        self.menu_rect = self.menu.map(|m| self.page_menu(ctx, m, ai, &t, out));
        self.bubble_rect = self
            .bubble_menu
            .map(|m| self.bubble_menu(ctx, m, main, &t, out));
    }

    fn page_menu(
        &mut self,
        ctx: &Context,
        m: Menu,
        ai: &AiSettings,
        t: &Tokens,
        out: &mut Commands,
    ) -> Rect {
        let mut close = false;
        let mut languages = m.languages;
        let rect = popup(ctx, Id::new("page-ai-menu"), m.at, t, |ui| {
            close = match languages {
                true => self.languages(ui, m, ai, &mut languages, out),
                false => self.tasks(ui, m, ai, &mut languages, out),
            };
        });
        match (close, &mut self.menu) {
            (true, _) => self.menu = None,
            (false, Some(menu)) => {
                if languages && !menu.languages {
                    self.filter.clear();
                }
                menu.languages = languages;
            }
            (false, None) => {}
        }
        rect
    }

    /// The tasks; true once one is picked.
    fn tasks(
        &self,
        ui: &mut Ui,
        m: Menu,
        ai: &AiSettings,
        languages: &mut bool,
        out: &mut Commands,
    ) -> bool {
        let t = Tokens::get(ui.ctx());
        if !PageTask::ALL
            .iter()
            .any(|task| ai.page_preset(*task).is_some())
        {
            let why = "Summarize, translate and simplify pages with an AI model.";
            ui.add(egui::Label::new(RichText::new(why).size(t.text_small).color(t.muted)).wrap());
            if widgets::menu_row(ui, Icon::Settings, "Set up AI…", None, false).clicked() {
                out.push(Command::OpenSettings(Tab::Ai));
                return true;
            }
            return false;
        }
        let (pane, page) = (m.pane, m.page);
        let run = |task| Command::PageAi(Action::Run { pane, page, task });
        if widgets::menu_row(ui, Icon::Summarize, "Summarize page", None, false).clicked() {
            out.push(run(PageTask::Summarize));
            return true;
        }
        match ai.tasks.translate.language.as_deref() {
            Some(tag) => {
                let name = language_name(tag).unwrap_or(tag);
                let to = format!("Translate to {name}");
                if widgets::menu_row(ui, Icon::Translate, &to, None, false).clicked() {
                    let tag = tag.to_owned();
                    out.push(Command::PageAi(Action::Translate { pane, page, tag }));
                    return true;
                }
                let other =
                    widgets::menu_row(ui, Icon::ChevronRight, "Other language…", None, false);
                *languages |= other.clicked();
            }
            None => {
                let pick = widgets::menu_row(ui, Icon::Translate, "Translate page…", None, false);
                *languages |= pick.clicked();
            }
        }
        let simplify = widgets::menu_row(ui, Icon::Simplify, "Simplify page", None, false);
        if simplify
            .on_hover_text("Rewrites the page in plain English")
            .clicked()
        {
            out.push(run(PageTask::Simplify));
            return true;
        }
        if self.showing_on(pane, page).is_some() {
            rule(ui, &t);
            if widgets::menu_row(ui, Icon::Book, "Show original page", None, false).clicked() {
                out.push(Command::PageAi(Action::Show {
                    pane,
                    page,
                    slot: None,
                }));
                return true;
            }
        }
        false
    }

    /// Every language, recent ones first; true once one is picked.
    fn languages(
        &mut self,
        ui: &mut Ui,
        m: Menu,
        ai: &AiSettings,
        languages: &mut bool,
        out: &mut Commands,
    ) -> bool {
        let t = Tokens::get(ui.ctx());
        if widgets::menu_row(ui, Icon::ChevronLeft, "Translate to…", None, false).clicked() {
            *languages = false;
            return false;
        }
        let edit = TextEdit::singleline(&mut self.filter)
            .hint_text("Find a language")
            .desired_width(f32::INFINITY);
        let field = ui.add(edit);
        let name = |b: &mut egui::accesskit::Node| b.set_label(FIND);
        ui.ctx().accesskit_node_builder(field.id, name);
        if !field.has_focus() && self.filter.is_empty() {
            field.request_focus();
        }
        let find = self.filter.trim().to_lowercase();
        let matches = |tag: &str, name: &str| {
            find.is_empty()
                || name.to_lowercase().contains(&find)
                || tag.to_lowercase().starts_with(&find)
        };
        let recent: Vec<&str> = match find.is_empty() {
            true => ai
                .tasks
                .translate
                .recent
                .iter()
                .map(String::as_str)
                .collect(),
            false => Vec::new(),
        };
        let all: Vec<(&str, &str)> = LANGUAGES
            .iter()
            .copied()
            .filter(|(tag, name)| matches(tag, name))
            .collect();
        let current = ai.tasks.translate.language.as_deref();
        let mut picked = None;
        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            picked = all.first().map(|(tag, _)| (*tag).to_owned());
        }
        ScrollArea::vertical().max_height(LIST_H).show(ui, |ui| {
            for tag in &recent {
                let Some(name) = language_name(tag) else {
                    continue;
                };
                if language_row(ui, name, current == Some(*tag)).clicked() {
                    picked = Some((*tag).to_owned());
                }
            }
            if !recent.is_empty() {
                rule(ui, &t);
            }
            for (tag, name) in &all {
                if language_row(ui, name, current == Some(*tag)).clicked() {
                    picked = Some((*tag).to_owned());
                }
            }
            if all.is_empty() {
                ui.label(
                    RichText::new("No language matches")
                        .size(t.text_small)
                        .color(t.muted),
                );
            }
        });
        let Some(tag) = picked else {
            return false;
        };
        let (pane, page) = (m.pane, m.page);
        out.push(Command::PageAi(Action::Translate { pane, page, tag }));
        true
    }

    fn bubble_menu(
        &mut self,
        ctx: &Context,
        m: BubbleMenu,
        main: &Library,
        t: &Tokens,
        out: &mut Commands,
    ) -> Rect {
        let (pane, page, slot) = (m.pane, m.page, m.slot);
        let shown = self.showing_on(pane, page) == Some(slot);
        // What it is, who wrote it, and when; or why it failed.
        let (what, by, when, failed, busy) = match slot {
            Slot::Saved(id) => match self.saved(main, id) {
                Some(p) => (
                    label(&p.task),
                    p.by.clone(),
                    Some(crate::export::date(p.added)),
                    None,
                    false,
                ),
                None => {
                    self.bubble_menu = None;
                    return Rect::NOTHING;
                }
            },
            Slot::Run(id) => match self.runs.iter().find(|r| r.id == id) {
                Some(r) => (
                    label(&r.task),
                    r.by.clone(),
                    None,
                    r.failed.clone(),
                    r.failed.is_none(),
                ),
                None => {
                    self.bubble_menu = None;
                    return Rect::NOTHING;
                }
            },
        };
        let mut close = false;
        let rect = popup(ctx, Id::new("page-ai-bubble-menu"), m.at, t, |ui| {
            let mut act = |ui: &mut Ui, icon, text: &str, action| {
                if widgets::menu_row(ui, icon, text, None, false).clicked() {
                    out.push(Command::PageAi(action));
                    close = true;
                }
            };
            if let Some(why) = &failed {
                ui.add(
                    egui::Label::new(RichText::new(why).size(t.text_small).color(t.danger)).wrap(),
                );
                act(
                    ui,
                    Icon::Rescan,
                    "Try again",
                    Action::Redo { pane, page, slot },
                );
                act(ui, Icon::Close, "Dismiss", Action::Delete(slot));
            } else if busy {
                match shown {
                    true => act(
                        ui,
                        Icon::Book,
                        "Show original page",
                        Action::Show {
                            pane,
                            page,
                            slot: None,
                        },
                    ),
                    false => act(
                        ui,
                        Icon::Show,
                        "Show as it comes",
                        Action::Show {
                            pane,
                            page,
                            slot: Some(slot),
                        },
                    ),
                }
                act(ui, Icon::Stop, "Stop", Action::Delete(slot));
            } else {
                match shown {
                    true => act(
                        ui,
                        Icon::Book,
                        "Show original page",
                        Action::Show {
                            pane,
                            page,
                            slot: None,
                        },
                    ),
                    false => act(
                        ui,
                        Icon::Show,
                        "Show this result",
                        Action::Show {
                            pane,
                            page,
                            slot: Some(slot),
                        },
                    ),
                }
                act(ui, Icon::Rescan, "Redo", Action::Redo { pane, page, slot });
                act(ui, Icon::Copy, "Copy text", Action::Copy(slot));
                ui.visuals_mut().override_text_color = Some(t.danger);
                act(ui, Icon::Remove, "Delete", Action::Delete(slot));
                ui.visuals_mut().override_text_color = None;
            }
            rule(ui, t);
            let small = |s: String| RichText::new(s).size(t.text_small).color(t.muted);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(small(what.clone()));
                    if !by.is_empty() {
                        ui.label(small(format!("By {by}")));
                    }
                    if let Some(when) = &when {
                        ui.label(small(format!("Made {when}")));
                    }
                });
            });
            ui.add_space(4.0);
        });
        if close {
            self.bubble_menu = None;
        }
        rect
    }
}

/// A menu's frame at `at`, kept on screen.
fn popup(ctx: &Context, id: Id, at: egui::Pos2, t: &Tokens, add: impl FnOnce(&mut Ui)) -> Rect {
    egui::Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(at)
        .constrain(true)
        .show(ctx, |ui| {
            Frame::menu(ui.style())
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius_lg)
                .inner_margin(Margin::same(4))
                .show(ui, |ui| {
                    ui.set_width(WIDTH);
                    add(ui);
                    widgets::close_on_key(ui);
                });
        })
        .response
        .rect
}

fn language_row(ui: &mut Ui, name: &str, on: bool) -> egui::Response {
    let button = Button::new(name)
        .frame_when_inactive(false)
        .selected(on)
        .min_size(vec2(ui.available_width(), 28.0));
    ui.add(button)
}

fn rule(ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, t.line));
}
