//! Keyboard shortcuts. One table drives the handler, tooltips and reference.

use egui::{Context, Key, KeyboardShortcut, ModifierNames, Modifiers};

use crate::command::{Command, Commands, WindowCommand};
use crate::documents::Placement;
use crate::layout::{Arrow, ViewMode};
use crate::platform::Platform;
use crate::route::Route;
use crate::zoom::{Fit, Step, ZoomCommand};

pub struct Binding {
    pub label: &'static str,
    pub shortcut: KeyboardShortcut,
    pub command: Command,
}

/// Every bound shortcut; features add theirs here as they land.
pub fn bindings(platform: Platform) -> [Binding; 36] {
    // Each desktop's own full-screen key.
    let fullscreen = match platform {
        Platform::Mac => KeyboardShortcut::new(Modifiers::CTRL | Modifiers::MAC_CMD, Key::F),
        Platform::Windows | Platform::Gnome => KeyboardShortcut::new(Modifiers::NONE, Key::F11),
    };
    [
        Binding {
            label: "Open a file",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::O),
            command: Command::ShowOpenDialog(Placement::Into),
        },
        zoom("Zoom in", Modifiers::COMMAND, Key::Plus, ZOOM_IN),
        zoom("Zoom in", Modifiers::COMMAND, Key::Equals, ZOOM_IN),
        zoom("Zoom out", Modifiers::COMMAND, Key::Minus, ZOOM_OUT),
        zoom("Fit width", Modifiers::COMMAND, Key::Num0, FIT_WIDTH),
        // mareader's bare keys too, while nothing takes typing.
        zoom("Zoom in", Modifiers::NONE, Key::Plus, ZOOM_IN),
        zoom("Zoom in", Modifiers::NONE, Key::Equals, ZOOM_IN),
        zoom("Zoom out", Modifiers::NONE, Key::Minus, ZOOM_OUT),
        view("Single page", Key::Num1, ViewMode::Single),
        view("Vertical scroll", Key::Num2, ViewMode::Vertical),
        // Arrows and vim's keys; the first of each names it.
        arrow("Previous page", Key::ArrowLeft, Arrow::Left),
        arrow("Next page", Key::ArrowRight, Arrow::Right),
        arrow(UP, Key::ArrowUp, Arrow::Up),
        arrow(DOWN, Key::ArrowDown, Arrow::Down),
        arrow("Previous page", Key::H, Arrow::Left),
        arrow("Next page", Key::L, Arrow::Right),
        arrow(UP, Key::K, Arrow::Up),
        arrow(DOWN, Key::J, Arrow::Down),
        // Shift+Space first: a bare pattern ignores Shift.
        page("Back a screen", Modifiers::NONE, Key::PageUp, false),
        page("Forward a screen", Modifiers::NONE, Key::PageDown, true),
        page("Back a screen", Modifiers::SHIFT, Key::Space, false),
        page("Forward a screen", Modifiers::NONE, Key::Space, true),
        Binding {
            label: "Auto-scroll",
            shortcut: KeyboardShortcut::new(Modifiers::SHIFT, Key::A),
            command: Command::AutoScroll,
        },
        Binding {
            label: "First page",
            shortcut: KeyboardShortcut::new(Modifiers::NONE, Key::Home),
            command: Command::FirstPage,
        },
        Binding {
            label: "Last page",
            shortcut: KeyboardShortcut::new(Modifiers::NONE, Key::End),
            command: Command::LastPage,
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
        // Before Find, whose pattern would take Shift too.
        Binding {
            label: "Distraction-free",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::F),
            command: Command::Chromeless,
        },
        // After full screen, which macOS puts on Ctrl+Cmd+F.
        Binding {
            label: "Find",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::F),
            command: Command::ShowFind,
        },
        // Shift first again: the bare key would take Shift+F3 too.
        step("Previous match", Modifiers::SHIFT, Key::F3, false),
        step("Next match", Modifiers::NONE, Key::F3, true),
        step(
            "Previous match",
            Modifiers::COMMAND | Modifiers::SHIFT,
            Key::G,
            false,
        ),
        step("Next match", Modifiers::COMMAND, Key::G, true),
        Binding {
            label: "Bookmark this page",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::D),
            command: Command::Bookmark(None),
        },
        Binding {
            label: "Previous pane",
            shortcut: KeyboardShortcut::new(Modifiers::SHIFT, Key::F6),
            command: Command::CyclePane { forward: false },
        },
        Binding {
            label: "Next pane",
            shortcut: KeyboardShortcut::new(Modifiers::NONE, Key::F6),
            command: Command::CyclePane { forward: true },
        },
    ]
}

