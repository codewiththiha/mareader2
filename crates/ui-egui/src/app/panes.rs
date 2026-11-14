use super::*;
use crate::ui_command::UiCommand;
use mareader_engine::documents::OpenRequest;

impl MareaderApp {
    /// Every pane as the rail's Library panel lists it.
    pub(super) fn tabs(&self) -> Vec<PaneTab> {
        // The library's name tells twins apart; the document's title doesn't.
        let name = |pane, book: Option<mareader_library::BookId>| {
            let named = book.and_then(|id| self.ui.library.model().book(id));
            let title = named.map(|b| b.title.value.as_str());
            let title = title.or_else(|| self.ui.documents.title(pane));
            title.unwrap_or("Empty pane").to_owned()
        };
        let panes = self.tree().panes().into_iter();
        let tab = |pane| {
            let book = self.ui.library.book_in(&self.ui.documents, pane);
            PaneTab {
                pane,
                name: name(pane, book),
                book,
            }
        };
        panes.map(tab).collect()
    }

    /// A book carried from the rail lands where it's let go.
    pub(super) fn drops_from_rail(&self, ui: &egui::Ui, out: &mut Commands) {
        if self.route() != Route::Reader {
            return;
        }
        let floating = self.settings().chrome.sidebar_overlay;
        let rail = (self.ui.sidebar.offset()).filter(|_| floating);
        let edge = rail.map(|o| o + crate::sidebar::WIDTH);
        // Docked, the rail ends where the panes begin.
        let left = self
            .ui
            .pane_rects
            .iter()
            .map(|(_, r)| r.left())
            .reduce(f32::min);
        let docked = left.filter(|_| self.ui.sidebar.is_open() && !floating);
        let landing = library_panel::Landing {
            panes: &self.ui.pane_rects,
            rail: edge.or(docked),
            full: self.tree().len() >= MAX_PANES,
        };
        let empty = |pane| self.ui.documents.title(pane).is_none();
        let name = |pane| {
            self.ui
                .documents
                .title(pane)
                .unwrap_or("this pane")
                .to_owned()
        };
        library_panel::drop_overlay(ui, &landing, empty, name, out);
    }

    /// Opens a library book: shown already, a click focuses its pane.
    pub(super) fn open_book(
        &mut self,
        ctx: &Context,
        id: mareader_library::BookId,
        pane: Option<PaneId>,
        placement: Placement,
    ) {
        // A lost book asks where it went instead of failing.
        if self
            .ui
            .library
            .model()
            .book(id)
            .is_some_and(|b| b.missing())
        {
            self.apply(
                ctx,
                Sent::Engine(Command::Library(
                    mareader_engine::library::Action::FindAgain(id),
                )),
            );
            return;
        }
        let dropped = pane.is_some();
        let panes = self.tree().panes();
        let showing = panes
            .into_iter()
            .find(|&p| self.ui.library.book_in(&self.ui.documents, p) == Some(id));
        if let (false, Some(showing)) = (dropped, showing) {
            self.focus(showing);
            self.go(Route::Reader);
            return;
        }
        // A library book beside an empty pane goes in it, as in mareader.
        let empty = self
            .ui
            .documents
            .title(pane.unwrap_or(self.active()))
            .is_none();
        let placement = match placement {
            Placement::Beside if empty => Placement::Into,
            other => other,
        };
        match self.ui.library.open(id) {
            Ok(request) => {
                let request = OpenRequest {
                    pane,
                    placement,
                    ..request
                };
                self.apply(ctx, Sent::Engine(Command::Open(request)));
            }
            Err(toast) => self.ui.toasts.push(toast),
        }
    }

    pub(super) fn area(ctx: &Context) -> Bounds {
        let r = ctx.content_rect();
        Bounds::new(r.left(), r.top(), r.width(), r.height())
    }

    /// The lifted pane follows the pointer; letting go moves it there.
    pub(super) fn lifted(&mut self, ctx: &Context) {
        let Some(lift) = self.ui.lifting else {
            return;
        };
        let pane = lift.pane;
        if !self.tree().panes().contains(&pane) || self.tree().len() < 2 {
            self.ui.lifting = None;
            return;
        }
        let title = self
            .ui
            .documents
            .title(pane)
            .unwrap_or("this pane")
            .to_owned();
        let target = match crate::lift::show(ctx, &lift, &self.ui.pane_rects, &title) {
            crate::lift::Lifted::Held => return,
            crate::lift::Lifted::Cancelled => None,
            crate::lift::Lifted::Dropped(target) => Some(target),
        };
        self.ui.lifting = None;
        let moved = match target {
            None => return,
            Some(LiftTarget::Swap(other)) => self.tree_mut().swap(pane, other),
            Some(LiftTarget::Dock(other, edge)) => {
                self.tree_mut().move_to(pane, other, edge, Self::area(ctx))
            }
        };
        if moved.is_err() {
            let note = "There's no room for the pane there.";
            self.ui.toasts.push(Toast::info(note));
        }
        ctx.request_repaint();
    }

