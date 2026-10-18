//! A folder the library takes books from: its options, rungs and ledger.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::book::Format;
use crate::id::{BookId, ContentId, FolderId, ShelfId};
use crate::library::Measure;
use crate::words::{name_or_stem, stem};

/// The size floor a new folder starts on.
pub const MIN_SIZE_DEFAULT: u64 = 30 * 1024;
/// One press of the size dial.
pub const MIN_SIZE_STEP: u64 = 10 * 1024;
/// The dial's ceiling; loads clamp back inside it.
pub const MIN_SIZE_MAX: u64 = 500 * 1024;

/// How a folder holds its books: the import sheet's three answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Every admitted file is copied into the library's own store.
    Copy,
    /// Books are read where they are; later files are not picked up.
    InPlace,
    /// Read in place, and walked again on open and on focus.
    Watched,
}

impl Mode {
    pub const ALL: [Self; 3] = [Self::Copy, Self::InPlace, Self::Watched];

    pub fn copies(self) -> bool {
        self == Self::Copy
    }

    pub fn in_place(self) -> bool {
        !self.copies()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copy into the library",
            Self::InPlace => "Read at place",
            Self::Watched => "Read at place, and watch for new books",
        }
    }

    /// The badge a shelf of this folder wears.
    pub fn badge(self) -> &'static str {
        match self {
            Self::Copy => "Copied",
            Self::InPlace | Self::Watched => "On disk",
        }
    }
}

/// What counts in a folder and how it lands; honoured on every walk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub formats: BTreeSet<Format>,
    /// The formats are the ones taken; off, they are the ones left.
    pub include: bool,
    /// Only files strictly larger than this count.
    pub min_size: u64,
    pub in_place: bool,
    pub watch: bool,
    /// A shelf for each subfolder, else one shelf for everything.
    pub groups: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            formats: Format::ALL.into_iter().collect(),
            include: true,
            min_size: MIN_SIZE_DEFAULT,
            in_place: true,
            watch: false,
            groups: true,
        }
    }
}

