use super::navigation::found_in;
use super::*;
use crate::ui_command::UiCommand;
use mareader_engine::event::Event;
use mareader_engine::session::Session;

mod desk;

impl MareaderApp {
    /// An intent goes to the engine; chrome is carried out here.
    pub(super) fn apply(&mut self, ctx: &Context, command: Command) {
        if !self.route().allows(&command) {
            return;
        }
        // Text edits must reach the pane focused when they were made.
        self.reconcile_texts();
        match command {
            Command::Intent(intent) => {
                let now = ctx.input(|i| i.time);
                Session::execute(&mut desk::Front { app: self, ctx }, intent, now);
                self.drain(ctx);
            }
            Command::Ui(ui) => self.chrome(ctx, ui),
        }
    }

    /// Shows what the session reported since it was last asked.
    pub(super) fn drain(&mut self, ctx: &Context) {
        for event in self.session.drain() {
            match event {
                Event::Toast(toast) => self.ui.toasts.push(toast),
                Event::FocusEdgeTab => {
                    ctx.memory_mut(|m| m.request_focus(egui::Id::new(desk::EDGE_TAB)));
                    crate::focus::by_key(ctx);
                }
                Event::ReleaseFocus => {
                    ctx.memory_mut(|m| m.focused().map(|id| m.surrender_focus(id)));
                }
            }
        }
    }

    fn chrome(&mut self, ctx: &Context, ui: UiCommand) {
        match ui {
            UiCommand::ToggleSidebar => self.ui.sidebar.toggle(),
            UiCommand::OpenSidebar(panel) => self.ui.sidebar.open(panel),
            UiCommand::CloseSidebar => self.ui.sidebar.close(),
            UiCommand::ShowOpenDialog(placement) => self.ui.files.pick(ctx, placement),
            UiCommand::Chromeless => self.toggle_chromeless(),
            UiCommand::Window(w) => ctx.send_viewport_cmd(w.viewport(ctx)),
            UiCommand::Notify(toast) => self.ui.toasts.push(toast),
            UiCommand::CloseToast(id) => self.ui.toasts.close(id, ctx.input(|i| i.time)),
            UiCommand::ClearToasts => self.ui.toasts.clear(ctx.input(|i| i.time)),
            other => self.reveal(ctx, other),
        }
    }

    /// The menus, windows and pages a command brings up.
    fn reveal(&mut self, ctx: &Context, ui: UiCommand) {
        match ui {
            UiCommand::ShowAppearance => {
                self.ui.titlebar.reveal();
                egui::Popup::open_id(ctx, egui::Id::new(crate::titlebar::APPEARANCE_POPOVER));
            }
            UiCommand::ShowViewMenu => self.show_view_menu(ctx),
            UiCommand::ShowSettings => self.open_settings(None),
            UiCommand::ShowShortcuts => crate::shortcuts::toggle(ctx),
            UiCommand::CloseSettings => self.ui.settings_window.close(),
            UiCommand::FindStep { forward } => self.find_step(ctx, forward),
            UiCommand::ShowFind => self.show_find(ctx),
            UiCommand::ShowSearch if self.ui.find.is_open() => self.ui.find.close(),
            UiCommand::ShowSearch => self.show_find(ctx),
            UiCommand::ShowDictionary => self.show_dictionary(),
            UiCommand::ShowReading => self.show_reading(ctx),
            UiCommand::ShowLibrary => self.show_library(),
            _ => {}
        }
    }

    fn toggle_chromeless(&mut self) {
        self.ui.chromeless = !self.ui.chromeless;
        if self.ui.chromeless {
            self.ui.sidebar.close();
            let keys = keymap::hint(self.ui.platform, &Command::Ui(UiCommand::Chromeless));
            let keys = keys.map_or(String::new(), |k| format!(" or {k}"));
            let how = format!("Distraction-free. Press Esc{keys} to leave.");
            self.ui.toasts.push(Toast::info(how));
        }
    }

    fn show_view_menu(&mut self, ctx: &Context) {
        self.ui.titlebar.reveal();
        let menu = match self.route() {
            Route::Reader => crate::view_menu::VIEW_MENU,
            Route::Library => crate::library::menu::LIBRARY_VIEW_MENU,
            Route::Dictionary => crate::titlebar::DICTIONARY_MENU,
        };
        egui::Popup::open_id(ctx, egui::Id::new(menu));
    }

    /// F3 and Cmd+G: the next match, opening the bar if it is shut.
    fn find_step(&mut self, ctx: &Context, forward: bool) {
        if !self.ui.find.is_open() {
            self.show_find(ctx);
            return;
        }
        let found = found_in(&self.ui.search, &self.ui.documents, &self.ui.find);
        self.ui.find.key_step(found, forward, ctx.input(|i| i.time));
    }

    /// With nothing to search in, find searches the library.
    fn show_find(&mut self, ctx: &Context) {
        match (self.route(), self.ui.documents.reading(self.active())) {
            // The page's own field is its find.
            (Route::Dictionary, _) => self.ui.dict.arrive(),
            (Route::Reader, Some(_)) => {
                self.ui.find.open(self.active());
                self.ui.titlebar.reveal();
            }
            _ => {
                self.go(Route::Library);
                let focus = ui_library::Action::Search(ui_library::SearchAction::Focus);
                self.apply(ctx, Command::Intent(Intent::Library(focus)));
            }
        }
    }

    fn show_dictionary(&mut self) {
        match self.route() {
            Route::Dictionary => self.go(self.dict_back()),
            // Over the page, unless Settings ask for the page.
            Route::Reader if self.settings().dictionary.opens == DictionaryOpens::Window => {
                self.dict_window(crate::dict::Window::Toggle);
            }
            _ => self.go(Route::Dictionary),
        }
    }

    /// Back to the pane reading aloud, turned to the words read.
    fn show_reading(&mut self, ctx: &Context) {
        if let Some((pane, page, _)) = self.ui.read_aloud.spot() {
            if self.route() != Route::Reader {
                self.go(Route::Reader);
            }
            self.focus(pane);
            self.ui.documents.go_to(pane, page);
        } else if let Some(origin) = self.ui.read_aloud.away().cloned() {
            self.reopen_reading(ctx, origin);
        }
    }

    fn show_library(&mut self) {
        match self.route() {
            Route::Reader | Route::Dictionary => self.go(Route::Library),
            // An empty reader is no place to go back to.
            Route::Library if self.reading() => self.go(Route::Reader),
            Route::Library => {}
        }
    }
}
