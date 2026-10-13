//! Keyboard shortcuts. One table drives the handler, tooltips and reference.

use egui::{Context, Key, KeyboardShortcut, ModifierNames, Modifiers};

use crate::command::{Command, Commands, WindowCommand};
use crate::platform::Platform;
use crate::zoom::{Fit, Step, ZoomCommand};

pub struct Binding {
    pub label: &'static str,
    pub shortcut: KeyboardShortcut,
    pub command: Command,
}

/// Every bound shortcut; features add theirs here as they land.
pub fn bindings(platform: Platform) -> [Binding; 10] {
    // Each desktop's own full-screen key.
    let fullscreen = match platform {
        Platform::Mac => KeyboardShortcut::new(Modifiers::CTRL | Modifiers::MAC_CMD, Key::F),
        Platform::Windows | Platform::Gnome => KeyboardShortcut::new(Modifiers::NONE, Key::F11),
    };
    [
        Binding {
            label: "Open a file",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::O),
            command: Command::ShowOpenDialog { beside: false },
        },
        zoom("Zoom in", Modifiers::COMMAND, Key::Plus, ZOOM_IN),
        zoom("Zoom in", Modifiers::COMMAND, Key::Equals, ZOOM_IN),
        zoom("Zoom out", Modifiers::COMMAND, Key::Minus, ZOOM_OUT),
        zoom("Fit width", Modifiers::COMMAND, Key::Num0, FIT_WIDTH),
        // mareader's bare keys too, while nothing takes typing.
        zoom("Zoom in", Modifiers::NONE, Key::Plus, ZOOM_IN),
        zoom("Zoom in", Modifiers::NONE, Key::Equals, ZOOM_IN),
        zoom("Zoom out", Modifiers::NONE, Key::Minus, ZOOM_OUT),
        Binding {
            label: "Settings",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma),
            command: Command::ShowSettings,
        },
        Binding {
            label: "Full screen",
            shortcut: fullscreen,
            command: Command::Window(WindowCommand::ToggleFullscreen),
        },
    ]
}

const ZOOM_IN: ZoomCommand = ZoomCommand::Step(Step::In);
const ZOOM_OUT: ZoomCommand = ZoomCommand::Step(Step::Out);
const FIT_WIDTH: ZoomCommand = ZoomCommand::Fit(Fit::Width);

fn zoom(label: &'static str, modifiers: Modifiers, key: Key, z: ZoomCommand) -> Binding {
    Binding {
        label,
        shortcut: KeyboardShortcut::new(modifiers, key),
        command: Command::Zoom(z),
    }
}

/// Bare keys that a focused text field would rather type.
fn types(shortcut: &KeyboardShortcut) -> bool {
    shortcut.modifiers.is_none()
        && matches!(shortcut.logical_key, Key::Plus | Key::Equals | Key::Minus)
}

/// Keys handled elsewhere, listed so the reference is complete.
const LISTED: [(&str, &str); 1] = [("Close a popover, window or sidebar", "Esc")];

/// Consumes this frame's shortcuts and queues their commands.
pub fn read(ctx: &Context, platform: Platform, out: &mut Commands) {
    let typing = ctx.egui_wants_keyboard_input();
    for b in bindings(platform) {
        if typing && types(&b.shortcut) {
            continue;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&b.shortcut)) {
            out.push(b.command);
        }
    }
}

/// Modifier words with key symbols: `Ctrl+,`, not `Ctrl+Comma`.
const WORDS: ModifierNames<'static> = ModifierNames {
    is_short: true,
    concat: "+",
    ..ModifierNames::NAMES
};

/// `⌘,` on macOS, `Ctrl+,` elsewhere.
pub fn text(platform: Platform, shortcut: &KeyboardShortcut) -> String {
    match platform {
        Platform::Mac => shortcut.format(&ModifierNames::SYMBOLS, true),
        Platform::Windows | Platform::Gnome => shortcut.format(&WORDS, false),
    }
}

/// The shortcut that sends `command`, for its tooltip or menu row.
pub fn hint(platform: Platform, command: &Command) -> Option<String> {
    bindings(platform)
        .iter()
        .find(|b| b.command == *command)
        .map(|b| text(platform, &b.shortcut))
}

/// The menu's shortcut list: each action once, by its first key.
pub fn reference(platform: Platform) -> Vec<(&'static str, String)> {
    let mut rows: Vec<(&'static str, String)> = Vec::new();
    for b in bindings(platform) {
        if !rows.iter().any(|(label, _)| *label == b.label) {
            rows.push((b.label, text(platform, &b.shortcut)));
        }
    }
    rows.push(("Zoom without a modifier", "+  −".to_owned()));
    let listed = LISTED.iter().map(|&(label, keys)| (label, keys.to_owned()));
    rows.into_iter().chain(listed).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_read_in_each_desktops_words() {
        let settings = Command::ShowSettings;
        assert_eq!(hint(Platform::Mac, &settings).as_deref(), Some("⌘,"));
        assert_eq!(
            hint(Platform::Windows, &settings).as_deref(),
            Some("Ctrl+,")
        );
        let full = Command::Window(WindowCommand::ToggleFullscreen);
        assert_eq!(hint(Platform::Gnome, &full).as_deref(), Some("F11"));
        assert_eq!(hint(Platform::Mac, &full).as_deref(), Some("⌃⌘F"));
        assert_eq!(hint(Platform::Mac, &Command::ShowLibrary), None);
        let open = Command::ShowOpenDialog { beside: false };
        assert_eq!(hint(Platform::Gnome, &open).as_deref(), Some("Ctrl+O"));
        let zoom_in = Command::Zoom(ZOOM_IN);
        assert_eq!(hint(Platform::Mac, &zoom_in).as_deref(), Some("⌘+"));
        let fit = Command::Zoom(FIT_WIDTH);
        assert_eq!(hint(Platform::Windows, &fit).as_deref(), Some("Ctrl+0"));
    }

    #[test]
    fn the_reference_lists_each_action_once() {
        let rows = reference(Platform::Mac);
        let zoom_in = rows.iter().filter(|(label, _)| *label == "Zoom in").count();
        assert_eq!(zoom_in, 1);
    }
}
