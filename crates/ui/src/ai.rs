//! The AI service behind Explain and the AI settings; desktop only.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
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
    /// Starts the service, keys under `root`, maybe an unbundled `helper`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach(&mut self, ctx: &egui::Context, root: PathBuf, helper: Option<PathBuf>) -> bool {
        let ctx = ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        let started = mareader_assist::Assist::new(root, waker);
        match started.map(|a| helper.into_iter().fold(a, |a, h| a.with_helper(h))) {
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
