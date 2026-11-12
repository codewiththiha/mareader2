use super::*;
use crate::ui_command::UiCommand;
use mareader_engine::command::Command;
use mareader_engine::documents::OpenRequest;

pub(super) fn set(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "platform" => Some(app.option_platform(key, value)),
        "toast" => Some(app.option_toast(key, value)),
        "toasts" => Some(app.option_toasts(key, value)),
        "pinned" => Some(app.option_pinned(key, value)),
        "workers" => Some(app.option_workers(key, value)),
        "pdf" | "epub" => Some(app.option_pdf_epub(key, value)),
        "sidebar" => Some(app.option_sidebar(key, value)),
        "find" => Some(app.option_find(key, value)),
        "open" => Some(app.option_open(key, value)),
        "independent" => Some(app.option_independent(key, value)),
        "independent_text" => Some(app.option_independent_text(key, value)),
        "independent_textures" => Some(app.option_independent_textures(key, value)),
        "shared_base" => Some(app.option_shared_base(key, value)),
        "library_click" => Some(app.option_library_click(key, value)),
        "focus" => Some(app.option_focus(key, value)),
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

    fn option_pinned(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let on = value == "true";
        for bar in Bar::ALL {
            self.settings_mut().chrome.pin(bar, on);
        }

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

    fn option_pdf_epub(&mut self, key: &str, value: &str) -> Result<(), String> {
        let (kind, pages) = value.split_once(':').unwrap_or((value, ""));
        let count = pages.parse().ok();
        let pages = count.unwrap_or(120);
        let request = match (key, kind) {
            ("epub", "sample") => OpenRequest::bytes(
                mareader_epub::sample::epub(count.unwrap_or(3), "A Mareader Book"),
                "sample.epub",
            ),
            (_, "sample") => OpenRequest::bytes(
                mareader_pdf::sample::pdf(pages, "A Mareader Sample"),
                "sample.pdf",
            ),
            (_, "figures") => OpenRequest::bytes(
                mareader_pdf::sample::figures(pages, "A Mareader Sample"),
                "figures.pdf",
            ),
            (_, "scan") => OpenRequest::bytes(
                mareader_pdf::sample::scan(pages, "A Mareader Scan"),
                "scan.pdf",
            ),
            _ => OpenRequest::path(value.into()),
        };
        // Into the pane in front now, so `focus=` aims each file.
        let request = OpenRequest {
            pane: Some(self.active()),
            ..request
        };
        self.queue(Sent::Engine(Command::Open(request)));

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

    fn option_independent(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("workspace.independent_themes", value == "true")?;
        Ok(())
    }

    fn option_independent_text(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("workspace.independent_text", value == "true")?;
        Ok(())
    }

    fn option_independent_textures(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("workspace.independent_textures", value == "true")?;
        Ok(())
    }

    fn option_shared_base(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("workspace.shared_base_mode", value == "true")?;
        Ok(())
    }

    fn option_library_click(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let some = match value {
            "replace" => LibraryClick::Replace,
            "split" => LibraryClick::Split,
            "drag_only" => LibraryClick::DragOnly,
            _ => return Err("library_click: replace, split or drag_only".into()),
        };
        self.set_field("workspace.library_click", some)?;
        Ok(())
    }

    fn option_focus(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let n: u32 = value.parse().map_err(|_| "focus: a pane number")?;
        let pane = self
            .tree()
            .panes()
            .get(n.saturating_sub(1) as usize)
            .copied();
        self.focus(pane.ok_or("focus: no such pane")?);

        Ok(())
    }

    fn option_panes(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let n: usize = value.parse().map_err(|_| "panes: 1 to 4")?;
        let area = Bounds::new(0.0, 0.0, 1600.0, 1000.0);
        while self.tree().len() < n.min(mareader_workspace::MAX_PANES) {
            let last = self.tree().panes().last().copied().unwrap_or(self.active());
            let axis = if self.tree().len() % 2 == 1 {
                SplitAxis::Horizontal
            } else {
                SplitAxis::Vertical
            };
            self.tree_mut()
                .split(last, axis, Side::After, area)
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }
}