/// Along a column the up and down arrows scroll; elsewhere they turn.
const UP: &str = "Scroll up, or previous page";
const DOWN: &str = "Scroll down, or next page";

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

fn view(label: &'static str, key: Key, mode: ViewMode) -> Binding {
    Binding {
        label,
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, key),
        command: Command::View(mode),
    }
}

fn arrow(label: &'static str, key: Key, arrow: Arrow) -> Binding {
    Binding {
        label,
        shortcut: KeyboardShortcut::new(Modifiers::NONE, key),
        command: Command::Arrow(arrow),
    }
}

fn step(label: &'static str, modifiers: Modifiers, key: Key, forward: bool) -> Binding {
    Binding {
        label,
        shortcut: KeyboardShortcut::new(modifiers, key),
        command: Command::FindStep { forward },
    }
}

/// Page keys: nearly a screen along a strip, a turn when paged.
fn page(label: &'static str, modifiers: Modifiers, key: Key, forward: bool) -> Binding {
    Binding {
        label,
        shortcut: KeyboardShortcut::new(modifiers, key),
        command: Command::Page { forward },
    }
}

/// Keys that a focused text field types or moves its cursor by.
fn types(shortcut: &KeyboardShortcut) -> bool {
    use Key::{
        A, ArrowDown, ArrowLeft, ArrowRight, ArrowUp, End, Equals, H, Home, J, K, L, Minus,
        PageDown, PageUp, Plus, Space,
    };
    let bare = shortcut.modifiers.is_none() || shortcut.modifiers == Modifiers::SHIFT;
    bare && matches!(
        shortcut.logical_key,
        Plus | Equals
            | A
            | Minus
            | ArrowLeft
            | ArrowRight
            | ArrowUp
            | ArrowDown
            | H
            | J
            | K
            | L
            | Space
            | PageUp
            | PageDown
            | Home
            | End
    )
}

/// Arrows move egui's focus and Space presses buttons, while one has it.
fn steers(b: &Binding) -> bool {
    matches!(b.command, Command::Arrow(_)) || b.shortcut.logical_key == Key::Space
}

/// Whether a key that sends `arrow` is still held down.
pub fn held(input: &egui::InputState, arrow: Arrow) -> bool {
    let keys = match arrow {
        Arrow::Left => [Key::ArrowLeft, Key::H],
        Arrow::Right => [Key::ArrowRight, Key::L],
        Arrow::Up => [Key::ArrowUp, Key::K],
        Arrow::Down => [Key::ArrowDown, Key::J],
    };
    keys.iter().any(|k| input.key_down(*k))
}

/// Keys handled elsewhere, listed so the reference is complete.
const LISTED: [(&str, &str); 4] = [
    ("Close a popover, window or sidebar", "Esc"),
    ("Move between controls", "Tab  Shift+Tab"),
    ("Press the focused control, link or mark", "Enter  Space"),
    ("Open the focused item's menu", "Shift+F10"),
];

/// Consumes this frame's shortcuts and queues their commands.
pub fn read(ctx: &Context, platform: Platform, route: Route, out: &mut Commands) {
    // A focused button wants Space and the arrows, not every key.
    let typing = ctx.text_edit_focused();
    let focused = ctx.memory(|m| m.focused().is_some()) || egui::Popup::is_any_open(ctx);
    // Keys the route ignores stay unconsumed, for the page's own widgets.
    for b in bindings(platform)
        .into_iter()
        .filter(|b| route.allows(&b.command))
    {
        if (typing && types(&b.shortcut)) || (focused && steers(&b)) {
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
        let open = Command::ShowOpenDialog(Placement::Into);
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
