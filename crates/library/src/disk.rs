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

/// The files' version; a newer one is left untouched.
pub const SCHEMA: u32 = 1;

/// Bytes hashed from each end of a large PDF or EPUB.
const SAMPLE: u64 = 64 << 10;

#[derive(Serialize, Deserialize)]
struct LibraryFile {
    schema: u32,
    clock: Clock,
    books: Vec<Book>,
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
        let states = self.load_states(&books);
        let mut library = Library::from_parts(file.clock, books, states);
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

/// SHA-256 of the size and both 64 KiB ends; text is hashed whole.
pub fn measure(path: &Path, format: Format) -> io::Result<Measure> {
    let mut file = File::open(path)?;
    let meta = file.metadata()?;
    let size = meta.len();
    let since = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    let mtime_ms = since.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
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
        stamp: FileStamp { size, mtime_ms },
    })
}
