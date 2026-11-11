use super::*;

pub(super) fn set(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "set" => Some(app.option_set(key, value)),
        "appearance_menu" => Some(app.option_appearance_menu(key, value)),
        "theme" => Some(app.option_theme(key, value)),
        "preset" => Some(app.option_preset(key, value)),
        "texture" => Some(app.option_texture(key, value)),
        "texture_scale" => Some(app.option_texture_scale(key, value)),
        "blend" => Some(app.option_blend(key, value)),
        "grain" => Some(app.option_grain(key, value)),
        "save_preset" => Some(app.option_save_preset(key, value)),
        "grain_intensity" => Some(app.option_grain_intensity(key, value)),
        "blend_area" => Some(app.option_blend_area(key, value)),
        "chromeless" => Some(app.option_chromeless(key, value)),
        "menu" => Some(app.option_menu(key, value)),
        "animations" => Some(app.option_animations(key, value)),
        "float" => Some(app.option_float(key, value)),
        "glass" => Some(app.option_glass(key, value)),
        "margin" => Some(app.option_margin(key, value)),
        "gap" => Some(app.option_gap(key, value)),
        "text_size" => Some(app.option_text_size(key, value)),
        "indent" => Some(app.option_indent(key, value)),
        "ink" => Some(app.option_ink(key, value)),
        "justify" => Some(app.option_justify(key, value)),
        "paragraph" => Some(app.option_paragraph(key, value)),
        "override_font" => Some(app.option_override_font(key, value)),
        "override_layout" => Some(app.option_override_layout(key, value)),
        "theme_pictures" => Some(app.option_theme_pictures(key, value)),
        "theme_emojis" => Some(app.option_theme_emojis(key, value)),
        "column_width" => Some(app.option_column_width(key, value)),
        "hyphenate" => Some(app.option_hyphenate(key, value)),
        "book_layout" => Some(app.option_book_layout(key, value)),
        "design" => Some(app.option_design(key, value)),
        "column" => Some(app.option_column(key, value)),
        "body_font" => Some(app.option_body_font(key, value)),
        "shadow" => Some(app.option_shadow(key, value)),
        "corners" => Some(app.option_corners(key, value)),
        "pane_gap" => Some(app.option_pane_gap(key, value)),
        "label" => Some(app.option_label(key, value)),
        "reduced" => Some(app.option_reduced(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    // Any setting by its dotted path: `layout.page_indicator:false`.
    fn option_set(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let (path, json) = value.split_once(':').ok_or("set: path:json")?;
        self.settings_mut().set_path(path, json)?;

        Ok(())
    }

    fn option_appearance_menu(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let style = match value.to_ascii_lowercase().as_str() {
            "classic" => Some(AppearanceMenuStyle::Classic),
            "dial_deck" | "dial-deck" => Some(AppearanceMenuStyle::DialDeck),
            _ => None,
        };
        let hint = "appearance_menu: classic or dial_deck";
        self.set_field("chrome.appearance_menu", style.ok_or(hint)?)?;
        Ok(())
    }

    fn option_theme(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let base = BaseMode::ALL
            .into_iter()
            .find(|b| b.label().eq_ignore_ascii_case(value));
        self.set_field(
            "appearance.base",
            base.ok_or("theme: light, dark, dim or black")?,
        )?;
        Ok(())
    }

    fn option_preset(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let p = builtin_presets().into_iter().find(|p| p.id == value);
        let hint = "preset: sepia, green, night, parchment, cinema";
        self.set_field("appearance", p.ok_or(hint)?.appearance)?;
        Ok(())
    }

    fn option_texture(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let mode = TextureMode::ALL.into_iter().find(|m| {
            let id = format!("{m:?}");
            id.eq_ignore_ascii_case(value)
        });
        let hint = "texture: none, paper, lined, grid, dotted, cross";
        self.set_field("appearance.texture", mode.ok_or(hint)?)?;
        Ok(())
    }

    fn option_texture_scale(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let scale: u16 = value.parse().map_err(|_| "texture_scale: 25 to 400")?;
        self.set_field("appearance.texture_scale", scale)?;
        Ok(())
    }

    fn option_blend(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("paper.blend", value == "true")?;
        Ok(())
    }

    fn option_grain(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let some = match value {
            "off" => NoiseMode::Off,
            "static" => NoiseMode::Static,
            "animated" => NoiseMode::Animated,
            _ => return Err("grain: off, static or animated".into()),
        };
        self.set_field("appearance.noise", some)?;
        Ok(())
    }

    fn option_save_preset(&mut self, _key: &str, value: &str) -> Result<(), String> {
        // `Name` or `Name/Section`, saving the look set so far.
        let (name, group) = value.split_once('/').unwrap_or((value, ""));
        let look = self.settings_mut().appearance.clone();
        self.settings_mut().save_preset(name, group, look);

        Ok(())
    }

    fn option_grain_intensity(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let v: f32 = value.parse().map_err(|_| "grain_intensity: 0 to 100")?;
        self.set_field("appearance.noise_intensity", v)?;
        Ok(())
    }

    fn option_blend_area(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let some = match value {
            "whole" => PaperArea::WholePage,
            "edges" => PaperArea::Edges,
            _ => return Err("blend_area: whole or edges".into()),
        };
        self.set_field("paper.area", some)?;
        Ok(())
    }

    fn option_chromeless(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.chromeless = value == "true";
        Ok(())
    }

    fn option_menu(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.menu_shot = value == "true";
        Ok(())
    }

    fn option_animations(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("motion.enabled", value == "true")?;
        Ok(())
    }

    fn option_float(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("chrome.sidebar_overlay", value == "true")?;
        Ok(())
    }

    fn option_glass(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("chrome.glass", value == "true")?;
        Ok(())
    }

    fn option_margin(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field(
            "layout.page_margin",
            value.parse::<f32>().map_err(|_| "margin: points")?,
        )?;
        Ok(())
    }

    fn option_gap(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("layout.no_gap", value == "false")?;
        Ok(())
    }

    fn option_text_size(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field(
            "text.font_size",
            value.parse::<f32>().map_err(|_| "text_size: px")?,
        )?;
        Ok(())
    }

    fn option_indent(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field(
            "text.text_indent",
            value.parse::<f32>().map_err(|_| "indent: ems")?,
        )?;
        Ok(())
    }

    fn option_ink(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field(
            "text.ink_contrast",
            value.parse::<f32>().map_err(|_| "ink: %")?,
        )?;
        Ok(())
    }

    fn option_justify(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("text.justify", value == "true")?;
        Ok(())
    }

    fn option_paragraph(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let gap: f32 = value.parse().map_err(|_| "paragraph: ems")?;
        self.set_field("text.paragraph_margin", gap)?;
        Ok(())
    }

    fn option_override_font(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("text.override_font", value == "true")?;
        Ok(())
    }

    fn option_override_layout(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("text.override_layout", value == "true")?;
        Ok(())
    }

    fn option_theme_pictures(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("text.theme_pictures", value == "true")?;
        Ok(())
    }

    fn option_theme_emojis(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("text.theme_emojis", value == "true")?;
        Ok(())
    }

    fn option_column_width(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let pct: f32 = value.parse().map_err(|_| "column_width: %")?;
        self.set_field("text.column_width_pct", pct)?;
        Ok(())
    }

    fn option_hyphenate(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("text.hyphenation", value == "true")?;
        Ok(())
    }

    fn option_book_layout(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("text.book_layout", value == "true")?;
        Ok(())
    }

    fn option_design(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let some = match value {
            "auto" => BookDesign::Auto,
            "on" => BookDesign::On,
            "off" => BookDesign::Off,
            _ => return Err("design: auto, on or off".into()),
        };
        self.set_field("text.book_design", some)?;
        Ok(())
    }

    fn option_column(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let some = match value {
            "left" => ColumnAlign::Left,
            "center" => ColumnAlign::Center,
            "right" => ColumnAlign::Right,
            _ => return Err("column: left, center or right".into()),
        };
        self.set_field("text.column_align", some)?;
        Ok(())
    }

    // `system:georgia`, `bundled:inter`: the body's face.
    fn option_body_font(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("text.default_font", FontChoice::parse(value))?;
        Ok(())
    }

    fn option_shadow(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.set_field("layout.page_shadow", value == "true")?;
        Ok(())
    }

    fn option_corners(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let some = match value {
            "rounded" => mareader_settings::Corners::Rounded,
            _ => mareader_settings::Corners::Square,
        };
        self.set_field("workspace.look.corners", some)?;
        Ok(())
    }

    fn option_pane_gap(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let gap: f32 = value.parse().map_err(|_| "pane_gap: points")?;
        self.set_field("workspace.look.gap", gap)?;
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
        self.reduced_motion
            .store(value == "true", Ordering::Relaxed);
        Ok(())
    }
}
