//! The library's state: the model, its files, and what the panes read.

mod add;
pub mod asks;
mod copying;
mod covers;
pub mod crumbs;
pub mod folders;
mod import_sheet;
mod living;
pub(crate) mod menu;
mod menus;
mod remove_alert;
mod review;
pub mod runs;
pub mod search;
pub mod sheets;
pub mod view;

use std::path::Path;

use egui::Context;
use mareader_library::{BookId, Format, Library, Opened, Position, SpinePosition, display_title};
use mareader_session::Document;
use mareader_workspace::PaneId;

use crate::documents::Documents;
use crate::ui_command::{Commands, Sent, UiCommand};
pub use add::ADD_MENU;
pub use covers::Covers;
use mareader_engine::command::Command;
pub use mareader_engine::library::{Ask, Bound, Field, Heard, Listing, Paused, Proposal, Spot};
use mareader_engine::library::{Job, News, Out, Probe, State, Status, wall_ms};
use mareader_engine::toast::Toast;
pub use menu::LIBRARY_VIEW_MENU;
pub use menus::{Organizer, organize_rows};

#[derive(Default)]
pub struct LibraryState {
    /// What outlives a frame: the model, the page's place, the sheet.
    state: mareader_engine::library::State,
    covers: Covers,
    motion: living::Motion,
}

/// Field reads fall through to the engine's state, as they did when it
/// was all here.
impl std::ops::Deref for LibraryState {
    type Target = mareader_engine::library::State;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl std::ops::DerefMut for LibraryState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

impl LibraryState {
    /// Runs `work` on the engine's state, then answers what it asked for.
    ///
    /// The engine never calls upward: a note, a copy, a file dialog all come
    /// back as an `Out` for the window to carry out.
    fn engine(&mut self, out: &mut Commands, work: impl FnOnce(&mut State, &mut Vec<Out>)) {
        let mut asked = Vec::new();
        work(&mut self.state, &mut asked);
        for ask in asked {
            match ask {
                Out::Toast(toast) => out.push(Sent::Ui(UiCommand::Notify(toast))),
                Out::Command(command) => out.push(Sent::Engine(command)),
            }
        }
    }

    /// What the AI's run reports; the engine's words become the app's.
    pub fn heard(&mut self, heard: Heard, out: &mut Commands) {
        self.engine(out, |state, asked| state.heard(heard, asked));
    }

    /// Paints `book`'s cover in `rect`; false when it isn't shelved.
    pub fn paint_cover(&mut self, ui: &egui::Ui, rect: egui::Rect, book: BookId) -> bool {
        let Some(found) = self.state.model.book(book) else {
            return false;
        };
        let t = crate::theme::Tokens::get(ui.ctx());
        view::thumb(ui, rect, found, &mut self.covers, &t);
        true
    }

    /// Disk work runs on the frame; set before [`Self::attach`].
    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
        self.covers.set_inline(inline);
    }

    /// The books, and their covers to draw them with.
    pub fn parts(&mut self) -> (&Library, &mut Covers) {
        (&self.state.model, &mut self.covers)
    }

    /// What the page draws with, its grid's moving parts included.
    fn page_parts(&mut self) -> (&Library, &mut Covers, &mut living::Motion) {
        (&self.state.model, &mut self.covers, &mut self.motion)
    }

    pub fn covers(&self) -> &Covers {
        &self.covers
    }

    /// Hands the frame's cover asks to their worker.
    pub fn end_frame(&mut self, ctx: &Context) {
        let cache = self.state.cache.clone();
        self.covers.end_frame(ctx, cache.as_deref());
    }

