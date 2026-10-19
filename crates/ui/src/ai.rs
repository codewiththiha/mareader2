//! The AI service behind Explain and the AI settings; desktop only.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::Arc;

use mareader_settings::AiSettings;
use mareader_study::Explainer;

#[cfg(not(target_arch = "wasm32"))]
use crate::library::organizing::{Ask, Proposal, Spot};
use crate::library::organizing::{Heard, Paused};
#[cfg(not(target_arch = "wasm32"))]
use crate::toast::Toast;

#[cfg(not(target_arch = "wasm32"))]
pub use mareader_assist::{Board, Job};

/// The service once attached, and the settings its answerer follows.
#[derive(Default)]
pub struct Ai {
    #[cfg(not(target_arch = "wasm32"))]
    assist: Option<mareader_assist::Assist>,
    made_for: Option<AiSettings>,
    /// The library run under way; dropping it stops the run.
    #[cfg(not(target_arch = "wasm32"))]
    organizing: Option<mareader_assist::Organizing>,
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

    /// Tasks the saved model `id` runs now.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn in_use(&self, id: mareader_settings::PresetId) -> usize {
        self.assist.as_ref().map_or(0, |a| a.in_use(id))
    }

    /// Files `ask`'s books, or carries on the paused run; false without AI.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn organize(&mut self, ask: &Ask, batch: u16, resume: bool) -> bool {
        let Some(assist) = &self.assist else {
            return false;
        };
        let req = request(ask, batch);
        self.organizing = Some(match resume {
            true => assist.resume(req),
            false => assist.organize(req),
        });
        true
    }

    /// The run a stop, a failure or quitting left, as the library shows it.
    pub fn paused(&self) -> Option<Paused> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(run) = self.assist.as_ref().and_then(|a| a.paused()) {
            let (done, total) = run.steps();
            return Some(Paused {
                done,
                total,
                books: run.books().map(mareader_library::BookId).collect(),
                whole: run.whole,
                ready: run.finished(),
            });
        }
        None
    }

    /// Sets the paused run aside for an Undo; false if there was none.
    pub fn set_aside(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(assist) = &self.assist {
            return assist.set_aside();
        }
        false
    }

    pub fn bring_back(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(assist) = &self.assist {
            return assist.bring_back();
        }
        false
    }

    /// Drops the paused run for good, once its plan is applied.
    pub fn forget(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(assist) = &self.assist {
            assist.forget();
        }
    }

    /// What the library run said since the last call.
    pub fn organized(&mut self) -> Vec<Heard> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(run) = &mut self.organizing {
            let heard: Vec<Heard> = run.poll().into_iter().map(heard).collect();
            let over = heard.iter().any(|h| !matches!(h, Heard::Progress { .. }));
            if over {
                self.organizing = None;
            }
            return heard;
        }
        Vec::new()
    }

    pub fn stop_organizing(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(mut run) = self.organizing.take() {
            run.cancel();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn send(&mut self, job: Job) {
        if let Some(assist) = &mut self.assist {
            assist.send(job);
        }
    }
}

/// The library's ask, keyed as the service wants it.
#[cfg(not(target_arch = "wasm32"))]
fn request(ask: &Ask, batch: u16) -> mareader_assist::Request {
    use mareader_assist::{Existing, Loose};
    let books = (ask.books.iter())
        .map(|b| Loose {
            key: b.id.0,
            title: b.title.clone(),
            author: b.author.clone().unwrap_or_default(),
        })
        .collect();
    let shelves = (ask.shelves.iter())
        .map(|s| Existing {
            key: s.id.0,
            name: s.name.clone(),
            parent: s.parent.map(|p| p.0),
        })
        .collect();
    mareader_assist::Request {
        books,
        shelves,
        batch: usize::from(batch),
        whole: ask.whole,
    }
}

/// The service's news, in the library's words.
#[cfg(not(target_arch = "wasm32"))]
fn heard(news: mareader_assist::Organized) -> Heard {
    use mareader_assist::{Organized, Stage};
    match news {
        Organized::Progress(p) => Heard::Progress {
            filing: p.stage == Stage::Filing,
            done: p.done,
            total: p.total,
        },
        Organized::Done(Ok(plan)) => Heard::Plan(proposal(plan)),
        Organized::Done(Err(f)) => Heard::Failed(failed(&f)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn proposal(plan: mareader_assist::Plan) -> Proposal {
    use mareader_library::{BookId, ShelfId};
    let spot = |s: mareader_assist::Spot| match s {
        mareader_assist::Spot::Existing(key) => Spot::Shelf(ShelfId(key)),
        mareader_assist::Spot::New(i) => Spot::New(i),
    };
    Proposal {
        shelves: (plan.shelves.into_iter())
            .map(|s| (s.name, s.parent.map(spot)))
            .collect(),
        placed: (plan.placed.into_iter())
            .map(|(key, s)| (BookId(key), spot(s)))
            .collect(),
        left: plan.left.into_iter().map(BookId).collect(),
    }
}

/// What a failed run's toast says; a busy model asks to wait.
#[cfg(not(target_arch = "wasm32"))]
fn failed(f: &mareader_study::Failure) -> Toast {
    use mareader_study::Why;
    let said = match f.why {
        Why::InUse => f
            .detail
            .clone()
            .unwrap_or_else(|| f.why.message().to_owned()),
        Why::NotHere => "No AI is set up to organize with. Choose a model in Settings › AI.".into(),
        Why::Refused => "The model declined to organize these books.".into(),
        Why::BadAnswer => "The model's answers couldn't be read. Try another model.".into(),
        why => format!("Couldn't organize the books. {}", why.message()),
    };
    match f.why {
        Why::InUse | Why::NotHere | Why::NoKey | Why::Locked => Toast::warning(said),
        _ => Toast::error(said),
    }
}
