use super::*;

use crate::ui_command::UiCommand;
use mareader_engine::command::Command;
use mareader_engine::library::Action as A;
use mareader_engine::library::SearchAction as S;

impl MareaderApp {
    /// Library work; its dialogs need the app's files.
    pub(super) fn library_action(
        &mut self,
        ctx: &Context,
        action: mareader_engine::library::Action,
    ) {
        if library_handles_1(&action) {
            self.library_special_1(ctx, action);
            return;
        }
        if library_handles_2(&action) {
            self.library_special_2(ctx, action);
            return;
        }
        if library_handles_3(&action) {
            self.library_special_3(ctx, action);
            return;
        }
        let mut out = Commands::new();
        self.ui.library.act(action, ctx.input(|i| i.time), &mut out);
        for command in out {
            self.apply(ctx, command);
        }
    }

    fn library_special_1(&mut self, ctx: &Context, action: mareader_engine::library::Action) {
        match action {
            A::Search(S::Focus) => {
                self.ui.titlebar.reveal();
                ui_library::search::focus(ctx);
            }
            A::Search(S::Choose(_) | S::ChooseLit) => {
                ui_library::search::blur(ctx);
                let mut out = Commands::new();
                self.ui.library.act(action, ctx.input(|i| i.time), &mut out);
                for command in out {
                    self.apply(ctx, command);
                }
            }
            A::ShowAdd => egui::Popup::open_id(ctx, egui::Id::new(ui_library::ADD_MENU)),
            A::PickFolder => {
                let ask = |root| Sent::Engine(Command::Library(A::AskFolder(root)));
                self.ui.files.pick_folder(ctx, "Import a folder", ask);
            }
            A::PickSearchFolder(book) => {
                let seek = move |folder| Sent::Engine(Command::Library(A::Seek { book, folder }));
                self.ui.files.pick_folder(ctx, "Search a folder", seek);
            }
            A::Book(mareader_engine::library::BookAction::PickImport) => {
                let to = self.ui.library.at();
                self.pick_import(ctx, None, to);
            }
            A::PickFrom(dir) => {
                let to = self.ui.library.at();
                self.pick_import(ctx, Some(dir), to);
            }
            A::PickInto(to) => self.pick_import(ctx, None, to),
            A::Book(mareader_engine::library::BookAction::Reveal(book)) => {
                let path = self.ui.library.model().book(book).and_then(|b| b.path());
                let shown = path.is_some_and(mareader_engine::reveal::reveal);
                if !shown {
                    let note = "The file manager couldn't be opened.";
                    self.ui.toasts.push(Toast::error(note));
                }
            }
            _ => {}
        }
    }

    fn library_special_2(&mut self, ctx: &Context, action: mareader_engine::library::Action) {
        match action {
            A::Shelf(mareader_engine::library::ShelfAction::Reveal(shelf)) => {
                let dir = self.ui.library.model().shelf_dir(shelf);
                let shown = dir.as_deref().is_some_and(mareader_engine::reveal::reveal);
                if !shown {
                    let note = "The file manager couldn't be opened.";
                    self.ui.toasts.push(Toast::error(note));
                }
            }
            A::PickFile(book) => {
                let Some(format) = self.ui.library.model().book(book).map(|b| b.format) else {
                    return;
                };
                let relink = move |path| Sent::Engine(Command::Library(A::Relink { book, path }));
                self.ui.files.pick_book(ctx, format.extensions(), relink);
            }
            A::Organize(picked) => self.organize(picked, false),
            A::OrganizeAnew(picked) => self.organize(picked, true),
            A::ResumeOrganizing => self.resume_organizing(),
            A::StopOrganizing => {
                self.ui.ai.stop_organizing();
                // What was done is kept; its card offers to carry on.
                self.ui.library.set_paused(self.ui.ai.paused());
                self.ui.library.organizer_end();
            }
            A::DiscardPaused => {
                let ready = self
                    .ui
                    .library
                    .organizer()
                    .paused()
                    .is_some_and(|p| p.ready);
                if self.ui.ai.set_aside() {
                    let said = match ready {
                        true => "Discarded the shelf plan.",
                        false => "Discarded the paused run.",
                    };
                    let undo = Command::Library(A::RestorePaused);
                    self.ui.toasts.push(Toast::info(said).undo(undo));
                }
                self.ui.library.set_paused(None);
            }
            A::RestorePaused => {
                self.ui.ai.bring_back();
                self.ui.library.set_paused(self.ui.ai.paused());
            }
            _ => {}
        }
    }

    fn library_special_3(&mut self, ctx: &Context, action: mareader_engine::library::Action) {
        match action {
            A::ApplyOrganized | A::DiscardOrganized => {
                // Applied, the run is done; a discarded plan can come back.
                let undoable = match action {
                    A::DiscardOrganized => self.ui.ai.set_aside(),
                    _ => {
                        self.ui.ai.forget();
                        false
                    }
                };
                self.ui.library.set_paused(None);
                let mut out = Commands::new();
                self.ui.library.act(action, ctx.input(|i| i.time), &mut out);
                if undoable {
                    let undo = Command::Library(A::RestorePaused);
                    let said = "Discarded the shelf plan.";
                    out.push(Sent::Ui(UiCommand::Notify(Toast::info(said).undo(undo))));
                }
                for command in out {
                    self.apply(ctx, command);
                }
            }
            _ => {}
        }
    }

