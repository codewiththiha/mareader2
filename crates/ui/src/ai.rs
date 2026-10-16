//! The AI service behind Explain and the AI settings; desktop only.

use std::sync::Arc;

use mareader_settings::AiSettings;
use mareader_study::Explainer;

#[cfg(not(target_arch = "wasm32"))]
pub use mareader_assist::{Board, Job};

/// The service once attached, and the settings its answerer follows.
#[derive(Default)]
pub struct Ai {
    #[cfg(not(target_arch = "wasm32"))]
    assist: Option<mareader_assist::Assist>,
    made_for: Option<AiSettings>,
}

impl Ai {
    /// Starts the service; keys live under `root`. False if it can't run.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach(&mut self, ctx: &egui::Context, root: std::path::PathBuf) -> bool {
        let ctx = ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        match mareader_assist::Assist::new(root, waker) {
            Ok(assist) => {
                self.assist = Some(assist);
                self.made_for = None;
                true
            }
            Err(_) => false,
        }
    }

    /// Takes in finished jobs; a new answerer if `ai` changed since.
    pub fn follow(&mut self, ai: &AiSettings) -> Option<Arc<dyn Explainer>> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(assist) = &mut self.assist {
            assist.poll();
            if self.made_for.as_ref() != Some(ai) {
                self.made_for = Some(ai.clone());
                return Some(assist.configure(ai));
            }
        }
        let _ = (ai, &self.made_for);
        None
    }

    /// What the AI settings show; none without the service.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn board(&self) -> Option<&Board> {
        self.assist.as_ref().map(mareader_assist::Assist::board)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn send(&mut self, job: Job) {
        if let Some(assist) = &mut self.assist {
            assist.send(job);
        }
    }
}
