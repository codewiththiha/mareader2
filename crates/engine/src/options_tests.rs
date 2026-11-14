//! The keys the engine owns, and the ones it leaves to a front end.

use super::*;
use crate::library::{Action, SearchAction};
use crate::session::Session;
use mareader_appearance::{BaseMode, NoiseMode};
use mareader_library::{CoverFit, Home, Layout, SortKey};
use mareader_session::Source;
use mareader_settings::{AppearanceMenuStyle, LibraryClick, Settings};

/// The command a key asks for, or why the question has no answer.
fn parsed(key: &str, value: &str) -> Result<Vec<Command>, String> {
    match parse(key, value) {
        None => Err(format!("{key} is a front end's own")),
        Some(found) => found,
    }
}

/// A session with the key carried out, so a setting can be looked at.
fn written(key: &str, value: &str) -> Result<Session, String> {
    let mut session = Session::new(Settings::default());
    for command in parsed(key, value)? {
        let done = session.apply(command);
        if let Some(why) = done.refused {
            return Err(why);
        }
    }
    Ok(session)
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
    let kept = [
        "save_preset",
        "label",
        "chromeless",
        "menu",
        "reduced",
        // These read the books as they stand, or fill a fixture.
        "select",
        "remove",
        "shelf",
        "books",
        "shelves",
        "copies",
        "runs",
        "organize",
        // These name the front end's own widgets.
        "platform",
        "sidebar",
        "find",
        "workers",
        "pinned",
        "focus",
        "panes",
        "open",
        "toast",
        "toasts",
        "page_ai",
        "page_ai_hold",
        "dictionaries",
        "explain",
        "selection",
    ];
    for key in kept {
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

#[test]
fn a_folder_key_reaches_the_library() -> Result<(), String> {
    for key in ["import", "folder"] {
        let commands = parsed(key, "/home/reader/Books")?;
        let Some(Command::Library(ask)) = commands.first() else {
            return Err(format!("{key} did not ask the library"));
        };
        // Both ask, and neither opens a sheet of its own accord.
        assert!(matches!(
            ask,
            Action::AskFolder(_) | Action::ImportFolder { .. }
        ));
    }
    // A folder read in place is watched, and counts every file it finds.
    let commands = parsed("folder", "/home/reader/Books")?;
    let Some(Command::Library(Action::ImportFolder { opts, .. })) = commands.first() else {
        return Err("folder did not ask for an import".into());
    };
    assert!(opts.watch, "the folder is watched");
    assert_eq!(opts.min_size, 0, "every file counts");
    Ok(())
}

#[test]
fn a_sort_key_writes_the_key_and_its_way() -> Result<(), String> {
    let session = written("sort", "title:desc")?;
    assert_eq!(session.settings().library.sort, SortKey::Title);
    assert!(!session.settings().library.ascending);
    // Without a way, the key's own natural one.
    let session = written("sort", "last_read")?;
    assert_eq!(session.settings().library.sort, SortKey::LastRead);
    assert!(!session.settings().library.ascending);
    let hint = "sort: arranged, last_read, title, author or added";
    assert_eq!(parsed("sort", "colour").err().as_deref(), Some(hint));
    Ok(())
}

#[test]
fn the_shelf_s_view_keys_land() -> Result<(), String> {
    let session = written("layout", "list")?;
    assert_eq!(session.settings().library.layout, Layout::List);
    let session = written("cover", "crop")?;
    assert_eq!(session.settings().library.cover, CoverFit::Crop);
    let session = written("home", "unshelved")?;
    assert_eq!(session.settings().library.home, Home::Unshelved);
    let session = written("columns", "3")?;
    assert_eq!(session.settings().library.columns, Some(3));
    Ok(())
}

#[test]
fn a_sample_is_built_without_a_file_on_disk() -> Result<(), String> {
    let commands = parsed("pdf", "sample:4")?;
    let Some(Command::Open(request)) = commands.first() else {
        return Err("pdf=sample did not ask for an open".into());
    };
    assert_eq!(request.name.as_deref(), Some("sample.pdf"));
    // No pane named, so the open lands in the one in front.
    assert!(request.pane.is_none(), "the pane in front takes it");
    let commands = parsed("epub", "sample")?;
    let Some(Command::Open(request)) = commands.first() else {
        return Err("epub=sample did not ask for an open".into());
    };
    assert_eq!(request.name.as_deref(), Some("sample.epub"));
    Ok(())
}

#[test]
fn a_path_is_opened_as_the_file_it_names() -> Result<(), String> {
    let commands = parsed("pdf", "/home/reader/Books/one.pdf")?;
    let Some(Command::Open(request)) = commands.first() else {
        return Err("pdf did not ask for an open".into());
    };
    assert!(
        matches!(request.source, Source::Path(_)),
        "the path is kept"
    );
    Ok(())
}

#[test]
fn the_workspace_keys_land() -> Result<(), String> {
    let session = written("library_click", "split")?;
    let click = session.settings().workspace.library_click;
    assert_eq!(click, LibraryClick::Split);
    let session = written("independent_textures", "true")?;
    assert!(session.settings().workspace.independent_textures);
    let session = written("shared_base", "true")?;
    assert!(session.settings().workspace.shared_base_mode);
    let hint = "library_click: replace, split or drag_only";
    assert_eq!(
        parsed("library_click", "double").err().as_deref(),
        Some(hint)
    );
    Ok(())
}

#[test]
fn a_search_reaches_the_library_field() -> Result<(), String> {
    let commands = parsed("search", "dune")?;
    let Some(Command::Library(Action::Search(SearchAction::Text(text)))) = commands.first() else {
        return Err("search did not ask the library".into());
    };
    assert_eq!(text, "dune");
    Ok(())
}

#[test]
fn a_meaning_language_is_checked_before_it_is_written() -> Result<(), String> {
    let session = written("meaning", "my")?;
    let language = session.settings().ai.tasks.explain.language.as_deref();
    assert_eq!(language, Some("my"));
    let hint = "meaning: a tag, e.g. my";
    assert_eq!(parsed("meaning", "zz").err().as_deref(), Some(hint));
    Ok(())
}
