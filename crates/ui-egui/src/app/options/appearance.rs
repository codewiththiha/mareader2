use super::*;

pub(super) fn set(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "save_preset" => Some(app.option_save_preset(key, value)),
        "chromeless" => Some(app.option_chromeless(key, value)),
        "menu" => Some(app.option_menu(key, value)),
        "label" => Some(app.option_label(key, value)),
        "reduced" => Some(app.option_reduced(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    fn option_save_preset(&mut self, _key: &str, value: &str) -> Result<(), String> {
        // `Name` or `Name/Section`, saving the look set so far.
        let (name, group) = value.split_once('/').unwrap_or((value, ""));
        let look = self.settings_mut().appearance;
        self.settings_mut().save_preset(name, group, look);

        Ok(())
    }

    fn option_chromeless(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.chromeless = value == "true";
        Ok(())
    }

    fn option_menu(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.menu_shot = value == "true";
        Ok(())
    }

    fn option_label(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let mut l = self.settings_mut().layout.clone();
        l.floating_label = value != "false";
        l.floating_label_persist = value == "persist";
        if value == "chapter" {
            l.floating_label_style = mareader_settings::FloatingLabelStyle::Chapter;
        };
        self.set_field("layout", l)?;

        Ok(())
    }

    fn option_reduced(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui
            .reduced_motion
            .store(value == "true", Ordering::Relaxed);
        Ok(())
    }
}
