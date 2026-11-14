use super::*;

mod appearance;
mod library;
mod page_ai;
mod panes;
mod read_aloud;
mod study;

impl MareaderApp {
    /// Headless knobs for `examples/shot.rs` and tests.
    ///
    /// The engine parses the keys that set what it holds, so a shot, a query
    /// string and a command line all meet the same answer. What is left here
    /// drives this window: its widgets, and the fixtures that fill them.
    pub fn set_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.reconcile_texts();
        if let Some(parsed) = mareader_engine::options::parse(key, value) {
            return self.carry(parsed?);
        }
        if let Some(result) = appearance::set(self, key, value) {
            return result;
        }
        if let Some(result) = library::set(self, key, value) {
            return result;
        }
        if let Some(result) = study::set(self, key, value) {
            return result;
        }
        if let Some(result) = page_ai::set(self, key, value) {
            return result;
        }
        if let Some(result) = read_aloud::set(self, key, value) {
            return result;
        }
        if let Some(result) = panes::set(self, key, value) {
            return result;
        }
        Err(format!("unknown option {key}"))
    }

    /// The engine's answer, handed on by the door each key asks for.
    ///
    /// A setting write lands at once, so the next key in the same string reads
    /// it; an ask or an open waits behind whatever is already queued.
    fn carry(&mut self, commands: Vec<Command>) -> Result<(), String> {
        for command in commands {
            if matches!(command, Command::SetField { .. }) {
                let done = self.session.apply(command);
                if let Some(why) = done.refused {
                    return Err(why);
                }
            } else {
                self.queue(Sent::Engine(command));
            }
        }
        Ok(())
    }
}

/// A toast from a shot's words: `_` for spaces, a prefix for its tone.
pub(super) fn shot_toast(value: &str) -> Toast {
    let words = value.replace('_', " ");
    let (kind, message) = words.split_once(':').unwrap_or(("", &words));
    match kind {
        "error" => Toast::error(message),
        "warning" => Toast::warning(message),
        "player" => Toast::info(message).player(),
        "undo" => Toast::info(message).undo(Command::Study(StudyAction::Restore(Vec::new()))),
        "action" => {
            Toast::info(message).action("Resume", Command::Study(StudyAction::Restore(Vec::new())))
        }
        _ => Toast::info(words.as_str()),
    }
}