    /// Drops open under the pointer, returning that pane; the library imports.
    pub(super) fn drops(&self, ctx: &Context, out: &mut Commands) -> Option<PaneId> {
        let desktop = cfg!(not(target_arch = "wasm32"));
        if desktop && self.route() == Route::Library {
            let dropped = ctx.input(|i| i.raw.dropped_files.clone());
            let paths = dropped.iter().map(|f| f.path().to_path_buf());
            let paths: Vec<PathBuf> = paths.collect();
            // Folders ask how they're kept; files are copied in.
            if !paths.is_empty() {
                let to = self.ui.library.file_target.or(self.ui.library.at());
                let import = mareader_engine::library::BookAction::Import { paths, to };
                out.push(Sent::Engine(Command::Library(
                    mareader_engine::library::Action::Book(import),
                )));
            }
            return None;
        }
        let pointer = ctx.pointer_latest_pos();
        let under = pointer
            .and_then(|p| self.ui.pane_rects.iter().find(|(_, r)| r.contains(p)))
            .map_or(self.active(), |(id, _)| *id);
        let (hovering, dropped) = ctx.input(|i| {
            let first = i.raw.dropped_files.first().cloned();
            (!i.raw.hovered_files.is_empty(), first)
        });
        if let Some(file) = dropped {
            self.ui.files.dropped(ctx, &file, under);
        }
        hovering.then_some(under)
    }

    /// Every document pane's counter, progress line and bottom bar.
    pub(super) fn page_bars(&mut self, ctx: &Context, backdrop: &Backdrop, out: &mut Commands) {
        let rects = &self.ui.pane_rects;
        self.ui
            .page_bars
            .retain(|id, _| rects.iter().any(|(p, _)| p == id));
        let pinned = self.settings().chrome.pinned(self.route().bar());
        let label = LabelChrome {
            bar: self
                .ui
                .titlebar
                .visible(pinned)
                .then(|| self.ui.titlebar.band_bottom(ctx)),
        };
        let tokens = theme::Tokens::get(ctx);
        let toasts = self.ui.toasts.covers();
        let rounded = rects.len() > 1 && self.settings().workspace.look.rounded();
        let layout = self.settings().layout.clone();
        let corner = if rounded { tokens.radius_lg } else { 0.0 };
        // The title bar's glass takes slot 0.
        for (i, (id, rect)) in rects.iter().enumerate() {
            if let Some(reading) = self.ui.documents.reading(*id) {
                // Distraction-free keeps the label, which has its own setting.
                if self.ui.chromeless {
                    floating_label::show(ctx, *rect, &reading, &layout, label);
                    continue;
                }
                let look = BarLook {
                    layout: &layout,
                    backdrop,
                    slot: i + 1,
                    corner,
                    toasts,
                };
                let kept =
                    bookmarks::on_pages(&self.ui.study, &self.ui.library, &self.ui.documents, *id);
                let marked = kept.iter().any(|(_, p)| *p == reading.current);
                let bar = self.ui.page_bars.entry(*id).or_default();
                page_bar::show(ctx, *rect, &reading, (&look, marked), bar, out);
                floating_label::show(ctx, *rect, &reading, &layout, label);
            }
        }
    }
}

