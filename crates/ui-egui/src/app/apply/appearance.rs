use super::*;
use crate::command::{Intent, UiCommand};

pub(super) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::Intent(Intent::SetAppearance(..))
            | Command::Intent(Intent::SetInkContrast(..))
            | Command::Intent(Intent::SetQuickText(..))
            | Command::Intent(Intent::SetIndependence(..))
            | Command::Intent(Intent::SavePreset { .. })
            | Command::Intent(Intent::DeletePreset(..))
            | Command::Intent(Intent::SetTitlebarPinned(..))
            | Command::Ui(UiCommand::ShowAppearance)
            | Command::Ui(UiCommand::ShowViewMenu)
            | Command::Ui(UiCommand::ShowSettings)
            | Command::Ui(UiCommand::ShowShortcuts)
            | Command::Intent(Intent::OpenSettings(..))
            | Command::Ui(UiCommand::CloseSettings)
            | Command::Intent(Intent::SetSettings(..))
    )
}

pub(super) fn apply(app: &mut MareaderApp, ctx: &Context, command: Command) {
    super::two_stage(app, ctx, command, handles_1, apply_1, handles_2, apply_2);
}

fn handles_1(command: &Command) -> bool {
    matches!(
        command,
        Command::Intent(Intent::SetAppearance(..))
            | Command::Intent(Intent::SetInkContrast(..))
            | Command::Intent(Intent::SetQuickText(..))
            | Command::Intent(Intent::SetIndependence(..))
            | Command::Intent(Intent::SavePreset { .. })
            | Command::Intent(Intent::DeletePreset(..))
            | Command::Intent(Intent::SetTitlebarPinned(..))
            | Command::Ui(UiCommand::ShowAppearance)
            | Command::Ui(UiCommand::ShowViewMenu)
            | Command::Ui(UiCommand::ShowSettings)
            | Command::Ui(UiCommand::ShowShortcuts)
            | Command::Intent(Intent::OpenSettings(..))
            | Command::Ui(UiCommand::CloseSettings)
    )
}

fn apply_1(app: &mut MareaderApp, ctx: &Context, command: Command) {
    match command {
        // The shelf wears the window's look, the crowned pane's if split.
        Command::Intent(Intent::SetAppearance(a)) if app.route() != Route::Reader => {
            let routing = app.routing();
            let mut global = app.settings().appearance;
            app.looks.edit_window(&mut global, routing, a);
            let _ = app.set_field("appearance", global);
        }
        Command::Intent(Intent::SetAppearance(a)) => {
            let routing = app.routing();
            let mut global = app.settings().appearance;
            app.looks.edit(app.active(), &mut global, routing, a);
            let _ = app.set_field("appearance", global);
        }
        Command::Intent(Intent::SetInkContrast(ink)) => {
            let _ = app.set_field("text.ink_contrast", ink);
        }
        Command::Intent(Intent::SetQuickText(q)) => {
            let mut text = app.settings().text.clone();
            q.apply(&mut text);
            let _ = app.set_field("text", text);
        }
        Command::Intent(Intent::SetIndependence(i)) => {
            let _ = app.set_field("workspace.independent_themes", i.themes);
            let _ = app.set_field("workspace.independent_textures", i.textures);
        }
        Command::Intent(Intent::SavePreset { name, group, look }) => {
            let mut settings = app.settings().clone();
            settings.save_preset(&name, &group, look);
            app.set_settings(settings);
        }
        Command::Intent(Intent::DeletePreset(id)) => {
            let mut settings = app.settings().clone();
            settings.delete_preset(&id);
            app.set_settings(settings);
        }
        Command::Intent(Intent::SetTitlebarPinned(p)) => {
            app.titlebar.pinned = p;
            app.send(Intent::SetTitlebarPinned(p));
        }
        Command::Ui(UiCommand::ShowAppearance) => {
            app.titlebar.reveal();
            egui::Popup::open_id(ctx, egui::Id::new(crate::titlebar::APPEARANCE_POPOVER));
        }
        Command::Ui(UiCommand::ShowViewMenu) => {
            app.titlebar.reveal();
            let menu = match app.route() {
                Route::Reader => crate::view_menu::VIEW_MENU,
                Route::Library => crate::library::menu::LIBRARY_VIEW_MENU,
                Route::Dictionary => crate::titlebar::DICTIONARY_MENU,
            };
            egui::Popup::open_id(ctx, egui::Id::new(menu));
        }
        Command::Ui(UiCommand::ShowSettings) => app.open_settings(None),
        Command::Ui(UiCommand::ShowShortcuts) => crate::shortcuts::toggle(ctx),
        Command::Intent(Intent::OpenSettings(tab)) => app.open_settings(Some(tab)),
        Command::Ui(UiCommand::CloseSettings) => app.settings_window.close(),
        _ => {}
    }
}

fn handles_2(command: &Command) -> bool {
    matches!(command, Command::Intent(Intent::SetSettings(..)))
}

fn apply_2(app: &mut MareaderApp, _ctx: &Context, command: Command) {
    if let Command::Intent(Intent::SetSettings(s)) = command {
        let pinned = s.chrome.pinned(app.route().bar());
        app.send(Intent::SetSettings(s));
        app.titlebar.pinned = pinned;
    }
}
