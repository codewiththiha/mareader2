//! The rescan tick, and the offline test the view asks for.

use egui::Context;

use super::LibraryState;
pub use mareader_engine::library::offline;
use mareader_engine::library::{Job, RESCAN_GAP, Status};

impl LibraryState {
    /// Rescans when the window comes back, and after files settle.
    pub fn tick(&mut self, ctx: &Context) {
        // Removals and replacements free store copies; they go now.
        let swept = self.model.take_swept();
        if let Some(store) = self.folder.as_ref().map(|f| f.store())
            && !swept.is_empty()
        {
            self.io.send(Job::Delete {
                store,
                paths: swept,
            });
        }
        let (now, focused) = ctx.input(|i| (i.time, i.focused));
        let back = focused && !self.folders.focused;
        self.folders.focused = focused;
        let rested = self.folders.last.is_none_or(|t| now - t >= RESCAN_GAP);
        let watched = self.model.folders().any(|f| f.owes_walk());
        if back && rested && watched && self.status != Status::Loading {
            self.rescan(now);
        }
        if let Some(at) = self.folders.settle_at {
            match now >= at {
                true => self.rescan(now),
                false => ctx.request_repaint_after(std::time::Duration::from_secs_f64(at - now)),
            }
        }
    }
}
