//! What the page asked for: one router for the study tools' actions.

use super::Study;
use crate::ui_command::{Commands, Sent, UiCommand};
use egui::{Context, Rect};
use mareader_engine::command::Command;
use mareader_engine::study::Action;
use mareader_engine::toast::Toast;
use mareader_library::{Library, Mark};

impl Study {
    /// Takes the card's and menu's asks; marks change in the libraries.
    pub fn act(&mut self, ctx: &Context, main: &mut Library, action: Action, out: &mut Commands) {
        let wall = mareader_engine::library::wall_ms();
        match action {
            Action::Remove(ids) => self.act_remove(main, ids, wall, out),
            Action::Restore(ids) => {
                main.set_marks_alive(&ids, true, wall);
                self.loose.set_marks_alive(&ids, true, wall);
            }
            Action::Retry => {
                if let Some(card) = self.card.clone() {
                    self.failed.remove(&card.key);
                    self.ask(ctx, card.key, &card.word, &card.context);
                }
            }
            Action::AskAgain => {
                // The saved answer stays until a new one replaces it.
                if let Some(card) = self.card.clone()
                    && !self.asking.contains_key(&card.key)
                {
                    self.answers.remove(&card.key);
                    self.failed.remove(&card.key);
                    self.ask(ctx, card.key, &card.word, &card.context);
                }
                self.save(main);
            }
            Action::Close => {
                self.card = None;
                self.menu = None;
            }
            Action::SelectSeveral(pane, mark) => self.pick(pane, mark),
            Action::Restyle(id, style) => {
                if !main.set_style(id, style, wall) {
                    self.loose.set_style(id, style, wall);
                }
                // The menu stays, showing the new look.
                if let Some(m) = self.menu.as_mut().filter(|m| m.mark == id) {
                    m.style = Some(style);
                }
            }
            Action::RestyleAll(changes, skipped) => {
                let mut before = Vec::new();
                for (id, style) in changes {
                    let Some(was) = self.mark(main, id).map(Mark::look) else {
                        continue;
                    };
                    if main.set_style(id, style, wall) || self.loose.set_style(id, style, wall) {
                        before.push((id, was));
                    }
                }
                let n = before.len();
                let message = match (n, skipped) {
                    (0, _) => "That look is kept for marks of their own".to_owned(),
                    (1, 0) => "Restyled 1 highlight".to_owned(),
                    (n, 0) => format!("Restyled {n} highlights"),
                    (n, s) => format!("Restyled {n} of {} highlights", n + s),
                };
                let toast = match n {
                    0 => Toast::info(message),
                    _ => Toast::info(message).undo(Command::Study(Action::Unstyle(before))),
                };
                out.push(Sent::Ui(UiCommand::Notify(toast)));
            }
            Action::Unstyle(looks) => {
                for (id, style) in looks {
                    if !main.set_style(id, style, wall) {
                        self.loose.set_style(id, style, wall);
                    }
                }
            }
            Action::PickPage | Action::PickAll => {
                if let Some(p) = &mut self.picking {
                    let more = match action {
                        Action::PickPage => p.page.clone(),
                        _ => p.all.clone(),
                    };
                    p.add(&more);
                }
            }
            Action::DonePicking => self.picking = None,
            Action::WriteNote(id) => {
                if let Some(m) = self.menu.take() {
                    self.open_note(main, m.pane, id, m.rect, true);
                }
            }
            Action::WriteNoteIn(pane, id) => {
                self.open_note(main, pane, id, Rect::NOTHING, true);
            }
            Action::EditNote => {
                if let Some(n) = &mut self.note {
                    n.draft = Some(n.mark.note.value.clone().unwrap_or_default());
                    n.focus = 2;
                }
            }
            Action::SaveNote(id) => self.save_note(main, id, out),
            Action::CancelNote => self.cancel_note(main),
            Action::DeleteNote(id) => {
                self.note = None;
                self.forget_note(main, id, out);
            }
            Action::TypeNote(text, save) => {
                let Some(n) = self.note.as_mut() else {
                    return;
                };
                n.draft = Some(text);
                if save {
                    let id = n.mark.id;
                    self.save_note(main, id, out);
                }
            }
            Action::RestoreNote(id, text) => {
                if !main.set_note(id, Some(text.clone()), wall) {
                    self.loose.set_note(id, Some(text), wall);
                }
            }
        }
    }
}
