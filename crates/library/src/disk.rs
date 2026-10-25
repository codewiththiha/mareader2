//! The library's files and measuring; a crash never leaves half a file.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::book::{Book, FileStamp, Format, State};
use crate::clock::Clock;
use crate::id::{BookId, ContentId, DeviceId};
use crate::library::{Library, Measure, Parts};
use crate::merge::Merge;
use crate::records;
use crate::shelves::{Membership, Shelf};
use crate::store::{OpenError, Store};

/// The state files' version, and the last `library.json` one.
/// Newer files stay untouched.
pub const SCHEMA: u32 = 4;

/// Bytes hashed from each end of a large PDF or EPUB.
const SAMPLE: u64 = 64 << 10;

const NEWER: &str = "A newer Mareader saved this library; it's left untouched.";

/// The library before SQLite; read once, then kept as `.migrated`.
#[derive(Deserialize)]
struct LibraryFile {
    schema: u32,
    clock: Clock,
    books: Vec<Book>,
    #[serde(default)]
    sources: Vec<crate::legacy::OldFolder>,
    #[serde(default)]
    shelves: Vec<Shelf>,
    #[serde(default)]
    members: Vec<Membership>,
}

/// Just the version, read when the rest won't parse.
#[derive(Deserialize)]
struct Probe {
    schema: u32,
}

/// A position before SQLite; read once, then the folder is renamed.
#[derive(Deserialize)]
struct StateFile {
    schema: u32,
    book: BookId,
    state: State,
}

/// A library read from its folder, and what went wrong on the way.
#[derive(Debug)]
pub struct Loaded {
    pub library: Library,
    /// Problems worth telling the reader, in their words.
    pub notes: Vec<String>,
    /// False when a newer Mareader wrote the files: they stay as they are.
    pub writable: bool,
    /// The open file saves go to; None when nothing may be written.
    pub store: Option<Store>,
}

/// What `library.json` held, if there is one to move into the store.
enum Legacy {
    Absent,
    Read(Box<Parts>),
    /// It can't be moved: the library opens empty and read only.
    Stop,
}

/// The folder a library lives in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    root: PathBuf,
}

impl Folder {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn library_file(&self) -> PathBuf {
        self.root.join("library.json")
    }

    fn store_file(&self) -> PathBuf {
        self.root.join("library.sqlite")
    }

    /// Where imported books are copied: `files/<content>/<name>`.
    pub fn store(&self) -> PathBuf {
        self.root.join("files")
    }

    fn states(&self) -> PathBuf {
        self.root.join("state")
    }

    /// Reads the library, else starts one; unreadable files are kept aside.
    pub fn load(&self, device: DeviceId) -> Loaded {
        let mut notes = Vec::new();
        let legacy = match self.store_file().exists() {
            true => Legacy::Absent,
            false => self.read_legacy(&mut notes),
        };
        let legacy = match legacy {
            Legacy::Stop => return fresh(device, notes, false),
            Legacy::Read(parts) => Some(*parts),
            Legacy::Absent => None,
        };
        let Some(store) = self.open_store(&mut notes) else {
            return fresh(device, notes, false);
        };
        let (mut library, store) = match legacy {
            Some(parts) => self.migrate(parts, store, &mut notes),
            None => match read(&store, device) {
                Ok((parts, skipped)) => {
                    if skipped > 0 {
                        let note =
                            format!("{skipped} saved records couldn't be read; they're kept.");
                        notes.push(note);
                    }
                    (Library::from_parts(parts), Some(store))
                }
                Err(e) => {
                    notes.push(format!("Couldn't read the library: {e}"));
                    return fresh(device, notes, false);
                }
            },
        };
        let mut store = store.filter(|s| {
            let newer = s.newer();
            if newer {
                notes.push(NEWER.into());
            }
            !newer
        });
        self.adopt_states(&mut library, store.as_mut(), &mut notes);
        // Opening a copy once marked it opened by hand; it's ours again.
        let files = fs::canonicalize(self.store()).unwrap_or_else(|_| self.store());
        library.reclaim_store(&files);
        library.check_present(Path::is_file);
        Loaded {
            library,
            notes,
            writable: store.is_some(),
            store,
        }
    }

    /// The store, opened; a damaged file is kept aside for a new one.
    fn open_store(&self, notes: &mut Vec<String>) -> Option<Store> {
        let path = self.store_file();
        let e = match Store::open(&path) {
            Ok(store) => return Some(store),
            Err(OpenError::Other(e)) => {
                notes.push(format!("Couldn't open the library: {e}"));
                return None;
            }
            Err(OpenError::Unreadable(e)) => e,
        };
        let aside = self.root.join("library.unreadable.sqlite");
        let kept = fs::rename(&path, aside).is_ok();
        for end in ["-wal", "-shm"] {
            let mut side = path.clone().into_os_string();
            side.push(end);
            let _ = fs::remove_file(side);
        }
        let note = match kept {
            true => format!("The library couldn't be read ({e}); it was kept aside."),
            false => format!("The library couldn't be read ({e})."),
        };
        notes.push(note);
        kept.then(|| Store::open(&path).ok()).flatten()
    }

