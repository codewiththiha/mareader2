use super::*;
use crate::command::{Intent, UiCommand};

pub(super) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::Ui(UiCommand::ShowLibrary)
            | Command::Intent(Intent::SetLibraryView(..))
            | Command::Intent(Intent::OpenBook(..))
            | Command::Intent(Intent::PlaceBook { .. })
            | Command::Intent(Intent::Library(..))
            | Command::Intent(Intent::ForgetPositions { .. })
    )
}

pub(super) fn apply(app: &mut MareaderApp, ctx: &Context, command: Command) {
    super::one_stage(app, ctx, command, handles_1, apply_1);
}

fn handles_1(command: &Command) -> bool {
    matches!(
        command,
        Command::Ui(UiCommand::ShowLibrary)
            | Command::Intent(Intent::SetLibraryView(..))
            | Command::Intent(Intent::OpenBook(..))
            | Command::Intent(Intent::PlaceBook { .. })
            | Command::Intent(Intent::Library(..))
            | Command::Intent(Intent::ForgetPositions { .. })
    )
}

fn apply_1(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        Command::Ui(UiCommand::ShowLibrary) => match app.route() {
            Route::Reader | Route::Dictionary => app.go(Route::Library),
            // An empty reader is no place to go back to.
            Route::Library if app.reading() => app.go(Route::Reader),
            Route::Library => {}
        },
        Command::Intent(Intent::SetLibraryView(view)) => {
            if let Ok(json) = serde_json::to_string(&view) {
                app.send(Intent::SetField {
                    path: "library".into(),
                    value: json,
                });
            }
        }
        // A lost book asks where it went instead of failing.
        Command::Intent(Intent::OpenBook(book)) => {
            // Do nothing is for the rail; the page has nothing to drag to.
            let placement = match app.settings().workspace.library_click {
                LibraryClick::Split => Placement::Beside,
                LibraryClick::Replace | LibraryClick::DragOnly => Placement::Into,
            };
            app.open_book(ctx, book, None, placement);
        }
        Command::Intent(Intent::PlaceBook {
            book,
            pane,
            placement,
        }) => app.open_book(ctx, book, pane, placement),
        Command::Intent(Intent::Library(action)) => app.library_action(ctx, action),
        Command::Intent(Intent::ForgetPositions { books, paths }) => {
            let panes: Vec<PaneId> = app
                .tree()
                .panes()
                .into_iter()
                .filter(|&pane| match app.documents.book(pane) {
                    Some(book) => books.contains(&book),
                    None => app
                        .documents
                        .path(pane)
                        .is_some_and(|open| paths.iter().any(|path| path.as_path() == open)),
                })
                .collect();
            for note in app.documents.restart_at_start(ctx, &panes) {
                app.toasts.push(note);
            }
        }

        _ => {}
    }
}
