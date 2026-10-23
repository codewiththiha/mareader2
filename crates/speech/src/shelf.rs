//! Voice models on disk, and downloads that run one at a time.

use std::collections::{HashMap, VecDeque};

use crate::models::{MODELS, Model, ModelFile};

/// One download: a model's own files, or one of its voices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    Model(&'static str),
    /// A model's id, then the voice's.
    Voice(&'static str, &'static str),
}

impl Item {
    pub fn model(self) -> &'static str {
        match self {
            Self::Model(m) | Self::Voice(m, _) => m,
        }
    }
}

/// Where a download stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Absent,
    /// Queued behind another download.
    Waiting,
    Fetching {
        got: u64,
        total: u64,
    },
    Paused {
        got: u64,
        total: u64,
    },
    /// Whole, its checksum being read.
    Checking,
    Ready,
    Failed(String),
}

impl Stage {
    /// Running or about to.
    pub fn busy(&self) -> bool {
        matches!(self, Self::Waiting | Self::Fetching { .. } | Self::Checking)
    }
}

const ABSENT: Stage = Stage::Absent;

/// The models on offer and the downloads in hand.
pub struct Shelf {
    catalogue: &'static [Model],
    stages: HashMap<Item, Stage>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    queue: VecDeque<Item>,
    #[cfg(not(target_arch = "wasm32"))]
    desk: Option<desk::Desk>,
    #[cfg(not(target_arch = "wasm32"))]
    studio: crate::studio::Studio,
}

impl Default for Shelf {
    fn default() -> Self {
        Self {
            catalogue: MODELS,
            stages: HashMap::new(),
            queue: VecDeque::new(),
            #[cfg(not(target_arch = "wasm32"))]
            desk: None,
            #[cfg(not(target_arch = "wasm32"))]
            studio: crate::studio::Studio::default(),
        }
    }
}