    /// `library.json` into the store, then renamed so it's read once.
    fn migrate(
        &self,
        parts: Parts,
        mut store: Store,
        notes: &mut Vec<String>,
    ) -> (Library, Option<Store>) {
        let mut library = Library::from_parts(parts);
        library.mark_all();
        let dirty = library.take_dirty();
        match store.apply(&library.changes(&dirty)) {
            Ok(()) => {
                let mut done = self.library_file().into_os_string();
                done.push(".migrated");
                let _ = fs::rename(self.library_file(), done);
                (library, Some(store))
            }
            Err(e) => {
                notes.push(format!("Couldn't move the library to its new file: {e}"));
                (library, None)
            }
        }
    }

    /// The old library file, if there is one and this build can read it.
    fn read_legacy(&self, notes: &mut Vec<String>) -> Legacy {
        let text = match fs::read_to_string(self.library_file()) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Legacy::Absent,
            Err(e) => {
                notes.push(format!("Couldn't read the library: {e}"));
                return Legacy::Stop;
            }
        };
        let newer = |text: &str| {
            let probe = serde_json::from_str::<Probe>(text);
            probe.is_ok_and(|p| p.schema > SCHEMA)
        };
        let file = match serde_json::from_str::<LibraryFile>(&text) {
            // A newer file may not parse here at all.
            Err(_) if newer(&text) => None,
            Ok(file) if file.schema > SCHEMA => None,
            Ok(file) => Some(file),
            Err(e) => {
                let aside = self.root.join("library.unreadable.json");
                let kept = fs::rename(self.library_file(), &aside).is_ok();
                let note = match kept {
                    true => format!("The library couldn't be read ({e}); it was kept aside."),
                    false => format!("The library couldn't be read ({e})."),
                };
                notes.push(note);
                return match kept {
                    true => Legacy::Absent,
                    false => Legacy::Stop,
                };
            }
        };
        let Some(file) = file else {
            notes.push(NEWER.into());
            return Legacy::Stop;
        };
        let mut parts = Parts::new(file.clock);
        parts.books = file.books.into_iter().map(|b| (b.id, b)).collect();
        parts.legacy = file.sources;
        parts.shelves = file.shelves.into_iter().map(|s| (s.id, s)).collect();
        let members = file.members.into_iter();
        parts.members = members.map(|m| ((m.shelf, m.book), m)).collect();
        Legacy::Read(Box::new(parts))
    }

    /// Positions from `state/` files into the store, then the folder renamed.
    fn adopt_states(
        &self,
        library: &mut Library,
        store: Option<&mut Store>,
        notes: &mut Vec<String>,
    ) {
        let found = self.load_states(&library.books);
        if found.is_empty() {
            return;
        }
        for (id, state) in found {
            match library.states.get_mut(&id) {
                Some(kept) => kept.merge(state),
                None => {
                    library.states.insert(id, state);
                }
            }
            library.states_dirty.insert(id);
        }
        let Some(store) = store else {
            return;
        };
        let dirty = library.take_dirty();
        match store.apply(&library.changes(&dirty)) {
            Ok(()) => {
                let mut done = self.states().into_os_string();
                done.push(".migrated");
                let _ = fs::rename(self.states(), done);
            }
            Err(e) => notes.push(format!("Couldn't move reading places to the library: {e}")),
        }
    }

    /// Each book's state; an unreadable or orphaned file is skipped.
    fn load_states(&self, books: &BTreeMap<BookId, Book>) -> BTreeMap<BookId, State> {
        let Ok(entries) = fs::read_dir(self.states()) else {
            return BTreeMap::new();
        };
        let read = |path: PathBuf| {
            let text = fs::read_to_string(path).ok()?;
            let file = serde_json::from_str::<StateFile>(&text).ok()?;
            let known = file.schema <= SCHEMA && books.contains_key(&file.book);
            known.then_some((file.book, file.state))
        };
        let json = |p: &PathBuf| p.extension().is_some_and(|e| e == "json");
        let paths = entries.filter_map(|e| e.ok().map(|e| e.path()));
        paths.filter(json).filter_map(read).collect()
    }
}

/// An empty library; `writable` false when the folder must stay as is.
fn fresh(device: DeviceId, notes: Vec<String>, writable: bool) -> Loaded {
    Loaded {
        library: Library::new(device),
        notes,
        writable,
        store: None,
    }
}