impl Options {
    /// A watching copy is a copy.
    pub fn mode(&self) -> Mode {
        match (self.in_place, self.watch) {
            (false, _) => Mode::Copy,
            (true, false) => Mode::InPlace,
            (true, true) => Mode::Watched,
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.in_place = mode.in_place();
        self.watch = mode == Mode::Watched;
    }

    pub fn step_min_size(&mut self, steps: i64) {
        let next = self.min_size as i64 + steps * MIN_SIZE_STEP as i64;
        self.min_size = next.clamp(0, MIN_SIZE_MAX as i64) as u64;
    }

    pub fn min_size_label(&self) -> String {
        format!("{} KB", self.min_size / 1024)
    }

    /// Whether a file of `format` and `size` counts as a book here.
    pub fn admits(&self, format: Format, size: u64) -> bool {
        self.include == self.formats.contains(&format) && size > self.min_size
    }

    /// An empty selection is no folder at all: back to the defaults.
    fn sanitize(&mut self) {
        self.min_size = self.min_size.min(MIN_SIZE_MAX);
        if self.formats.is_empty() {
            self.formats = Format::ALL.into_iter().collect();
            self.include = true;
        }
    }
}

/// `path` under `root` as a rung key: `""` is the root, `a/b` below.
pub fn rel_under(path: &Path, root: &Path) -> Option<String> {
    let rest = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rest
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(parts.join("/"))
}

/// The rung key of the folder a file stands in.
pub fn subfolder_key(file: &Path, root: &Path) -> Option<String> {
    rel_under(file.parent()?, root)
}

/// Every rung of a key, the root first and the key itself last.
pub fn key_chain(key: &str) -> Vec<&str> {
    let mut out = vec![""];
    if key.is_empty() {
        return out;
    }
    out.extend(key.match_indices('/').filter_map(|(at, _)| key.get(..at)));
    out.push(key);
    out
}

/// The rung a key stands inside; the root stands inside nothing.
pub fn parent_key(key: &str) -> Option<&str> {
    if key.is_empty() {
        return None;
    }
    Some(key.rfind('/').and_then(|at| key.get(..at)).unwrap_or(""))
}

/// Whether `key` is `zone` or below it; the empty zone is the tree.
pub fn key_in_zone(key: &str, zone: &str) -> bool {
    zone.is_empty()
        || key == zone
        || key
            .strip_prefix(zone)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The directory a rung names under `root`.
pub fn dir_of_rung(root: &Path, key: &str) -> PathBuf {
    key.split('/')
        .filter(|part| !part.is_empty())
        .fold(root.to_path_buf(), |dir, part| dir.join(part))
}

/// Per-rung answers, inherited downward; the deepest answer wins.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rungs(BTreeMap<String, bool>);

impl Rungs {
    /// The deepest answer on `key`'s chain, if any rung gave one.
    pub fn at(&self, key: &str) -> Option<bool> {
        key_chain(key)
            .into_iter()
            .rev()
            .find_map(|rung| self.0.get(rung).copied())
    }

    pub fn set(&mut self, key: &str, on: bool) {
        self.0.insert(key.to_string(), on);
    }

    /// Back to inheriting: the rung's own answer goes.
    pub fn inherit(&mut self, key: &str) {
        self.0.remove(key);
    }

    /// Drop every answer in `zone` and below it.
    pub fn clear_zone(&mut self, zone: &str) {
        self.0.retain(|key, _| !key_in_zone(key, zone));
    }

    pub fn any_on(&self) -> bool {
        self.0.values().any(|on| *on)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A file a walk found and measured, and what it says of itself.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    pub format: Format,
    pub measure: Measure,
    /// Read only for content the library hasn't met.
    pub title: Option<String>,
    pub author: Option<String>,
}

/// A removal or a move-out, remembered by the folder that placed the book.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    pub content: ContentId,
    /// The removed row; a restore brings it, and what was kept, back.
    pub book: BookId,
    /// The name the shelf showed.
    #[serde(default)]
    pub title: String,
    pub format: Format,
    /// A restore looks here first.
    pub last_path: PathBuf,
    /// A restore lands the book here, else on the folder's root shelf.
    #[serde(default)]
    pub shelf: Option<ShelfId>,
    #[serde(default)]
    pub removed_ms: u64,
    /// The book left as the library's own copy; it was not removed.
    #[serde(default)]
    pub moved: bool,
    /// The row that came home for this file, while it stands.
    #[serde(default)]
    pub returned: Option<BookId>,
    /// The file's size when it left; zero when never measured.
    #[serde(default)]
    pub size: u64,
}

impl Tombstone {
    pub fn label(&self) -> String {
        name_or_stem(&self.title, &self.last_path)
    }
}

/// A folder the library takes books from, with what it has done there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: FolderId,
    /// Never rewritten: a moved folder is a missing one.
    pub root: PathBuf,
    #[serde(default)]
    pub opts: Options,
    /// Contents this folder placed; a walk never places them twice.
    #[serde(default)]
    pub placed: BTreeSet<ContentId>,
    /// Removals and move-outs, so a walk stays quiet about them.
    #[serde(default)]
    pub ignored: Vec<Tombstone>,
    /// What the latest walk saw of the books it placed.
    #[serde(default)]
    pub last_seen: Vec<(ContentId, PathBuf)>,
    /// Rung key to the shelf cut for it, reused by every walk.
    #[serde(default)]
    pub shelf_map: BTreeMap<String, ShelfId>,
    /// Per-rung watching; the root's answer is `opts.watch`.
    #[serde(default)]
    pub tracking: Rungs,
    /// Per-rung shape; the root's answer is `opts.groups`.
    #[serde(default)]
    pub shapes: Rungs,
    #[serde(default)]
    pub added: u64,
}

impl Folder {
    /// A folder never walked: an empty ledger.
    pub fn new(id: FolderId, root: PathBuf, opts: Options, added: u64) -> Self {
        let mut folder = Self {
            id,
            root,
            opts,
            placed: BTreeSet::new(),
            ignored: Vec::new(),
            last_seen: Vec::new(),
            shelf_map: BTreeMap::new(),
            tracking: Rungs::default(),
            shapes: Rungs::default(),
            added,
        };
        folder.sanitize();
        folder
    }

    /// The folder's own name, as its root shelf is first called.
    pub fn name(&self) -> String {
        stem_or_root(&self.root)
    }