    /// Takes in finished disk work.
    pub fn receive(&mut self, ctx: &Context, out: &mut Commands) {
        let now = ctx.input(|i| i.time);
        for news in self.io.poll() {
            match news {
                News::Loaded(loaded) => {
                    // Opens before loading can't have touched the model.
                    let loaded = *loaded;
                    self.model = loaded.library;
                    self.status = match loaded.writable {
                        true => Status::Ready,
                        false => Status::ReadOnly,
                    };
                    let notes = loaded.notes.into_iter().map(Toast::error);
                    out.extend(notes.map(|t| Sent::Ui(UiCommand::Notify(t))));
                    let held = self.waiting.drain(..);
                    out.extend(held.map(|r| Sent::Engine(Command::Open(r))));
                    // Gone files and new books, found where they're kept.
                    self.rescan(now);
                }
                News::Measured(probe, measure) => {
                    let opened = Opened {
                        path: probe.path,
                        measure,
                        format: probe.format,
                        title: probe.title,
                        author: probe.author,
                    };
                    self.model.record_open_as(opened, wall_ms(), probe.book);
                }
                News::WriteFailed(e) => out.push(Sent::Ui(UiCommand::Notify(Toast::error(e)))),
                News::Walked(done) => self.engine(out, |s, a| s.walked(*done, now, a)),
                News::Files(done) => self.engine(out, |s, a| s.measured(*done, now, a)),
                News::Copied(done) => self.engine(out, |s, a| s.copied(*done, now, a)),
                News::Checked(gone) => self.checked(&gone),
                News::Relinked(relink, measured) => {
                    self.engine(out, |s, a| s.relink_measured(relink, measured, a))
                }
                News::Sought {
                    book,
                    folder,
                    found,
                } => self.engine(out, |s, a| s.sought_found(book, &folder, found, a)),
                News::Beat(beat) => self.runs.beat(beat),
            }
        }
    }

    /// Records opens and positions; true if a file went to be measured.
    pub fn observe(&mut self, documents: &Documents, panes: &[PaneId]) -> bool {
        let mut measuring = false;
        if self.status == Status::Loading {
            return measuring;
        }
        let now = wall_ms();
        for &pane in panes {
            let (Some(reading), Some(place)) = (documents.reading(pane), documents.place(pane))
            else {
                self.seen.remove(&pane);
                continue;
            };
            // The view carries its book: a failed open leaves nothing.
            let reader = documents.book(pane);
            let shown = (place.path, reader);
            if self.seen.get(&pane) != Some(&shown) {
                let (path, book) = shown.clone();
                self.seen.insert(pane, shown);
                self.io.send(Job::Measure(Box::new(Probe {
                    format: format_of(&path, &reading.document),
                    title: title_of(&reading.document, &path),
                    author: author_of(&reading.document),
                    path,
                    book,
                })));
                measuring = true;
                continue;
            }
            let (path, reader) = shown;
            // A book removed while read keeps no place, nor hands it on.
            let id = match reader {
                Some(id) => self.model.book(id).map(|b| b.id),
                None => self.model.book_at(&path),
            };
            let Some(id) = id else {
                continue;
            };
            let position = Position {
                page: place.page,
                pages: place.pages,
                place: place
                    .anchor
                    .filter(|_| reading.document.is_complete())
                    .map(|anchor| (anchor.block, anchor.offset)),
                spine: place.spine.map(|spine| SpinePosition {
                    href: spine.href,
                    item: spine.item,
                    block: spine.block,
                    offset: spine.offset,
                }),
            };
            self.model.record_position(id, position, now);
        }
        self.seen.retain(|p, _| panes.contains(p));
        measuring
    }

    /// The book `pane` reads as: its own, else its file's.
    pub fn book_in(&self, documents: &Documents, pane: PaneId) -> Option<BookId> {
        let own = documents.book(pane);
        own.or_else(|| self.model.book_at(&documents.place(pane)?.path))
    }
}

/// The file's own format; the document says if it's a book.
fn format_of(path: &Path, document: &Document) -> Format {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    match mareader_epub::Format::of_name(name) {
        Some(mareader_epub::Format::Plain) => Format::Text,
        Some(mareader_epub::Format::Markdown) => Format::Markdown,
        None if document.is_reflowable() => Format::Epub,
        None => Format::Pdf,
    }
}

/// A book's own title, unless generic; else the file's name.
fn title_of(document: &Document, path: &Path) -> String {
    let own = match document {
        Document::Epub { book, .. } => book.title(),
        Document::Pdf(pdf) => pdf.raw_title(),
    };
    display_title(own, path)
}

/// The document's own author, if it names one.
fn author_of(document: &Document) -> Option<String> {
    let own = match document {
        Document::Epub { book, .. } => book.author(),
        Document::Pdf(pdf) => pdf.raw_author(),
    };
    own.map(str::trim)
        .filter(|a| !a.is_empty())
        .map(str::to_owned)
}
