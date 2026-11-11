use super::*;

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
            let global = &mut app.settings.appearance;
            app.looks.edit_window(global, routing, a);
        }
        Command::Intent(Intent::SetAppearance(a)) => {
            let routing = app.routing();
            let global = &mut app.settings.appearance;
            app.looks.edit(app.active, global, routing, a);
        }
        Command::Intent(Intent::SetInkContrast(ink)) => {
            app.settings.text.ink_contrast = ink;
            app.settings.text.sanitize();
        }
        Command::Intent(Intent::SetQuickText(q)) => q.apply(&mut app.settings.text),
        Command::Intent(Intent::SetIndependence(i)) => {
            app.settings.workspace.independent_themes = i.themes;
            app.settings.workspace.independent_textures = i.textures;
        }
        Command::Intent(Intent::SavePreset { name, group, look }) => {
            app.settings.save_preset(&name, &group, look);
        }
        Command::Intent(Intent::DeletePreset(id)) => app.settings.delete_preset(&id),
        Command::Intent(Intent::SetTitlebarPinned(p)) => {
            app.titlebar.pinned = p;
            app.settings.chrome.pin(app.route().bar(), p);
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
        let mut s = *s;
        s.sanitize();
        app.titlebar.pinned = s.chrome.pinned(app.route().bar());
        app.settings = s;
    }
}