    /// A rung's name: its last directory, the root's is the folder's.
    pub fn rung_name(&self, key: &str) -> String {
        match key.rsplit('/').next() {
            Some(last) if !last.is_empty() => last.to_string(),
            _ => self.name(),
        }
    }

    pub fn mode(&self) -> Mode {
        self.opts.mode()
    }

    /// The shape at `key`: the deepest rung that answered, else the root's.
    pub fn shape_at(&self, key: &str) -> bool {
        self.shapes.at(key).unwrap_or(self.opts.groups)
    }

    /// Record a rung's shape; the root's answer stands for the tree.
    pub fn set_shape(&mut self, key: &str, groups: bool) {
        if key.is_empty() {
            self.opts.groups = groups;
            self.shapes.clear_zone("");
        } else {
            self.shapes.set(key, groups);
        }
    }

    /// Whether a rung is cut into a shelf: the root always is.
    pub fn cuts(&self, key: &str) -> bool {
        key.is_empty() || self.shape_at(key)
    }

    /// The rung a directory's files land on: its nearest cut rung.
    pub fn rung_for(&self, key: &str) -> String {
        key_chain(key)
            .into_iter()
            .rev()
            .find(|rung| self.cuts(rung))
            .unwrap_or_default()
            .to_string()
    }

    /// The rung of the file at `path`, if the path is in this tree.
    pub fn rung_of(&self, path: &Path) -> Option<String> {
        subfolder_key(path, &self.root).map(|key| self.rung_for(&key))
    }

    /// The shelves this tree names for `path`: its rung's and the root's.
    pub fn rungs_for(&self, path: &Path) -> (Option<ShelfId>, Option<ShelfId>) {
        let Some(key) = self.rung_of(path) else {
            return (None, None);
        };
        (
            self.shelf_map.get(&key).copied(),
            self.shelf_map.get("").copied(),
        )
    }

    /// Whether a rung is watched: its own answer, else its nearest one.
    pub fn tracks_rung(&self, key: &str) -> bool {
        self.mode().in_place() && self.tracking.at(key).unwrap_or(false)
    }

    /// Whether a walk is owed on open and focus.
    pub fn owes_walk(&self) -> bool {
        self.mode().in_place() && self.tracking.any_on()
    }

    /// Watch one rung or stop; the root's answer mirrors onto the flag.
    pub fn set_tracking(&mut self, key: &str, on: bool) {
        if key.is_empty() {
            self.tracking.clear_zone("");
        }
        self.tracking.set(key, on);
        self.opts.watch = self.tracking.at("").unwrap_or(false);
    }

    pub fn is_ignored(&self, content: ContentId) -> bool {
        self.ignored.iter().any(|stone| stone.content == content)
    }

    pub fn tombstone(&self, content: ContentId) -> Option<&Tombstone> {
        self.ignored.iter().find(|stone| stone.content == content)
    }

    /// Take a tombstone off; the import that lands marks the file placed.
    pub fn lift(&mut self, content: ContentId) -> Option<Tombstone> {
        let at = self.ignored.iter().position(|s| s.content == content)?;
        Some(self.ignored.remove(at))
    }

    /// Remember what a walk saw of the books this folder placed.
    pub fn record_seen<'a>(&mut self, seen: impl IntoIterator<Item = (ContentId, &'a Path)>) {
        self.last_seen = seen
            .into_iter()
            .filter(|(content, _)| self.placed.contains(content))
            .map(|(content, path)| (content, path.to_path_buf()))
            .collect();
    }

    /// Clamp what a load or a newer Mareader could have left odd.
    pub fn sanitize(&mut self) {
        self.opts.sanitize();
        self.shelf_map = std::mem::take(&mut self.shelf_map)
            .into_iter()
            .map(|(key, id)| (key.replace('\\', "/"), id))
            .collect();
        if self.opts.watch && self.tracking.is_empty() {
            self.tracking.set("", true);
        }
        let mut seen = BTreeSet::new();
        self.ignored.retain(|stone| seen.insert(stone.content));
    }
}

/// A folder's own name, as its root shelf is first called.
pub fn root_name(root: &Path) -> String {
    stem_or_root(root)
}

fn stem_or_root(root: &Path) -> String {
    match root.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => stem(root),
    }
}