/// The store's rows as a library, and how many couldn't be read.
fn read(store: &Store, device: DeviceId) -> Result<(Parts, usize), String> {
    let rows = store.rows()?;
    let clock = store.meta("clock")?;
    let clock = clock.and_then(|c| serde_json::from_str(&c).ok());
    Ok(records::assemble(rows, clock.unwrap_or(Clock::new(device))))
}

/// Writes `bytes` to `path` whole: a temp file beside it, then a rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let temp = path.with_file_name(name);
    // Closed before the rename, as Windows needs.
    {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// Copies `file` into the store under its content; a whole copy is kept.
pub fn copy_in(store: &Path, file: &Path, content: ContentId) -> io::Result<(PathBuf, FileStamp)> {
    let unnamed = || io::Error::new(io::ErrorKind::InvalidInput, "the file has no name");
    let name = file.file_name().ok_or_else(unnamed)?;
    let dir = store.join(content.to_string());
    fs::create_dir_all(&dir)?;
    let dest = dir.join(name);
    let size = fs::metadata(file)?.len();
    if !fs::metadata(&dest).is_ok_and(|m| m.len() == size) {
        let mut part = name.to_os_string();
        part.push(".part");
        let temp = dir.join(part);
        fs::copy(file, &temp)?;
        fs::rename(&temp, &dest).inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })?;
    }
    let dest = fs::canonicalize(&dest)?;
    let stamp = stamp_of(&fs::metadata(&dest)?);
    Ok((dest, stamp))
}

/// Copies each picture to where the `copy` keeps it, inside its folder.
pub fn copy_beside(copy: &Path, pictures: &[(PathBuf, PathBuf)]) {
    let Some(dir) = copy.parent() else {
        return;
    };
    let inside = |(_, kept): &&(PathBuf, PathBuf)| {
        kept.components().all(|c| matches!(c, Component::Normal(_)))
    };
    for (src, kept) in pictures.iter().filter(inside) {
        let dest = dir.join(kept);
        let Ok(size) = fs::metadata(src).map(|m| m.len()) else {
            continue;
        };
        if fs::metadata(&dest).is_ok_and(|m| m.len() == size) {
            continue;
        }
        let _ = dest.parent().map(fs::create_dir_all);
        let mut part = dest.clone().into_os_string();
        part.push(".part");
        let part = PathBuf::from(part);
        if fs::copy(src, &part)
            .and_then(|_| fs::rename(&part, &dest))
            .is_err()
        {
            let _ = fs::remove_file(&part);
        }
    }
}

/// Deletes store copies with their folders, pictures and all; others stay.
pub fn delete_copies(store: &Path, paths: &[PathBuf]) -> usize {
    let Ok(store) = fs::canonicalize(store) else {
        return 0;
    };
    let mut deleted = 0;
    for path in paths {
        let Some(dir) = path.parent().and_then(|d| fs::canonicalize(d).ok()) else {
            continue;
        };
        // Only `files/<content>/<name>`: never anything outside the store.
        if dir.parent() != Some(store.as_path()) || fs::remove_file(path).is_err() {
            continue;
        }
        deleted += 1;
        // The folder is this copy's alone: its name is the content's.
        let _ = fs::remove_dir_all(&dir);
    }
    deleted
}

/// A file's size and modification time.
fn stamp_of(meta: &fs::Metadata) -> FileStamp {
    let since = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    let mtime_ms = since.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    FileStamp {
        size: meta.len(),
        mtime_ms,
    }
}

/// SHA-256 of the size and both 64 KiB ends; text is hashed whole.
pub fn measure(path: &Path, format: Format) -> io::Result<Measure> {
    let mut file = File::open(path)?;
    let stamp = stamp_of(&file.metadata()?);
    let size = stamp.size;
    let mut hasher = Sha256::new();
    hasher.update(size.to_le_bytes());
    if format.hashed_whole() || size <= 2 * SAMPLE {
        io::copy(&mut file, &mut hasher)?;
    } else {
        let mut head = Vec::new();
        Read::by_ref(&mut file)
            .take(SAMPLE)
            .read_to_end(&mut head)?;
        hasher.update(&head);
        let mut tail = Vec::new();
        file.seek(SeekFrom::End(-(SAMPLE as i64)))?;
        file.read_to_end(&mut tail)?;
        hasher.update(&tail);
    }
    let digest = hasher.finalize();
    let first: [u8; 16] = digest
        .get(..16)
        .and_then(|b| b.try_into().ok())
        .unwrap_or_default();
    Ok(Measure {
        content: ContentId(u128::from_be_bytes(first)),
        stamp,
    })
}

/// How far a walk may go before it stops short.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub files: usize,
    pub folders: usize,
    pub depth: usize,
    /// Files changed this recently may still be copying in.
    pub settle_ms: u64,
    pub now_ms: u64,
}

