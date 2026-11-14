//! The keys the engine owns, and the ones it leaves to a front end.

use super::*;
use crate::session::Session;
use mareader_appearance::{BaseMode, NoiseMode};
use mareader_settings::{AppearanceMenuStyle, Settings};

/// The command a key asks for, or why the question has no answer.
fn parsed(key: &str, value: &str) -> Result<Command, String> {
    match parse(key, value) {
        None => Err(format!("{key} is a front end's own")),
        Some(found) => found,
    }
}

/// A session with the key carried out, so a setting can be looked at.
fn written(key: &str, value: &str) -> Result<Session, String> {
    let mut session = Session::new(Settings::default());
    let done = session.apply(parsed(key, value)?);
    done.refused.map_or(Ok(session), Err)
}

#[test]
fn a_number_reaches_the_setting_it_names() -> Result<(), String> {
    let session = written("text_size", "21")?;
    assert_eq!(session.settings().text.font_size, 21.0);
    let session = written("pane_gap", "12.5")?;
    assert_eq!(session.settings().workspace.look.gap, 12.5);
    Ok(())
}

#[test]
fn a_flag_reaches_the_setting_it_names() -> Result<(), String> {
    let session = written("glass", "true")?;
    assert!(session.settings().chrome.glass);
    // `gap` is the space between pages, so the setting is its opposite.
    let session = written("gap", "false")?;
    assert!(session.settings().layout.no_gap);
    Ok(())
}

#[test]
fn a_choice_is_read_ignoring_case() -> Result<(), String> {
    let session = written("grain", "ANIMATED")?;
    assert_eq!(session.settings().appearance.noise, NoiseMode::Animated);
    let session = written("appearance_menu", "dial-deck")?;
    let style = session.settings().chrome.appearance_menu;
    assert_eq!(style, AppearanceMenuStyle::DialDeck);
    Ok(())
}

#[test]
fn a_theme_and_a_preset_both_land() -> Result<(), String> {
    let session = written("theme", "dim")?;
    assert_eq!(session.settings().appearance.base, BaseMode::Dim);
    let session = written("preset", "sepia")?;
    let look = session.settings().appearance;
    assert_ne!(look, Settings::default().appearance);
    Ok(())
}

#[test]
fn any_setting_by_its_path_is_one_key() -> Result<(), String> {
    let session = written("set", "layout.page_indicator:false")?;
    assert!(!session.settings().layout.page_indicator);
    let missing = parsed("set", "layout.page_indicator");
    assert_eq!(missing.err().as_deref(), Some("set: path:json"));
    Ok(())
}

#[test]
fn a_bad_value_says_what_the_key_takes() -> Result<(), String> {
    let hint = |key: &str, value: &str| parsed(key, value).err();
    assert_eq!(hint("text_size", "big").as_deref(), Some("text_size: px"));
    let grain = "grain: off, static or animated";
    assert_eq!(hint("grain", "loud").as_deref(), Some(grain));
    let theme = "theme: light, dark, dim or black";
    assert_eq!(hint("theme", "mauve").as_deref(), Some(theme));
    let preset = "preset: sepia, green, night, parchment, cinema";
    assert_eq!(hint("preset", "noir").as_deref(), Some(preset));
    Ok(())
}

#[test]
fn a_key_the_front_end_keeps_is_not_here() {
    // These read the settings as they stand, or the window's own state.
    for key in ["save_preset", "label", "chromeless", "menu", "reduced"] {
        assert!(parse(key, "true").is_none(), "{key} moved");
    }
    assert!(parse("no_such_key", "1").is_none());
}

#[test]
fn a_path_the_settings_do_not_have_is_refused() {
    let mut session = Session::new(Settings::default());
    let done = session.apply(Command::SetField {
        path: "no.such.dial".into(),
        value: "1".into(),
    });
    assert!(done.refused.is_some(), "an unknown path is refused");
}
