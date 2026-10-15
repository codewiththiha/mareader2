//! The open dialog and dropped files; results arrive later as commands.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use egui::{Context, DroppedFileHandle};
use mareader_workspace::PaneId;

use crate::command::{Command, Commands};
use crate::documents::OpenRequest;

/// What the open dialog admits.
/// Markdown and plain text open converted to books.
const BOOKS: [&str; 7] = ["pdf", "epub", "txt", "text", "md", "markdown", "mdown"];

/// Commands finished off the frame, waiting for the next one.
#[derive(Clone, Default)]
pub struct Files {
    inbox: Arc<Mutex<Commands>>,
    /// One dialog at a time; a second request is ignored.
    picking: Arc<AtomicBool>,
}

impl Files {
    /// Moves arrived commands into this frame's queue.
    pub fn collect(&self, out: &mut Commands) {
        let mut inbox = self.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        out.append(&mut inbox);
    }

    fn deliver(&self, ctx: &Context, command: Command) {
        let mut inbox = self.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        inbox.push(command);
        ctx.request_repaint();
    }

    /// Shows the system open dialog; `beside` opens the pick in a split.
    pub fn pick(&self, ctx: &Context, beside: bool) {
        if self.picking.swap(true, Ordering::AcqRel) {
            return;
        }
        let (files, ctx) = (self.clone(), ctx.clone());
        let done = move |request: Option<OpenRequest>| {
            files.picking.store(false, Ordering::Release);
            // Cancelling is silent, as in mareader.
            if let Some(request) = request {
                files.deliver(&ctx, Command::Open(OpenRequest { beside, ..request }));
            }
        };
        if !dialog::run(done) {
            self.picking.store(false, Ordering::Release);
        }
    }

    /// Shows the folder dialog; `then` turns the pick into a command.
    pub fn pick_folder(
        &self,
        ctx: &Context,
        title: &str,
        then: impl FnOnce(PathBuf) -> Command + Send + 'static,
    ) {
        let title = title.to_owned();
        let make = move || rfd::AsyncFileDialog::new().set_title(title);
        self.pick_path(ctx, make, true, then);
    }

    /// Shows the file dialog for one book's `extensions`.
    pub fn pick_book(
        &self,
        ctx: &Context,
        extensions: &'static [&'static str],
        then: impl FnOnce(PathBuf) -> Command + Send + 'static,
    ) {
        let make = move || {
            books()
                .set_title("Choose the book's file")
                .add_filter("This book's format", extensions)
        };
        self.pick_path(ctx, make, false, then);
    }

    fn pick_path(
        &self,
        ctx: &Context,
        make: impl FnOnce() -> rfd::AsyncFileDialog + Send + 'static,
        folder: bool,
        then: impl FnOnce(PathBuf) -> Command + Send + 'static,
    ) {
        if self.picking.swap(true, Ordering::AcqRel) {
            return;
        }
        let (files, ctx) = (self.clone(), ctx.clone());
        let done = move |path: Option<PathBuf>| {
            files.picking.store(false, Ordering::Release);
            if let Some(path) = path {
                files.deliver(&ctx, then(path));
            }
        };
        if !dialog::run_path(make, folder, done) {
            self.picking.store(false, Ordering::Release);
        }
    }

    /// Opens a dropped file in `pane`.
    pub fn dropped(&self, ctx: &Context, file: &DroppedFileHandle, pane: PaneId) {
        let place = move |request: OpenRequest| OpenRequest {
            pane: Some(pane),
            ..request
        };
        dialog::read_drop(self, ctx, file, place);
    }
}

fn books() -> rfd::AsyncFileDialog {
    rfd::AsyncFileDialog::new()
        .set_title("Open a book")
        .add_filter("Books", &BOOKS)
}

#[cfg(not(target_arch = "wasm32"))]
mod dialog {
    use super::*;

    /// A helper thread keeps the window drawing; false if it didn't start.
    pub(super) fn run(done: impl FnOnce(Option<OpenRequest>) + Send + 'static) -> bool {
        std::thread::Builder::new()
            .name("mareader-open-dialog".into())
            .spawn(move || {
                let file = pollster::block_on(books().pick_file());
                done(file.map(|f| OpenRequest::path(f.path().to_path_buf())));
            })
            .is_ok()
    }

    /// A file or folder path, picked on a helper thread.
    pub(super) fn run_path(
        make: impl FnOnce() -> rfd::AsyncFileDialog + Send + 'static,
        folder: bool,
        done: impl FnOnce(Option<PathBuf>) + Send + 'static,
    ) -> bool {
        std::thread::Builder::new()
            .name("mareader-path-dialog".into())
            .spawn(move || {
                let dialog = make();
                let picked = match folder {
                    true => pollster::block_on(dialog.pick_folder()),
                    false => pollster::block_on(dialog.pick_file()),
                };
                done(picked.map(|f| f.path().to_path_buf()));
            })
            .is_ok()
    }

    /// A desktop drop has a real path; a worker reads it.
    pub(super) fn read_drop(
        files: &Files,
        ctx: &Context,
        file: &DroppedFileHandle,
        place: impl FnOnce(OpenRequest) -> OpenRequest,
    ) {
        let request = place(OpenRequest::path(file.path().to_path_buf()));
        files.deliver(ctx, Command::Open(request));
    }
}

/// Browsers hide paths: files arrive as bytes, read asynchronously.
#[cfg(target_arch = "wasm32")]
mod dialog {
    use super::*;
    use crate::toast::Toast;

    pub(super) fn run(done: impl FnOnce(Option<OpenRequest>) + 'static) -> bool {
        wasm_bindgen_futures::spawn_local(async move {
            let request = match books().pick_file().await {
                Some(file) => Some(OpenRequest::bytes(file.read().await, file.file_name())),
                None => None,
            };
            done(request);
        });
        true
    }

    /// Browsers hide paths, so folders and relinks can't be picked.
    pub(super) fn run_path(
        _make: impl FnOnce() -> rfd::AsyncFileDialog,
        _folder: bool,
        _done: impl FnOnce(Option<PathBuf>),
    ) -> bool {
        false
    }

    pub(super) fn read_drop(
        files: &Files,
        ctx: &Context,
        file: &DroppedFileHandle,
        place: impl FnOnce(OpenRequest) -> OpenRequest + 'static,
    ) {
        let (files, ctx, file) = (files.clone(), ctx.clone(), Arc::clone(file));
        wasm_bindgen_futures::spawn_local(async move {
            let name = file.path().to_string_lossy().into_owned();
            let command = match file.bytes_async().await {
                Ok(bytes) => Command::Open(place(OpenRequest::bytes(bytes, name))),
                Err(e) => Command::Notify(Toast::error(format!("Couldn't read {name}: {e}"))),
            };
            files.deliver(&ctx, command);
        });
    }
}
