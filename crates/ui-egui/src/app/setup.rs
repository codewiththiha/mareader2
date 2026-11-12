use egui::Context;

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
        self.queue(Command::Intent(Intent::Open(request)));
        true
    }

    /// Keeps the library in `root`; without one it lives in memory.
    pub fn attach_library(&mut self, root: PathBuf) {
        self.ui.library.attach(root);
    }

    /// Starts the AI service, its keys kept in `root`; `helper` as in `Ai`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_ai(&mut self, ctx: &Context, root: PathBuf, helper: Option<PathBuf>) {
        self.ui.ai.attach(ctx, root, helper);
        // A run stopped before quitting waits on its card.
        self.ui.library.set_paused(self.ui.ai.paused());
    }

    /// Keeps language packs in `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_dictionaries(&mut self, ctx: &Context, dir: PathBuf) {
        let ctx = ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        if let Err(e) = self.ui.dict.attach(dir, waker) {
            self.ui
                .toasts
                .push(Toast::info(format!("Dictionaries are off: {e}")));
        }
    }

    /// Keeps the word-level dataset in `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_vocabulary(&mut self, ctx: &Context, dir: PathBuf) {
        let ctx = ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        if let Err(e) = self.ui.vocab.attach(dir, waker) {
            self.ui
                .toasts
                .push(Toast::info(format!("Word levels are off: {e}")));
        }
    }

    /// Keeps downloaded voice models in `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_voice_models(&mut self, ctx: &Context, dir: PathBuf) {
        self.attach_voice_models_from(ctx, dir, mareader_speech::MODELS);
    }

    /// As [`Self::attach_voice_models`], offering `catalogue` instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_voice_models_from(
        &mut self,
        ctx: &Context,
        dir: PathBuf,
        catalogue: &'static [mareader_speech::Model],
    ) {
        let ctx = ctx.clone();
        let waker = Arc::new(move || ctx.request_repaint());
        if let Err(e) = self.attach_voices(dir, catalogue, waker) {
            self.ui
                .toasts
                .push(Toast::info(format!("Voice models are off: {e}")));
        }
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

    /// Keeps covers and other rebuildable files in `dir`.
    pub fn attach_cache(&mut self, dir: PathBuf) {
        self.ui.library.attach_cache(dir);
    }
}