/// The window's own keys: what reads this window's own state.
///
/// `None` for a key that is not one of them, so the next family is asked.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "platform" => Some(app.option_platform(key, value)),
        "toast" => Some(app.option_toast(key, value)),
        "toasts" => Some(app.option_toasts(key, value)),
        "workers" => Some(app.option_workers(key, value)),
        "sidebar" => Some(app.option_sidebar(key, value)),
        "find" => Some(app.option_find(key, value)),
        "open" => Some(app.option_open(key, value)),
        "panes" => Some(app.option_panes(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    fn option_platform(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.platform = Platform::parse(value).ok_or("platform: mac, windows or gnome")?;

        Ok(())
    }

    fn option_toast(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.toasts.push(shot_toast(value));
        Ok(())
    }

    fn option_toasts(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.toasts.set_open(value == "open");
        Ok(())
    }

    fn option_workers(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let n = value
            .parse()
            .map_err(|_| "workers: a count, 0 runs inline")?;
        self.ui.documents.set_workers(n);
        self.ui.library.set_inline(n == 0);
        self.ui.study.set_inline(n == 0);
        self.ui.page_ai.set_inline(n == 0);
        self.ui.dict.set_inline(n == 0);
        self.ui.sounds.set_inline(n == 0);
        self.ui.search.set_inline(n == 0);

        Ok(())
    }

    fn option_sidebar(&mut self, _key: &str, value: &str) -> Result<(), String> {
        match Panel::parse(value) {
            Some(p) => self.ui.sidebar.open(Some(p)),
            None if value == "closed" => self.ui.sidebar.close(),
            None => return Err("sidebar: thumbnails, outline, library or closed".into()),
        };
        Ok(())
    }

    // The bar's query, landing on its first hit once `open=find`.
    fn option_find(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.find.seed(value);
        Ok(())
    }

    fn option_open(&mut self, _key: &str, value: &str) -> Result<(), String> {
        match value {
            "appearance" => self.queue(Sent::Ui(UiCommand::ShowAppearance)),
            "settings" => self.queue(Sent::Ui(UiCommand::ShowSettings)),
            "shortcuts" => self.queue(Sent::Ui(UiCommand::ShowShortcuts)),
            "menu" => self.queue(Sent::Ui(UiCommand::ShowViewMenu)),
            "find" => self.queue(Sent::Ui(UiCommand::ShowFind)),
            "search" => {
                let focus = mareader_engine::library::SearchAction::Focus;
                self.queue(Sent::Engine(Command::Library(
                    mareader_engine::library::Action::Search(focus),
                )));
            }
            "add" => self.queue(Sent::Engine(Command::Library(
                mareader_engine::library::Action::ShowAdd,
            ))),
            "add_folder" => {
                let root = std::path::PathBuf::from("/home/reader/Books");
                self.queue(Sent::Engine(Command::Library(
                    mareader_engine::library::Action::AskFolder(root),
                )));
            }
            // The removal sheet for what `select` picked.
            "remove" => {
                let books = self.ui.queued.iter().find_map(|c| match c {
                    Sent::Engine(Command::Library(mareader_engine::library::Action::Pick(
                        mareader_library::picks::PickAction::Only(p),
                    ))) => Some(p.iter().filter_map(|p| match p {
                        mareader_library::picks::Pick::Book(id) => Some(*id),
                        _ => None,
                    })),
                    _ => None,
                });
                let books: Vec<_> = books.ok_or("open=remove: select books first")?.collect();
                let (shelves, links) = (Vec::new(), Vec::new());
                let ask = mareader_engine::library::BookAction::AskRemove {
                    books,
                    shelves,
                    links,
                };
                self.queue(Sent::Engine(Command::Library(
                    mareader_engine::library::Action::Book(ask),
                )));
            }
            "rename" => {
                let first = self.ui.library.model().recent().first().map(|b| b.id);
                let first = first.ok_or("open=rename: no book")?;
                let ask = mareader_engine::library::Action::AskRename(
                    mareader_engine::library::Named::Book(first),
                );
                self.queue(Sent::Engine(Command::Library(ask)));
            }
            // The first missing book's find-again sheet.
            "find_again" => {
                let lost = self.ui.library.model().books().find(|b| b.missing());
                let lost = lost.ok_or("open=find_again: no missing book")?.id;
                self.queue(Sent::Engine(Command::Library(
                    mareader_engine::library::Action::FindAgain(lost),
                )));
            }
            _ => {
                return Err(
                    "open: appearance, settings, menu, find, folders, find_again, remove \
                     or rename"
                        .into(),
                );
            }
        };
        Ok(())
    }

    fn option_panes(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let n: usize = value.parse().map_err(|_| "panes: 1 to 4")?;
        let want = n.min(mareader_workspace::MAX_PANES);
        // A shot has no window to ask, so the room is a fixture.
        let area = Bounds::new(0.0, 0.0, 1600.0, 1000.0);
        while self.tree().len() < want {
            let last = self.tree().panes().last().copied().unwrap_or(self.active());
            let edge = if self.tree().len() % 2 == 1 {
                mareader_workspace::Edge::Right
            } else {
                mareader_workspace::Edge::Bottom
            };
            let made = self.session.split_at(last, edge, area);
            if made.is_none() {
                return Err("panes: no room for another".to_owned());
            }
        }

        Ok(())
    }
}

/// A toast from a shot's words: `_` for spaces, a prefix for its tone.
fn shot_toast(value: &str) -> Toast {
    let words = value.replace('_', " ");
    let (kind, message) = words.split_once(':').unwrap_or(("", &words));
    match kind {
        "error" => Toast::error(message),
        "warning" => Toast::warning(message),
        "player" => Toast::info(message).player(),
        "undo" => Toast::info(message).undo(Command::Study(StudyAction::Restore(Vec::new()))),
        "action" => {
            Toast::info(message).action("Resume", Command::Study(StudyAction::Restore(Vec::new())))
        }
        _ => Toast::info(words.as_str()),
    }
}