impl Shelf {
    pub fn catalogue(&self) -> &'static [Model] {
        self.catalogue
    }

    pub fn model(&self, id: &str) -> Option<&'static Model> {
        self.catalogue.iter().find(|m| m.id == id)
    }

    /// Keeps at most `bytes` of speech the models made, to say again.
    pub fn set_speech_memory(&self, bytes: usize) {
        #[cfg(not(target_arch = "wasm32"))]
        self.studio.set_memory(bytes);
        #[cfg(target_arch = "wasm32")]
        let _ = bytes;
    }

    /// Whether downloads can happen here.
    pub fn attached(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.desk.is_some();
        #[cfg(target_arch = "wasm32")]
        false
    }

    pub fn stage(&self, item: Item) -> &Stage {
        self.stages.get(&item).unwrap_or(&ABSENT)
    }

    /// The models downloaded whole, in offer order.
    pub fn ready(&self) -> Vec<&'static Model> {
        let ready = |m: &&Model| *self.stage(Item::Model(m.id)) == Stage::Ready;
        self.catalogue.iter().filter(ready).collect()
    }

    /// Whether `model` can speak in `voice` now.
    pub fn has(&self, model: &Model, voice: &str) -> bool {
        if *self.stage(Item::Model(model.id)) != Stage::Ready {
            return false;
        }
        let own = |v: &&crate::ModelVoice| *self.stage(Item::Voice(model.id, v.id)) == Stage::Ready;
        voice == model.first_voice || model.voice(voice).as_ref().is_some_and(own)
    }

    /// The bytes `model` keeps on disk once downloaded.
    pub fn on_disk(&self, model: &Model) -> u64 {
        if *self.stage(Item::Model(model.id)) != Stage::Ready {
            return 0;
        }
        let extra = model.voices.iter().filter(|v| v.id != model.first_voice);
        let extra = extra.filter(|v| *self.stage(Item::Voice(model.id, v.id)) == Stage::Ready);
        model.bytes() + extra.map(|v| v.file.bytes).sum::<u64>()
    }

    /// Takes in what downloads sent; true if anything changed.
    pub fn poll(&mut self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(desk) = &mut self.desk else {
                return false;
            };
            let (events, ended) = desk.poll();
            let changed = !events.is_empty();
            for (item, stage) in events {
                self.stages.insert(item, stage);
            }
            if let Some(model) = ended {
                self.forget(model);
            }
            self.pump();
            changed
        }
        #[cfg(target_arch = "wasm32")]
        false
    }

    /// Each item of `model`: its own files, then each extra voice.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn items(model: &'static Model) -> impl Iterator<Item = Item> {
        let voices = model.voices.iter().filter(|v| v.id != model.first_voice);
        std::iter::once(Item::Model(model.id)).chain(voices.map(|v| Item::Voice(model.id, v.id)))
    }

    /// Every stage of `model` back to absent.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn forget(&mut self, model: &'static str) {
        self.stages.retain(|item, _| item.model() != model);
        self.queue.retain(|item| item.model() != model);
    }

    /// Starts the next waiting download if none runs.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn pump(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        while self.desk.as_ref().is_some_and(|d| d.current().is_none()) {
            let Some(item) = self.queue.pop_front() else {
                return;
            };
            let Some((model, files)) = self.files(item) else {
                continue;
            };
            if let Some(desk) = &mut self.desk {
                let got = desk.found(model, &files);
                let total = files.iter().map(|f| f.bytes).sum();
                let got = match got {
                    Stage::Paused { got, .. } => got,
                    _ => 0,
                };
                desk.start(item, model, files);
                self.stages.insert(item, Stage::Fetching { got, total });
            }
        }
    }

    /// What `item` downloads, and from which model.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn files(&self, item: Item) -> Option<(&'static Model, Vec<ModelFile>)> {
        let model = self.model(item.model())?;
        match item {
            Item::Model(_) => Some((model, model.core())),
            Item::Voice(_, v) => Some((model, vec![model.voice(v)?.file])),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Shelf {
    /// Keeps the models of `catalogue` in `dir`, made if missing.
    pub fn attach(
        &mut self,
        dir: std::path::PathBuf,
        catalogue: &'static [Model],
        waker: crate::Waker,
    ) -> Result<(), String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        self.catalogue = catalogue;
        self.stages.clear();
        self.queue.clear();
        let desk = desk::Desk::new(dir, waker);
        for model in catalogue {
            for item in Self::items(model) {
                if let Some((model, files)) = self.files(item) {
                    self.stages.insert(item, desk.found(model, &files));
                }
            }
        }
        self.desk = Some(desk);
        Ok(())
    }

    /// Queues `item`, or resumes it; a voice waits for its model.
    pub fn download(&mut self, item: Item) {
        if let Item::Voice(m, _) = item
            && *self.stage(Item::Model(m)) != Stage::Ready
        {
            return;
        }
        if self.files(item).is_none() || self.desk.is_none() {
            return;
        }
        match self.stage(item) {
            Stage::Absent | Stage::Paused { .. } | Stage::Failed(_) => {
                self.stages.insert(item, Stage::Waiting);
                self.queue.push_back(item);
                self.pump();
            }
            _ => {}
        }
    }

    /// Stops `item`, keeping what came to resume later.
    pub fn pause(&mut self, item: Item) {
        let Some(desk) = &mut self.desk else { return };
        if desk.current() == Some(item) {
            desk.halt(mareader_fetch::Halt::Pause);
        } else if *self.stage(item) == Stage::Waiting {
            self.queue.retain(|i| *i != item);
            if let Some((model, files)) = self.files(item)
                && let Some(desk) = &self.desk
            {
                self.stages.insert(item, desk.found(model, &files));
            }
        }
    }

    /// Stops `item` and drops what came of it.
    pub fn cancel(&mut self, item: Item) {
        let Some(desk) = &mut self.desk else { return };
        if desk.current() == Some(item) {
            desk.halt(mareader_fetch::Halt::Cancel);
            return;
        }
        self.queue.retain(|i| *i != item);
        if *self.stage(item) == Stage::Ready {
            return;
        }
        if let Some((model, files)) = self.files(item)
            && let Some(desk) = &self.desk
        {
            desk.drop_files(model, &files);
        }
        self.stages.insert(item, Stage::Absent);
    }

    /// Deletes `item`: a model goes with all its voices.
    pub fn remove(&mut self, item: Item) {
        let Some(desk) = &mut self.desk else { return };
        match item {
            Item::Model(m) => {
                let running = desk.current().is_some_and(|i| i.model() == m);
                if running {
                    // Deleted once the download has let go of its files.
                    desk.doom(m);
                } else {
                    desk.drop_model(m);
                }
                self.forget(m);
            }
            Item::Voice(..) => {
                if desk.current() == Some(item) {
                    desk.halt(mareader_fetch::Halt::Cancel);
                    return;
                }
                self.queue.retain(|i| *i != item);
                if let Some((model, files)) = self.files(item)
                    && let Some(desk) = &self.desk
                {
                    desk.drop_files(model, &files);
                }
                self.stages.insert(item, Stage::Absent);
            }
        }
    }

    /// The models loaded and the speech they made.
    pub(crate) fn studio(&self) -> &crate::studio::Studio {
        &self.studio
    }

    /// Where `model` is kept, once attached.
    pub fn folder(&self, model: &Model) -> Option<std::path::PathBuf> {
        self.desk.as_ref().map(|d| d.folder(model))
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod desk {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::{Duration, Instant};

    use mareader_fetch::{Halt, Kind, Stopped, fetch};

    use super::{Item, Stage};
    use crate::models::{Model, ModelFile};

    /// Progress is told this often at most.
    const TELL_EVERY: Duration = Duration::from_millis(100);

    type Event = (Item, Stage);

    pub struct Desk {
        dir: PathBuf,
        waker: crate::Waker,
        tell: Sender<Event>,
        events: Receiver<Event>,
        current: Option<(Item, Arc<AtomicU8>)>,
        /// A model to delete once its running download stops.
        doomed: Option<&'static str>,
    }

    impl Desk {
        pub fn new(dir: PathBuf, waker: crate::Waker) -> Self {
            let (tell, events) = channel();
            Self {
                dir,
                waker,
                tell,
                events,
                current: None,
                doomed: None,
            }
        }

        pub fn folder(&self, model: &Model) -> PathBuf {
            self.dir.join(model.id)
        }

        pub fn current(&self) -> Option<Item> {
            self.current.as_ref().map(|(item, _)| *item)
        }

        pub fn halt(&self, halt: Halt) {
            if let Some((_, flag)) = &self.current {
                flag.store(halt as u8, Ordering::Relaxed);
            }
        }

        /// Stops the running download; its model goes when it has.
        pub fn doom(&mut self, model: &'static str) {
            self.halt(Halt::Cancel);
            self.doomed = Some(model);
        }

        /// How much of `files` is on disk.
        pub fn found(&self, model: &Model, files: &[ModelFile]) -> Stage {
            found(&self.folder(model), files)
        }

        pub fn drop_files(&self, model: &Model, files: &[ModelFile]) {
            drop_files(&self.folder(model), files);
        }

        pub fn drop_model(&self, model: &str) {
            let _ = std::fs::remove_dir_all(self.dir.join(model));
        }

        /// What came, and a doomed model's id once deleted.
        pub fn poll(&mut self) -> (Vec<Event>, Option<&'static str>) {
            let mut events: Vec<Event> = self.events.try_iter().collect();
            let ended = events.iter().any(|(item, stage)| {
                self.current() == Some(*item)
                    && !matches!(stage, Stage::Fetching { .. } | Stage::Checking)
            });
            // A doomed model's news is stale: it is already forgotten.
            if let Some(model) = self.doomed {
                events.retain(|(item, _)| item.model() != model);
            }
            if !ended {
                return (events, None);
            }
            self.current = None;
            let doomed = self.doomed.take();
            if let Some(model) = doomed {
                self.drop_model(model);
            }
            (events, doomed)
        }

        /// Fetches `files` of `model` on a thread of its own.
        pub fn start(&mut self, item: Item, model: &'static Model, files: Vec<ModelFile>) {
            let flag = Arc::new(AtomicU8::new(Halt::Go as u8));
            self.current = Some((item, Arc::clone(&flag)));
            let folder = self.folder(model);
            let (tell, waker) = (self.tell.clone(), Arc::clone(&self.waker));
            std::thread::spawn(move || {
                let say = |stage: Stage| {
                    let _ = tell.send((item, stage));
                    waker();
                };
                let stage = run(model, (&folder, &files), &flag, &say);
                say(stage);
            });
        }
    }

    /// Each file not yet whole, in turn; the stage it ends at.
    fn run(
        model: &Model,
        (folder, files): (&Path, &[ModelFile]),
        flag: &AtomicU8,
        say: &dyn Fn(Stage),
    ) -> Stage {
        let total: u64 = files.iter().map(|f| f.bytes).sum();
        let mut base = 0;
        let now = Instant::now();
        let mut last = now.checked_sub(TELL_EVERY).unwrap_or(now);
        for f in files {
            let path = folder.join(f.path);
            if len(&path) == Some(f.bytes) {
                base += f.bytes;
                continue;
            }
            if let Some(dir) = path.parent()
                && let Err(e) = std::fs::create_dir_all(dir)
            {
                return Stage::Failed(e.to_string());
            }
            let part = part(&path);
            let kind = Kind::Sha256 {
                sha256: f.sha256,
                bytes: f.bytes,
            };
            let mut progress = |got: u64, _| {
                let got = base + got;
                if got >= total {
                    say(Stage::Checking);
                } else if last.elapsed() >= TELL_EVERY {
                    last = Instant::now();
                    say(Stage::Fetching { got, total });
                }
            };
            match fetch(&model.urls(f), &part, kind, flag, &mut progress) {
                Ok(()) => {
                    if let Err(e) = std::fs::rename(&part, &path) {
                        return Stage::Failed(e.to_string());
                    }
                }
                Err(Stopped::Halted(Halt::Pause)) => {
                    let got = base + len(&part).unwrap_or(0);
                    return Stage::Paused { got, total };
                }
                Err(Stopped::Halted(_)) => {
                    drop_files(folder, files);
                    return Stage::Absent;
                }
                Err(Stopped::Failed(why)) => return Stage::Failed(why),
            }
            base += f.bytes;
        }
        Stage::Ready
    }

    /// Whole when every file is its listed size; else what is here.
    fn found(folder: &Path, files: &[ModelFile]) -> Stage {
        let total: u64 = files.iter().map(|f| f.bytes).sum();
        let (mut got, mut whole, mut any) = (0, true, false);
        for f in files {
            let path = folder.join(f.path);
            if len(&path) == Some(f.bytes) {
                (got, any) = (got + f.bytes, true);
                continue;
            }
            whole = false;
            if let Some(n) = len(&part(&path)) {
                (got, any) = (got + n, true);
            }
        }
        match (whole, any) {
            (true, _) => Stage::Ready,
            (false, true) => Stage::Paused { got, total },
            (false, false) => Stage::Absent,
        }
    }

    fn drop_files(folder: &Path, files: &[ModelFile]) {
        for f in files {
            let path = folder.join(f.path);
            let _ = std::fs::remove_file(part(&path));
            let _ = std::fs::remove_file(path);
        }
    }

    fn len(path: &Path) -> Option<u64> {
        std::fs::metadata(path).ok().map(|m| m.len())
    }

    /// `<path>.part`: where a file grows until it checks out.
    fn part(path: &Path) -> PathBuf {
        let mut name = path.as_os_str().to_owned();
        name.push(".part");
        PathBuf::from(name)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::time::Duration;

    use sha2::{Digest, Sha256};

    use super::*;
    use crate::models::ModelVoice;

    fn leak(s: String) -> &'static str {
        Box::leak(s.into_boxed_str())
    }

    fn body(n: usize, seed: u8) -> Vec<u8> {
        (0..n)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    fn sha(b: &[u8]) -> &'static str {
        leak(
            Sha256::digest(b)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        )
    }

    /// Serves `files` by path, ranges honoured; else 404.
    fn serve(files: HashMap<String, Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split(' ').nth(1).unwrap_or("/").to_owned();
                let mut from = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap() <= 2 {
                        break;
                    }
                    if let Some(r) = h.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        from = r.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                let (status, bytes, extra) = match files.get(&path) {
                    Some(full) if from > 0 => {
                        let range = format!(
                            "Content-Range: bytes {from}-{}/{}\r\n",
                            full.len() - 1,
                            full.len()
                        );
                        ("206 Partial Content", full[from..].to_vec(), range)
                    }
                    Some(full) => ("200 OK", full.clone(), String::new()),
                    None => ("404 Not Found", b"no".to_vec(), String::new()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    bytes.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&bytes);
            }
        });
        format!("http://{addr}")
    }

    /// Models `ids`, each with a config, a weight file and two voices.
    fn catalogue(ids: &[&str], wrong: bool) -> (&'static [Model], HashMap<String, Vec<u8>>) {
        let mut files = HashMap::new();
        let mut models = Vec::new();
        let mut listed = |id: &str, path: &str, n: usize, seed: u8| {
            let b = body(n, seed);
            let file = ModelFile {
                path: leak(path.to_owned()),
                bytes: n as u64,
                sha256: if wrong { sha(b"other") } else { sha(&b) },
                hosts: None,
            };
            files.insert(format!("/{id}/{path}"), b);
            file
        };
        let mut specs = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            let seed = i as u8 * 7;
            let config = listed(id, "config.json", 40, seed);
            let weights = listed(id, "weights.bin", 300_000, seed + 1);
            let a = listed(id, "voices/a.pt", 2_000, seed + 2);
            let b = listed(id, "voices/b.pt", 3_000, seed + 3);
            specs.push((leak((*id).to_owned()), [config, weights], [a, b]));
        }
        let host = serve(files.clone());
        for (id, own, [a, b]) in specs {
            let voice = |vid: &'static str, file| ModelVoice {
                id: vid,
                name: vid,
                language: "en-US",
                feminine: true,
                file,
            };
            let hosts: &'static [&'static str] =
                Box::leak(vec![leak(format!("{host}/{id}/"))].into_boxed_slice());
            models.push(Model {
                id,
                name: id,
                about: "A test model",
                license: "MIT",
                hosts,
                files: Box::leak(own.to_vec().into_boxed_slice()),
                voices: Box::leak(vec![voice("a", a), voice("b", b)].into_boxed_slice()),
                first_voice: "a",
            });
        }
        (Box::leak(models.into_boxed_slice()), files)
    }

    fn shelf(dir: &std::path::Path, catalogue: &'static [Model]) -> Shelf {
        let mut shelf = Shelf::default();
        shelf
            .attach(dir.to_owned(), catalogue, Arc::new(|| {}))
            .unwrap();
        shelf
    }

    fn settle(shelf: &mut Shelf, until: impl Fn(&Shelf) -> bool) {
        for _ in 0..1000 {
            shelf.poll();
            if until(shelf) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("never settled: {:?}", shelf.stages);
    }

    #[test]
    fn a_model_comes_with_a_voice_takes_more_and_goes_whole() {
        let dir = tempfile::tempdir().unwrap();
        let (models, _) = catalogue(&["tiny"], false);
        let mut s = shelf(dir.path(), models);
        let model = &models[0];
        assert_eq!(*s.stage(Item::Voice("tiny", "b")), Stage::Absent);
        // A voice waits for its model.
        s.download(Item::Voice("tiny", "b"));
        assert_eq!(*s.stage(Item::Voice("tiny", "b")), Stage::Absent);
        s.download(Item::Model("tiny"));
        assert!(s.stage(Item::Model("tiny")).busy());
        settle(&mut s, |s| *s.stage(Item::Model("tiny")) == Stage::Ready);
        assert!(s.has(model, "a") && !s.has(model, "b"));
        assert_eq!(s.ready().len(), 1);
        let folder = dir.path().join("tiny");
        assert!(folder.join("weights.bin").exists() && folder.join("voices/a.pt").exists());
        s.download(Item::Voice("tiny", "b"));
        settle(&mut s, |s| s.has(model, "b"));
        assert_eq!(s.on_disk(model), 40 + 300_000 + 2_000 + 3_000);
        s.remove(Item::Voice("tiny", "b"));
        assert!(!s.has(model, "b") && !folder.join("voices/b.pt").exists());
        s.remove(Item::Model("tiny"));
        assert!(!folder.exists());
        assert!(s.ready().is_empty());
        // Found again on the next start, by what is on disk.
        s.download(Item::Model("tiny"));
        settle(&mut s, |s| *s.stage(Item::Model("tiny")) == Stage::Ready);
        let again = shelf(dir.path(), models);
        assert_eq!(*again.stage(Item::Model("tiny")), Stage::Ready);
    }

    #[test]
    fn downloads_take_turns_and_a_waiting_one_can_be_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (models, _) = catalogue(&["one", "two", "three"], false);
        let mut s = shelf(dir.path(), models);
        for id in ["one", "two", "three"] {
            s.download(Item::Model(id));
        }
        assert!(matches!(
            s.stage(Item::Model("one")),
            Stage::Fetching { .. }
        ));
        assert_eq!(*s.stage(Item::Model("two")), Stage::Waiting);
        s.cancel(Item::Model("three"));
        assert_eq!(*s.stage(Item::Model("three")), Stage::Absent);
        settle(&mut s, |s| *s.stage(Item::Model("two")) == Stage::Ready);
        assert_eq!(*s.stage(Item::Model("one")), Stage::Ready);
        assert_eq!(*s.stage(Item::Model("three")), Stage::Absent);
        assert!(!dir.path().join("three").exists());
    }

    #[test]
    fn a_file_with_the_wrong_checksum_is_never_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (models, _) = catalogue(&["bad"], true);
        let mut s = shelf(dir.path(), models);
        s.download(Item::Model("bad"));
        settle(&mut s, |s| {
            matches!(s.stage(Item::Model("bad")), Stage::Failed(_))
        });
        let why = Stage::Failed("the file isn't the one listed".into());
        assert_eq!(*s.stage(Item::Model("bad")), why);
        assert!(!dir.path().join("bad/config.json").exists());
    }

    #[test]
    fn a_stopped_download_is_found_paused_and_resumes() {
        let dir = tempfile::tempdir().unwrap();
        let (models, files) = catalogue(&["half"], false);
        let folder = dir.path().join("half");
        std::fs::create_dir_all(&folder).unwrap();
        let weights = &files["/half/weights.bin"];
        std::fs::write(folder.join("weights.bin.part"), &weights[..100_000]).unwrap();
        let mut s = shelf(dir.path(), models);
        let total = models[0].bytes();
        let paused = Stage::Paused {
            got: 100_000,
            total,
        };
        assert_eq!(*s.stage(Item::Model("half")), paused);
        s.download(Item::Model("half"));
        settle(&mut s, |s| *s.stage(Item::Model("half")) == Stage::Ready);
        assert_eq!(std::fs::read(folder.join("weights.bin")).unwrap(), *weights);
        assert!(!folder.join("weights.bin.part").exists());
    }
}
