//! Keyboard shortcuts. One table drives the handler, tooltips and reference.

use egui::{Context, Key, KeyboardShortcut, ModifierNames, Modifiers};

use crate::command::{Command, Commands, WindowCommand};
use crate::platform::Platform;

pub struct Binding {
    pub label: &'static str,
    pub shortcut: KeyboardShortcut,
    pub command: Command,
}

/// Every bound shortcut; features add theirs here as they land.
pub fn bindings(platform: Platform) -> [Binding; 3] {
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

/// Keys handled elsewhere, listed so the reference is complete.
const LISTED: [(&str, &str); 1] = [("Close a popover, window or sidebar", "Esc")];

/// Consumes this frame's shortcuts and queues their commands.
pub fn read(ctx: &Context, platform: Platform, out: &mut Commands) {
    for b in bindings(platform) {
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

/// What the overflow menu's shortcut list shows.
pub fn reference(platform: Platform) -> Vec<(&'static str, String)> {
    let bound = bindings(platform).map(|b| (b.label, text(platform, &b.shortcut)));
    let listed = LISTED.iter().map(|&(label, keys)| (label, keys.to_owned()));
    bound.into_iter().chain(listed).collect()
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
    }
}