impl Limits {
    pub fn new(now_ms: u64) -> Self {
        Self {
            files: 20_000,
            folders: 20_000,
            depth: 24,
            settle_ms: 3_000,
            now_ms,
        }
    }
}

/// What a walk found, and how it ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Walked {
    /// Sorted by path.
    pub files: Vec<(PathBuf, Format, FileStamp)>,
    /// Stopped at a limit: what wasn't seen may still be there.
    pub truncated: bool,
    /// Files skipped as still being written; worth another look soon.
    pub settling: usize,
    /// The folder itself couldn't be read.
    pub offline: bool,
}

/// Folders a walk never enters: system litter and build trees.
const SKIP: [&str; 6] = [
    "node_modules",
    "__MACOSX",
    "$RECYCLE.BIN",
    "System Volume Information",
    "@eaDir",
    "lost+found",
];

/// The book files under `root` that `keep` takes, never following links.
pub fn walk(
    root: &Path,
    keep: impl Fn(Format, u64) -> bool,
    limits: Limits,
    avoid: &[PathBuf],
) -> Walked {
    let mut walked = Walked::default();
    if !root.is_dir() {
        walked.offline = true;
        return walked;
    }
    let mut folders = vec![(root.to_path_buf(), 0)];
    let mut entered = 0;
    while let Some((dir, depth)) = folders.pop() {
        entered += 1;
        let Ok(entries) = fs::read_dir(&dir) else {
            walked.offline |= dir == root;
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Hidden files and Office's `~$` locks are never books.
            if name.starts_with('.') || name.starts_with("~$") {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                let skipped = SKIP.contains(&name.as_ref()) || avoid.contains(&path);
                if skipped {
                    continue;
                }
                if depth >= limits.depth || entered + folders.len() >= limits.folders {
                    walked.truncated = true;
                    continue;
                }
                folders.push((path, depth + 1));
                continue;
            }
            // Links are refused: they lead out of the folder, or round it.
            let Some(format) = kind.is_file().then(|| Format::of_path(&path)).flatten() else {
                continue;
            };
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let stamp = stamp_of(&meta);
            if !keep(format, stamp.size) {
                continue;
            }
            if limits.now_ms.saturating_sub(stamp.mtime_ms) < limits.settle_ms {
                walked.settling += 1;
                continue;
            }
            if walked.files.len() >= limits.files {
                walked.truncated = true;
                break;
            }
            walked.files.push((path, format, stamp));
        }
        if walked.files.len() >= limits.files && walked.truncated {
            break;
        }
    }
    walked.files.sort_by(|a, b| a.0.cmp(&b.0));
    walked
}

/// A content id remembered for a path, valid while its stamp holds.
pub type Known = std::collections::HashMap<PathBuf, (FileStamp, ContentId)>;

/// Measures a walk's files, re-hashing changed ones; `each` hears progress.
pub fn measure_all(
    files: Vec<(PathBuf, Format, FileStamp)>,
    known: &Known,
    mut each: impl FnMut(usize, &Path),
) -> Vec<(PathBuf, Format, Measure)> {
    let mut out = Vec::with_capacity(files.len());
    for (done, (path, format, stamp)) in files.into_iter().enumerate() {
        each(done, &path);
        let cached = known.get(&path).filter(|(s, _)| *s == stamp);
        let measured = match cached {
            Some(&(stamp, content)) => Some(Measure { content, stamp }),
            // A file gone mid-walk is simply not found.
            None => measure(&path, format).ok(),
        };
        out.extend(measured.map(|m| (path, format, m)));
    }
    out
}

/// What a book's lost file is sought by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sought {
    pub content: ContentId,
    pub format: Format,
    /// Its file's last name and size, when known.
    pub name: Option<std::ffi::OsString>,
    pub size: Option<u64>,
}

/// What a search for a lost file turned up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FoundAgain {
    /// The very content, under any name.
    Same(PathBuf, Measure),
    /// Its old name with other content: an edition or an edit.
    Named(PathBuf, Measure),
    Nothing,
}

/// Looks under `folder` for the content, else the old name.
pub fn find_again(folder: &Path, sought: &Sought, limits: Limits) -> FoundAgain {
    let limits = Limits {
        settle_ms: 0,
        ..limits
    };
    let walked = walk(folder, |f, _| f == sought.format, limits, &[]);
    let mut named = None;
    for (path, format, stamp) in walked.files {
        let same_name = sought.name.is_some() && path.file_name() == sought.name.as_deref();
        let same_size = sought.size == Some(stamp.size);
        if !same_name && !same_size {
            continue;
        }
        let Ok(m) = measure(&path, format) else {
            continue;
        };
        if m.content == sought.content {
            return FoundAgain::Same(path, m);
        }
        if same_name && named.is_none() {
            named = Some((path, m));
        }
    }
    named.map_or(FoundAgain::Nothing, |(p, m)| FoundAgain::Named(p, m))
}
