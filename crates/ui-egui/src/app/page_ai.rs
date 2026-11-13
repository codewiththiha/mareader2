use egui::Context;

use super::*;

use crate::page_ai::task_for;
use crate::ui_command::UiCommand;
use mareader_engine::page_ai::Action;
use mareader_settings::{PageStart, PageTask};

impl MareaderApp {
    /// What the page AI's menus asked for.
    pub(super) fn page_ai_act(&mut self, ctx: &Context, action: mareader_engine::page_ai::Action) {
        let now = ctx.input(|i| i.time);
        let mut out = Commands::new();
        let task = match &action {
            Action::Run { task, .. } => task_for(*task, &self.settings().ai),
            Action::Translate { tag, .. } => {
                let mut ai = self.settings().ai.clone();
                ai.tasks.translate.pick(tag);
                let _ = self.set_field("ai", ai);
                Some(mareader_library::AiTask::Translate {
                    language: tag.clone(),
                })
            }
            _ => None,
        };
        if matches!(&action, Action::Run { .. } | Action::Translate { .. }) {
            self.page_ai_run(action, task, now, &mut out);
        } else if matches!(
            &action,
            Action::StopFollow(_) | Action::RunPages { .. } | Action::PagesMenu { .. }
        ) {
            self.page_ai_pages(action);
        } else {
            self.page_ai_result(ctx, action, now, &mut out);
        }
        self.ui.queued.extend(out);
    }

    fn page_ai_run(
        &mut self,
        action: Action,
        task: Option<mareader_library::AiTask>,
        now: f64,
        out: &mut Commands,
    ) {
        match action {
            Action::Run {
                pane, page, how, ..
            }
            | Action::Translate {
                pane, page, how, ..
            } => {
                let Some(task) = task else { return };
                let kind = crate::page_ai::page_task(&task);
                if how.remember {
                    let start = match how.continuous {
                        true => PageStart::Continuous,
                        false => PageStart::Page,
                    };
                    let mut tasks = self.settings().ai.tasks.clone();
                    match kind {
                        PageTask::Translate => tasks.translate.start = start,
                        PageTask::Simplify => tasks.simplify.start = start,
                        PageTask::Summarize => {}
                    }
                    let _ = self.set_field("ai.tasks", tasks);
                }
                let book = self.ui.library.book_in(&self.ui.documents, pane);
                let name = self.ui.documents.title(pane).unwrap_or_default().to_owned();
                self.ui.documents.clear_selection(pane);
                let ai_settings = self.settings().ai.clone();
                let asked = crate::page_ai::Asked {
                    pane,
                    page,
                    book,
                    name: &name,
                    documents: &self.ui.documents,
                    ai: &self.ui.ai,
                    settings: &ai_settings,
                };
                match how.continuous && kind != PageTask::Summarize {
                    true => {
                        let model = self.ui.library.model();
                        self.ui.page_ai.follow(asked, model, task, true, now, out);
                    }
                    false => self.ui.page_ai.run(asked, task, now, out),
                }
            }
            _ => {}
        }
    }

    fn page_ai_pages(&mut self, action: Action) {
        match action {
            Action::StopFollow(pane) => self.ui.page_ai.ledger.stop_follow(pane),
            Action::RunPages {
                pane,
                pages,
                task,
                tag,
            } => {
                let task = match (task, tag) {
                    (PageTask::Translate, Some(tag)) => {
                        let mut ai = self.settings().ai.clone();
                        ai.tasks.translate.pick(&tag);
                        let _ = self.set_field("ai", ai);
                        Some(mareader_library::AiTask::Translate { language: tag })
                    }
                    (task, _) => {
                        let ai = self.settings().ai.clone();
                        task_for(task, &ai)
                    }
                };
                let Some(task) = task else { return };
                let book = self.ui.library.book_in(&self.ui.documents, pane);
                let name = self.ui.documents.title(pane).unwrap_or_default().to_owned();
                let ai_settings = self.settings().ai.clone();
                let asked = crate::page_ai::Asked {
                    pane,
                    page: pages.first().copied().unwrap_or_default(),
                    book,
                    name: &name,
                    documents: &self.ui.documents,
                    ai: &self.ui.ai,
                    settings: &ai_settings,
                };
                self.ui.page_ai.ledger.run_pages(asked, &pages, task);
            }
            Action::PagesMenu { pane, pages, at } => {
                self.ui.page_ai.open_pages_menu(pane, pages, at)
            }
            _ => {}
        }
    }

    fn page_ai_result(&mut self, ctx: &Context, action: Action, now: f64, out: &mut Commands) {
        match action {
            Action::StopPages(pane) => self.ui.page_ai.ledger.stop_groups(pane),
            Action::Redo { pane, page, slot } => {
                let book = self.ui.library.book_in(&self.ui.documents, pane);
                let name = self.ui.documents.title(pane).unwrap_or_default().to_owned();
                let ai_settings = self.settings().ai.clone();
                let asked = crate::page_ai::Asked {
                    pane,
                    page,
                    book,
                    name: &name,
                    documents: &self.ui.documents,
                    ai: &self.ui.ai,
                    settings: &ai_settings,
                };
                let model = self.ui.library.model();
                self.ui.page_ai.again(asked, model, slot, now, out);
            }
            Action::Show { pane, page, slot } => self.ui.page_ai.show_result(pane, page, slot, now),
            Action::Copy(slot) => {
                if let Some(text) = self.ui.page_ai.text_of(self.ui.library.model(), slot) {
                    ctx.copy_text(text);
                    out.push(Sent::Ui(UiCommand::Notify(Toast::info("Copied"))));
                }
            }
            Action::Delete(slot) => self.ui.page_ai.delete(self.ui.library.model_mut(), slot),

            _ => {}
        }
    }
}
