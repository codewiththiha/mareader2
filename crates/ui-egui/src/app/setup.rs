use egui::Context;
use mareader_engine::host::Machine;

use super::*;

impl MareaderApp {
    /// Reopens what was read at exit, if its file is still there.
    pub fn resume(&mut self) -> bool {
        let Some(last) = self.ui.last.take().filter(|l| l.path.is_file()) else {
            return false;
        };
        let place = last.place.map(|(block, offset)| Anchor { block, offset });
        let spine = last.spine.map(|place| mareader_session::SpinePlace {
            href: place.href,
            item: place.item,
            block: place.block,
            offset: place.offset,
        });
        let request = OpenRequest {
            page: last.page,
            place,
            spine,
            pages: last.pages,
            book: last.book,
            ..OpenRequest::path(last.path)
        };
        self.queue(Sent::Engine(Command::Open(request)));
        true
    }

    /// Wires every controller to the places `machine` names.
    ///
    /// The session keeps the machine, so a front end can ask it back, and the
    /// waker built here is the session's too: a service on its own thread can
    /// ask for a frame. A place left unnamed attaches nothing, and its
    /// controller keeps its state in memory.
    pub fn attach(&mut self, ctx: &Context, machine: Machine) {
        self.session.set_machine(machine.clone());
        let waker = repainter(ctx);
        self.session.set_waker(waker.clone());
        if let Some(root) = machine.library.clone() {
            self.ui.library.attach(root, waker.clone());
        }
        if let Some(dir) = machine.cache.clone() {
            self.ui.library.attach_cache(dir);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.attach_services(ctx, &machine, &waker);
        self.drain(ctx);
    }

    /// Voice models and the pronunciation pack, sharing files in `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn attach_voices(
        &mut self,
        dir: PathBuf,
        catalogue: &'static [mareader_speech::Model],
        waker: mareader_speech::Waker,
    ) -> Result<(), String> {
        let shelf = &mut self.ui.read_aloud.shelf;
        shelf.attach(dir.clone(), catalogue, waker.clone())?;
        self.ui.sounds.attach(dir, waker, shelf)
    }

    /// The services only a desktop has: words, voices and the AI.
    #[cfg(not(target_arch = "wasm32"))]
    fn attach_services(
        &mut self,
        ctx: &Context,
        machine: &Machine,
        waker: &mareader_speech::Waker,
    ) {
        if let Some(dir) = machine.dictionaries.clone()
            && let Err(why) = self.ui.dict.attach(dir, waker.clone())
        {
            self.refused("Dictionaries are off", why);
        }
        if let Some(dir) = machine.vocabulary.clone()
            && let Err(why) = self.ui.vocab.attach(dir, waker.clone())
        {
            self.refused("Word levels are off", why);
        }
        if let Some(dir) = machine.voices.clone() {
            let offered = machine.offered.unwrap_or(mareader_speech::MODELS);
            if let Err(why) = self.attach_voices(dir, offered, waker.clone()) {
                self.refused("Voice models are off", why);
            }
        }
        if let Some(root) = machine.ai.clone() {
            self.ui.ai.attach(ctx, root, machine.ai_helper.clone());
            // A run stopped before quitting waits on its card.
            self.ui.library.set_paused(self.ui.ai.paused());
        }
    }

    /// A service that refused to start says so, through the session's events.
    #[cfg(not(target_arch = "wasm32"))]
    fn refused(&mut self, what: &str, why: String) {
        self.session.report(Toast::info(format!("{what}: {why}")));
    }
}

/// Asks for a frame from any thread, as a service off this one needs.
fn repainter(ctx: &Context) -> mareader_speech::Waker {
    let ctx = ctx.clone();
    Arc::new(move || ctx.request_repaint())
}