    /// The last three books opened, rebuilt only as the library changes.
    pub(super) fn recent_books(&mut self) -> Vec<(mareader_library::BookId, String)> {
        let model = self.ui.library.model();
        if self.ui.reopen.0 != model.revision() {
            let books = model.recent().into_iter();
            let opened = books.filter(|b| b.opened.value > 0 && !b.missing());
            let three = opened.take(3).map(|b| (b.id, b.title.value.clone()));
            self.ui.reopen = (model.revision(), three.collect());
        }
        self.ui.reopen.1.clone()
    }

    /// Asks the AI to shelve loose books; a paused run asks first.
    pub(super) fn organize(&mut self, picked: Option<Vec<mareader_library::BookId>>, anew: bool) {
        self.ui.library.close_replace();
        if !anew && self.ui.library.organizer().paused().is_some() {
            return self.ui.library.ask_replace(picked);
        }
        match self.ui.library.ask_organize(picked.as_deref()) {
            Ok(ask) => self.start_organizing(&ask, false),
            Err(why) => self.ui.toasts.push(Toast::info(why)),
        }
    }

    /// Carries on the paused run; one with nothing left is let go.
    pub(super) fn resume_organizing(&mut self) {
        self.ui.library.close_replace();
        match self.ui.library.ask_resume() {
            Ok(ask) => self.start_organizing(&ask, true),
            Err(why) => {
                if !self.ui.library.busy() {
                    self.ui.ai.forget();
                    self.ui.library.set_paused(None);
                }
                self.ui.toasts.push(Toast::info(why));
            }
        }
    }

    /// Runs `ask`; a finished paused run needs no model to show its plan.
    pub(super) fn start_organizing(&mut self, ask: &ui_library::organizing::Ask, resume: bool) {
        let ai = &self.settings().ai;
        let ready = resume
            && self
                .ui
                .library
                .organizer()
                .paused()
                .is_some_and(|p| p.ready);
        let model = ai
            .organize_preset()
            .map(mareader_settings::ModelPreset::title);
        let Some(model) = model.or_else(|| ready.then(|| "plan ready".to_owned())) else {
            let why = "Choose a model to organize with in Settings › AI.";
            return self.ui.toasts.push(Toast::warning(why));
        };
        #[cfg(not(target_arch = "wasm32"))]
        if self.ui.ai.organize(ask, ai.tasks.organize.batch, resume) {
            self.ui
                .library
                .organizing(ask.books.iter().map(|b| b.id), &model);
            // A busy model says so at once.
            self.take_organized();
            return;
        }
        let _ = (ask, model, resume);
        self.ui
            .toasts
            .push(Toast::warning("The AI isn't available here."));
    }

    /// The run's news, into the library; an ended run's steps are read back.
    pub(super) fn take_organized(&mut self) {
        let heard = self.ui.ai.organized();
        use ui_library::organizing::Heard;
        let over = |h: &Heard| !matches!(h, Heard::Progress { .. });
        let empty = |h: &Heard| matches!(h, Heard::Plan(p) if p.placed.is_empty());
        if heard.iter().any(empty) {
            // A plan that moves nothing has nothing to review later.
            self.ui.ai.forget();
            self.ui.library.set_paused(None);
        } else if heard.iter().any(over) {
            self.ui.library.set_paused(self.ui.ai.paused());
        }
        let mut out = Commands::new();
        for heard in heard {
            self.ui.library.heard(heard, &mut out);
        }
        for command in out {
            self.queue(command);
        }
    }

    /// The file dialog for books, landing on `to` (none: the top).
    pub(super) fn pick_import(
        &mut self,
        ctx: &Context,
        from: Option<PathBuf>,
        to: Option<mareader_library::ShelfId>,
    ) {
        let import = move |paths| {
            let action = mareader_engine::library::BookAction::Import { paths, to };
            Sent::Engine(Command::Library(mareader_engine::library::Action::Book(
                action,
            )))
        };
        self.ui.files.pick_books(ctx, from, import);
    }
}

fn library_handles_1(action: &mareader_engine::library::Action) -> bool {
    matches!(
        action,
        A::Search(S::Focus | S::Choose(_) | S::ChooseLit)
            | A::ShowAdd
            | A::PickFolder
            | A::PickSearchFolder(..)
            | A::Book(
                mareader_engine::library::BookAction::PickImport
                    | mareader_engine::library::BookAction::Reveal(_)
            )
            | A::PickFrom(..)
            | A::PickInto(..)
    )
}

fn library_handles_2(action: &mareader_engine::library::Action) -> bool {
    matches!(
        action,
        A::Shelf(mareader_engine::library::ShelfAction::Reveal(_))
            | A::PickFile(..)
            | A::Organize(..)
            | A::OrganizeAnew(..)
            | A::ResumeOrganizing
            | A::StopOrganizing
            | A::DiscardPaused
            | A::RestorePaused
    )
}

fn library_handles_3(action: &mareader_engine::library::Action) -> bool {
    matches!(action, A::ApplyOrganized | A::DiscardOrganized)
}
