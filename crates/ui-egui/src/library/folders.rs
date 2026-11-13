//! The rescan tick, and the offline test the view asks for.

use egui::Context;

use super::LibraryState;
pub use mareader_engine::library::offline;

impl LibraryState {
    /// Rescans when the window comes back, and after files settle.
    pub fn tick(&mut self, ctx: &Context) {
        let (now, focused) = ctx.input(|i| (i.time, i.focused));
        if let Some(wait) = self.state.tick(now, focused) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
    }
}
