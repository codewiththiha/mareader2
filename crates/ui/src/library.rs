//! The library's state: the model, its files, and what the panes read.

mod covers;
mod io;
pub(crate) mod menu;
pub mod view;

use std::collections::BTreeMap;
use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};

use egui::Context;
use mareader_library::disk::Folder;
use mareader_library::{BookId, DeviceId, Format, Library, Opened, Position};
use mareader_session::{Anchor, Document, Source};
use mareader_workspace::PaneId;

use crate::command::{Command, Commands};
use crate::documents::{Documents, OpenRequest};
use crate::toast::Toast;
pub use covers::Covers;
use io::{Io, Job, News, Probe};
pub use menu::LIBRARY_VIEW_MENU;

/// Seconds between writes while reading.
pub const SAVE_DELAY: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    /// Its files are being read; opens wait for their positions.
    Loading,
    Ready,
    /// A newer app's files, or none readable: kept in memory only.
    ReadOnly,
}

pub struct LibraryState {
    model: Library,
    /// None: in memory only, as in tests and shots.
    folder: Option<Folder>,
    status: Status,
    io: Io,
    inline: bool,
    /// The file each pane showed when last looked at.
    seen: BTreeMap<PaneId, PathBuf>,
    /// Opens held until the library has loaded.
    waiting: Vec<OpenRequest>,
    /// When the last write left, in egui seconds.
    wrote: f64,
    covers: Covers,
    /// The library page is up in place of the panes.
    pub showing: bool,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self {
            model: Library::new(new_device()),
            folder: None,
            status: Status::Ready,
            io: Io::default(),
            inline: false,
            seen: BTreeMap::new(),
            waiting: Vec::new(),
            wrote: f64::NEG_INFINITY,
            covers: Covers::default(),
            showing: false,
        }
    }
}

impl LibraryState {
    pub fn model(&self) -> &Library {
        &self.model
    }

    pub fn loading(&self) -> bool {
        self.status == Status::Loading
    }

