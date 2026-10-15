//! The library's files and measuring; a crash never leaves half a file.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::book::{Book, FileStamp, Format, State};
use crate::clock::Clock;
use crate::id::{BookId, ContentId, DeviceId};
use crate::library::{Dirty, Library, Measure};
use crate::shelves::{Membership, Shelf};
use crate::sources::Source;

/// The files' version; 2 added folders, 3 shelves. Newer files stay untouched.
pub const SCHEMA: u32 = 3;

/// Bytes hashed from each end of a large PDF or EPUB.
const SAMPLE: u64 = 64 << 10;

#[derive(Serialize, Deserialize)]
struct LibraryFile {
    schema: u32,
    clock: Clock,
    books: Vec<Book>,
    #[serde(default)]
    sources: Vec<Source>,
    #[serde(default)]
    shelves: Vec<Shelf>,
    #[serde(default)]
    members: Vec<Membership>,
}

#[derive(Serialize, Deserialize)]
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

    fn states(&self) -> PathBuf {
        self.root.join("state")
    }

    fn state_file(&self, id: BookId) -> PathBuf {
        self.states().join(format!("{id}.json"))
    }

    /// Reads the library, else starts one; unreadable files are kept aside.
    pub fn load(&self, device: DeviceId) -> Loaded {
        let mut notes = Vec::new();
        let fresh = |notes| Loaded {
            library: Library::new(device),
            notes,
            writable: true,
        };
        let text = match fs::read_to_string(self.library_file()) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return fresh(notes),
            Err(e) => {
                notes.push(format!("Couldn't read the library: {e}"));
                return Loaded {
                    writable: false,
                    ..fresh(notes)
                };
            }
        };
        let file = match serde_json::from_str::<LibraryFile>(&text) {
            Ok(file) if file.schema > SCHEMA => {
                notes.push("A newer Mareader saved this library; it's left untouched.".into());
                return Loaded {
                    writable: false,
                    ..fresh(notes)
                };
            }
            Ok(file) => file,
            Err(e) => {
                let aside = self.root.join("library.unreadable.json");
                let kept = fs::rename(self.library_file(), &aside).is_ok();
                let note = match kept {
                    true => format!("The library couldn't be read ({e}); it was kept aside."),
                    false => format!("The library couldn't be read ({e})."),
                };
                notes.push(note);
                return Loaded {
                    writable: kept,
                    ..fresh(notes)
                };
            }
        };
        let books: BTreeMap<BookId, Book> = file.books.into_iter().map(|b| (b.id, b)).collect();
        let sources = file.sources.into_iter().map(|s| (s.id, s)).collect();
        let states = self.load_states(&books);
        let mut library = Library::from_parts(file.clock, books, states, sources);
        library.shelves = file.shelves.into_iter().map(|s| (s.id, s)).collect();
        let members = file.members.into_iter();
        library.members = members.map(|m| ((m.shelf, m.book), m)).collect();
        library.check_present(Path::is_file);
        Loaded {
            library,
            notes,
            writable: true,
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

    /// The files `dirty` names, with what each must now hold.
    pub fn writes(&self, library: &Library, dirty: &Dirty) -> Vec<(PathBuf, String)> {
        let mut out = Vec::new();
        if dirty.library {
            let file = LibraryFile {
                schema: SCHEMA,
                clock: library.clock,
                books: library.books.values().cloned().collect(),
                sources: library.sources.values().cloned().collect(),
                shelves: library.shelves.values().cloned().collect(),
                members: library.members.values().cloned().collect(),
            };
            if let Ok(json) = serde_json::to_string_pretty(&file) {
                out.push((self.library_file(), json));
            }
        }
        for id in &dirty.states {
            let Some(state) = library.states.get(id) else {
                continue;
            };
            let file = StateFile {
                schema: SCHEMA,
                book: *id,
                state: state.clone(),
            };
            if let Ok(json) = serde_json::to_string_pretty(&file) {
                out.push((self.state_file(*id), json));
            }
        }
        out
    }
}

/// Writes `bytes` to `path` whole: a temp file beside it, then a rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let temp = path.with_file_name(name);
    let mut file = File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
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

/// Measures what a walk found, re-hashing only files that changed.
pub fn measure_all(
    files: Vec<(PathBuf, Format, FileStamp)>,
    known: &Known,
) -> Vec<(PathBuf, Format, Measure)> {
    let mut out = Vec::with_capacity(files.len());
    for (path, format, stamp) in files {
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
