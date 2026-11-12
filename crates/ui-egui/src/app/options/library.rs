use super::*;
use crate::ui_command::UiCommand;
use mareader_engine::command::Command;

pub(super) fn set(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "library" => Some(app.option_library(key, value)),
        "select" => Some(app.option_select(key, value)),
        "remove" => Some(app.option_remove(key, value)),
        "import" => Some(app.option_import(key, value)),
        "folder" => Some(app.option_folder(key, value)),
        "layout" => Some(app.option_layout(key, value)),
        "columns" => Some(app.option_columns(key, value)),
        "cover" => Some(app.option_cover(key, value)),
        "sort" => Some(app.option_sort(key, value)),
        "books" => Some(app.option_books(key, value)),
        "shelves" => Some(app.option_shelves(key, value)),
        "copies" => Some(app.option_copies(key, value)),
        "organize" => Some(app.option_organize(key, value)),
        "shelf" => Some(app.option_shelf(key, value)),
        "search" => Some(app.option_search(key, value)),
        "home" => Some(app.option_home(key, value)),
        "runs" => Some(app.option_runs(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    fn option_library(&mut self, _key: &str, value: &str) -> Result<(), String> {
        match value {
            // After a queued open, which would otherwise leave it.
            "show"
                if self
                    .ui
                    .queued
                    .iter()
                    .any(|c| matches!(c, Sent::Engine(Command::Open(_)))) =>
            {
                self.queue(Sent::Ui(UiCommand::ShowLibrary));
            }
            "show" => self.go(Route::Library),
            _ => self.go(Route::Reader),
        };
        Ok(())
    }

    // The first books by last read, picked.
    fn option_select(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let n: usize = value.parse().map_err(|_| "select: a count of books")?;
        let recent = self.ui.library.model().recent().into_iter().take(n);
        let picks = recent
            .map(|b| mareader_library::picks::Pick::Book(b.id))
            .collect();
        let pick = mareader_engine::library::Action::Pick(
            mareader_library::picks::PickAction::Only(picks),
        );
        self.queue(Sent::Engine(Command::Library(pick)));

        Ok(())
    }

    // Asks to remove the first books by last read, or a shelf by name.
    fn option_remove(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let model = self.ui.library.model();
        let (books, shelves) = match value.strip_prefix("shelf:") {
            Some(name) => {
                let found = model.shelves().find(|s| s.name.value == name);
                let id = found.map(|s| s.id).ok_or("remove: no shelf by that name")?;
                (Vec::new(), vec![id])
            }
            None => {
                let n: usize = value.parse().map_err(|_| "remove: a count or shelf:Name")?;
                let recent = model.recent().into_iter().take(n);
                (recent.map(|b| b.id).collect(), Vec::new())
            }
        };
        let links = Vec::new();
        let ask = mareader_engine::library::BookAction::AskRemove {
            books,
            shelves,
            links,
        };
        self.queue(Sent::Engine(Command::Library(
            mareader_engine::library::Action::Book(ask),
        )));

        Ok(())
    }

    // The import sheet for a folder, as Add ▸ Folder… asks.
    fn option_import(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let ask = mareader_engine::library::Action::AskFolder(std::path::PathBuf::from(value));
        self.queue(Sent::Engine(Command::Library(ask)));

        Ok(())
    }

    // A folder read in place, as the sheet can add one.
    fn option_folder(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let root = std::path::PathBuf::from(value);
        // Watched, every file counted, so its books show as found.
        let opts = mareader_library::Options {
            watch: true,
            min_size: 0,
            ..mareader_library::Options::default()
        };
        let import = mareader_engine::library::Action::ImportFolder { root, opts };
        self.queue(Sent::Engine(Command::Library(import)));

        Ok(())
    }

    fn option_layout(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let some = match value {
            "list" => mareader_library::Layout::List,
            _ => mareader_library::Layout::Grid,
        };
        self.set_field("library.layout", some)?;
        Ok(())
    }

    fn option_columns(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("library.columns", value.parse::<usize>().ok())?;
        Ok(())
    }

    fn option_cover(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let some = match value {
            "crop" => mareader_library::CoverFit::Crop,
            _ => mareader_library::CoverFit::Fit,
        };
        self.set_field("library.cover", some)?;
        Ok(())
    }

    fn option_sort(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let (key, way) = value.split_once(':').unwrap_or((value, ""));
        let key = mareader_library::SortKey::ALL
            .into_iter()
            .find(|k| format!("{k:?}").eq_ignore_ascii_case(&key.replace('_', "")))
            .ok_or("sort: arranged, last_read, title, author or added")?;
        self.set_field("library.sort", key)?;
        let some = match way {
            "asc" => true,
            "desc" => false,
            _ => key.natural_ascending(),
        };
        self.set_field("library.ascending", some)?;
        Ok(())
    }

    fn option_books(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui
            .library
            .seed(value.parse().map_err(|_| "books: a count")?);
        Ok(())
    }

    fn option_shelves(&mut self, _key: &str, _value: &str) -> Result<(), String> {
        self.ui.library.seed_shelves();
        Ok(())
    }

    fn option_copies(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui
            .library
            .seed_copies(value.parse().map_err(|_| "copies: a count")?);
        Ok(())
    }

    // A made-up AI run over the loose copies, or its plan.
    fn option_organize(&mut self, _key: &str, value: &str) -> Result<(), String> {
        match value {
            "running" => self.ui.library.organize_demo(false),
            "review" => self.ui.library.organize_demo(true),
            "paused" => self.ui.library.organize_paused_demo(false),
            "ready" => self.ui.library.organize_paused_demo(true),
            _ => return Err("organize: running, review, paused or ready".into()),
        };
        Ok(())
    }

    // A shelf by its names from the top, as "Fiction/Classics".
    fn option_shelf(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let found = self.ui.library.open_shelf(value);
        found.then_some(()).ok_or("shelf: no shelf by that path")?;

        Ok(())
    }

    // Typed into the library's search field.
    fn option_search(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let text = mareader_engine::library::SearchAction::Text(value.to_owned());
        self.queue(Sent::Engine(Command::Library(
            mareader_engine::library::Action::Search(text),
        )));

        Ok(())
    }

    fn option_home(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let some = match value {
            "unshelved" => mareader_library::Home::Unshelved,
            _ => mareader_library::Home::Everything,
        };
        self.set_field("library.home", some)?;
        Ok(())
    }

    // Two imports under way, their cards shown at once.
    fn option_runs(&mut self, _key: &str, _value: &str) -> Result<(), String> {
        self.ui.library.runs.demo();
        Ok(())
    }
}