    /// Disk work runs on the frame; set before [`Self::attach`].
    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
        self.covers.set_inline(inline);
    }

    /// Keeps covers in `<dir>/covers` between launches.
    pub fn attach_cache(&mut self, dir: PathBuf) {
        self.covers.set_cache(dir.join("covers"));
    }

    /// The books, and their covers to draw them with.
    pub fn parts(&mut self) -> (&Library, &mut Covers) {
        (&self.model, &mut self.covers)
    }

    pub fn covers(&self) -> &Covers {
        &self.covers
    }

    /// Hands the frame's cover asks to their worker.
    pub fn end_frame(&mut self, ctx: &Context) {
        self.covers.end_frame(ctx);
    }

    /// Reads the library kept in `root`, and saves to it from now on.
    pub fn attach(&mut self, root: PathBuf) {
        let folder = Folder::new(root);
        self.io = if self.inline {
            Io::default()
        } else {
            Io::thread()
        };
        self.io.send(Job::Load(folder.clone(), self.model.device()));
        self.folder = Some(folder);
        self.status = Status::Loading;
    }

    /// Takes in finished disk work.
    pub fn receive(&mut self, ctx: &Context, out: &mut Commands) {
        for news in self.io.poll(ctx) {
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
                    out.extend(notes.map(Command::Notify));
                    let held = self.waiting.drain(..);
                    out.extend(held.map(Command::Open));
                }
                News::Measured(probe, measure) => {
                    let opened = Opened {
                        path: probe.path,
                        measure,
                        format: probe.format,
                        title: probe.title,
                        author: probe.author,
                    };
                    self.model.record_open(opened, wall_ms());
                }
                News::WriteFailed(e) => out.push(Command::Notify(Toast::error(e))),
            }
        }
    }

    /// Fills in where an open file was left; held while loading.
    pub fn resume(&mut self, mut request: OpenRequest) -> Option<OpenRequest> {
        let Source::Path(path) = &request.source else {
            return Some(request);
        };
        if request.page != 0 || request.place.is_some() {
            return Some(request);
        }
        if self.status == Status::Loading {
            self.waiting.push(request);
            return None;
        }
        // Recorded as `Documents::open` spells it: canonical.
        let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        let at = self.model.book_at(&real);
        if let Some(p) = at.and_then(|id| self.model.position(id)) {
            request.page = p.page;
            request.place = p.place.map(|(block, offset)| Anchor { block, offset });
        }
        Some(request)
    }

    /// Records opens and positions; true if a file went to be measured.
    pub fn observe(&mut self, documents: &Documents, panes: &[PaneId]) -> bool {
        let mut measuring = false;
        if self.status == Status::Loading {
            return measuring;
        }
        let now = wall_ms();
        for &pane in panes {
            let (Some(reading), Some((path, page, place))) =
                (documents.reading(pane), documents.place(pane))
            else {
                self.seen.remove(&pane);
                continue;
            };
            if self.seen.get(&pane) != Some(&path) {
                let format = format_of(&path, &reading.document);
                let title = title_of(&reading.document, &path);
                let author = author_of(&reading.document);
                self.seen.insert(pane, path.clone());
                self.io.send(Job::Measure(Probe {
                    path,
                    format,
                    title,
                    author,
                }));
                measuring = true;
                continue;
            }
            let Some(id) = self.model.book_at(&path) else {
                continue;
            };
            let position = Position {
                page,
                pages: reading.document.page_count(),
                place: place.map(|a| (a.block, a.offset)),
            };
            self.model.record_position(id, position, now);
        }
        self.seen.retain(|p, _| panes.contains(p));
        measuring
    }

    /// Writes changes at most every [`SAVE_DELAY`]; else the wait left.
    pub fn save(&mut self, now: f64) -> Option<f64> {
        let (Some(folder), Status::Ready) = (&self.folder, self.status) else {
            return None;
        };
        if !self.model.is_dirty() {
            return None;
        }
        let wait = self.wrote + SAVE_DELAY - now;
        if wait > 0.0 {
            return Some(wait);
        }
        let dirty = self.model.take_dirty();
        self.io.send(Job::Write(folder.writes(&self.model, &dirty)));
        self.wrote = now;
        None
    }

    /// Sends every pending change now: the app is saving or exiting.
    pub fn flush(&mut self) {
        self.wrote = f64::NEG_INFINITY;
        self.save(0.0);
    }

    /// Where `id`'s file opens, on its page; a toast if it's gone.
    pub fn open(&self, id: BookId) -> Result<OpenRequest, Toast> {
        let book = self
            .model
            .book(id)
            .ok_or_else(|| Toast::error("That book was removed."))?;
        let Some(path) = book.path() else {
            let title = &book.title.value;
            return Err(Toast::error(format!("“{title}” isn't where it was.")));
        };
        let position = self.model.position(id);
        Ok(OpenRequest {
            page: position.map_or(0, |p| p.page),
            place: position
                .and_then(|p| p.place)
                .map(|(block, offset)| Anchor { block, offset }),
            ..OpenRequest::path(path.to_path_buf())
        })
    }

    /// Made-up books for shots: some started, one missing.
    pub fn seed(&mut self, count: usize) {
        const BOOKS: [(&str, Option<&str>); 10] = [
            ("The Raven", Some("Edgar Allan Poe")),
            ("Middlemarch", Some("George Eliot")),
            ("A Field Guide to Ferns", None),
            ("Notes on Typography", Some("Ada Bringhurst")),
            ("The Voyage Out", Some("Virginia Woolf")),
            ("Collected Essays, Volume 10", Some("M. Aurel")),
            ("Letters from the Coast", None),
            ("An Atlas of Rivers", Some("Lena Ortiz")),
            ("Collected Essays, Volume 2", Some("M. Aurel")),
            ("Practical Rust", Some("Sam Okafor")),
        ];
        let formats = [Format::Epub, Format::Pdf, Format::Markdown, Format::Text];
        let now = wall_ms();
        for i in 0..count {
            let (title, author) = BOOKS
                .get(i % BOOKS.len())
                .copied()
                .unwrap_or(("Book", None));
            let format = formats
                .get(i % formats.len())
                .copied()
                .unwrap_or(Format::Pdf);
            let opened = Opened {
                path: PathBuf::from(format!("/books/{i}-{title}")),
                measure: mareader_library::Measure {
                    content: mareader_library::ContentId(i as u128 + 1),
                    stamp: Default::default(),
                },
                format,
                title: title.to_owned(),
                author: author.map(str::to_owned),
            };
            let at = now.saturating_sub(i as u64 * 7_000_000);
            let id = self.model.record_open(opened, at);
            if i % 3 != 2 {
                let pages = 40 + 37 * i;
                let position = Position {
                    page: pages * (i + 1) / (count + 2),
                    pages,
                    place: None,
                };
                self.model.record_position(id, position, at);
            }
        }
        // The fourth seeded book has lost its file.
        self.model
            .check_present(|p| !p.to_string_lossy().starts_with("/books/3-"));
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

/// A book's own title, else the file's name without its extension.
fn title_of(document: &Document, path: &Path) -> String {
    let own = match document {
        Document::Epub { book, .. } => book.title(),
        Document::Pdf(_) => None,
    };
    let stem = || path.file_stem().map(|s| s.to_string_lossy().into_owned());
    let own = own
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    own.or_else(stem).unwrap_or_else(|| "Untitled".into())
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

/// Random per library: the first launch keeps it in `library.json`.
fn new_device() -> DeviceId {
    DeviceId(RandomState::new().hash_one(wall_ms()))
}

/// Milliseconds since 1970; the browser preview has no clock here.
pub fn wall_ms() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        since.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
    #[cfg(target_arch = "wasm32")]
    0
}
