//! Loaded voice models and the speech they made, shared by every voicer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use mareader_g2p::{Dialect, G2p};
use mareader_kokoro::Kokoro;

use crate::Usage;

/// Speech kept by default, in bytes.
const MEMORY: usize = 64 << 20;
/// How long an unused model stays loaded by default.
const IDLE: Duration = Duration::from_secs(120);

/// One loaded thing, kept only while someone holds it.
struct Slot<T>(Mutex<Weak<T>>);

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self(Mutex::new(Weak::new()))
    }
}

impl<T> Slot<T> {
    /// The loaded one, or `load`'s; one load at a time.
    fn get(&self, load: impl FnOnce() -> Result<T, String>) -> Result<Arc<T>, String> {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(it) = held.upgrade() {
            return Ok(it);
        }
        let it = Arc::new(load()?);
        *held = Arc::downgrade(&it);
        Ok(it)
    }
}

/// What a sentence was said with, and the sentence.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    /// The model and build that said it, as `Kokoro (Small)`.
    pub model: Arc<str>,
    pub voice: String,
    /// The speed's bits, so equal speeds hash equal.
    pub speed: u32,
    pub text: String,
}

/// Mono samples at the model's rate.
pub(crate) type Pcm = Arc<[i16]>;

/// Speech already made, the least recently heard let go past the cap.
#[derive(Default)]
pub(crate) struct Takes {
    cap: usize,
    used: usize,
    clock: u64,
    kept: HashMap<Key, (Pcm, u64)>,
}

impl Takes {
    pub fn get(&mut self, key: &Key) -> Option<Pcm> {
        self.clock += 1;
        let (pcm, used) = self.kept.get_mut(key)?;
        *used = self.clock;
        Some(Arc::clone(pcm))
    }

    pub fn has(&self, key: &Key) -> bool {
        self.kept.contains_key(key)
    }

    pub fn put(&mut self, key: Key, pcm: Pcm) {
        self.clock += 1;
        self.used += bytes(&pcm);
        if let Some((old, _)) = self.kept.insert(key, (pcm, self.clock)) {
            self.used -= bytes(&old);
        }
        self.trim();
    }

    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap;
        self.trim();
    }

    fn trim(&mut self) {
        while self.used > self.cap {
            let oldest = self.kept.iter().min_by_key(|(_, (_, used))| *used);
            let Some(key) = oldest.map(|(k, _)| k.clone()) else {
                return;
            };
            if let Some((pcm, _)) = self.kept.remove(&key) {
                self.used -= bytes(&pcm);
            }
        }
    }
}

fn bytes(pcm: &Pcm) -> usize {
    std::mem::size_of_val(&**pcm)
}

#[derive(Default)]
struct Inner {
    /// The loaded build's name, and the model.
    kokoro: Mutex<(Arc<str>, Weak<Kokoro>)>,
    american: Slot<G2p>,
    british: Slot<G2p>,
    takes: Mutex<Takes>,
    idle: Mutex<Option<Duration>>,
}

/// Models load once however many voicers use them, and go with the last.
#[derive(Clone)]
pub struct Studio(Arc<Inner>);

impl Default for Studio {
    fn default() -> Self {
        let studio = Self(Arc::default());
        studio.set_memory(MEMORY);
        studio.set_idle(Some(IDLE));
        studio
    }
}

impl Studio {
    /// Keeps at most `bytes` of speech made.
    pub fn set_memory(&self, bytes: usize) {
        self.takes().set_cap(bytes);
    }

    /// Lets an unused model go after `idle`; `None` keeps it while open.
    pub fn set_idle(&self, idle: Option<Duration>) {
        *self.0.idle.lock().unwrap_or_else(PoisonError::into_inner) = idle;
    }

    pub(crate) fn idle(&self) -> Option<Duration> {
        *self.0.idle.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn usage(&self) -> Usage {
        let kokoro = self.kokoro_slot();
        let held = (kokoro.1.strong_count() > 0).then(|| kokoro.0.to_string());
        drop(kokoro);
        Usage {
            models: held.into_iter().collect(),
            speech: self.takes().used,
        }
    }

    /// Whether build `name` of Kokoro is loaded.
    pub(crate) fn has_kokoro(&self, name: &str) -> bool {
        let kokoro = self.kokoro_slot();
        *kokoro.0 == *name && kokoro.1.strong_count() > 0
    }

    fn kokoro_slot(&self) -> MutexGuard<'_, (Arc<str>, Weak<Kokoro>)> {
        self.0.kokoro.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn takes(&self) -> MutexGuard<'_, Takes> {
        self.0.takes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Build `name` of Kokoro, loaded once; another build is loaded anew.
    pub(crate) fn kokoro(
        &self,
        name: &Arc<str>,
        load: impl FnOnce() -> Result<Kokoro, String>,
    ) -> Result<Arc<Kokoro>, String> {
        let mut held = self.kokoro_slot();
        if held.0 == *name
            && let Some(it) = held.1.upgrade()
        {
            return Ok(it);
        }
        let it = Arc::new(load()?);
        *held = (Arc::clone(name), Arc::downgrade(&it));
        Ok(it)
    }

    pub(crate) fn g2p(
        &self,
        dialect: Dialect,
        load: impl FnOnce() -> Result<G2p, String>,
    ) -> Result<Arc<G2p>, String> {
        match dialect {
            Dialect::American => self.0.american.get(load),
            Dialect::British => self.0.british.get(load),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Key {
        Key {
            model: "Kokoro (Full)".into(),
            voice: "af_heart".into(),
            speed: 1f32.to_bits(),
            text: text.into(),
        }
    }

    fn pcm(samples: usize) -> Pcm {
        Arc::from(vec![0i16; samples])
    }

    #[test]
    fn the_least_recently_heard_goes_first() {
        let mut takes = Takes::default();
        takes.set_cap(250);
        takes.put(key("a"), pcm(50));
        takes.put(key("b"), pcm(50));
        assert!(takes.get(&key("a")).is_some());
        takes.put(key("c"), pcm(50));
        assert!(takes.has(&key("a")) && takes.has(&key("c")));
        assert!(!takes.has(&key("b")), "b was heard least recently");
    }

    #[test]
    fn a_smaller_cap_lets_go_at_once_and_none_keeps_nothing() {
        let mut takes = Takes::default();
        takes.set_cap(1000);
        takes.put(key("a"), pcm(100));
        takes.put(key("a"), pcm(100));
        takes.put(key("b"), pcm(100));
        takes.set_cap(200);
        assert!(!takes.has(&key("a")) && takes.has(&key("b")));
        takes.set_cap(0);
        assert!(!takes.has(&key("b")));
        takes.put(key("c"), pcm(1));
        assert!(!takes.has(&key("c")));
    }

    #[test]
    fn usage_counts_speech_kept_and_no_model_until_one_loads() {
        let studio = Studio::default();
        assert_eq!(studio.usage(), Usage::default());
        studio.takes().put(key("a"), pcm(100));
        assert_eq!(studio.usage().speech, 200);
        assert_eq!(studio.idle(), Some(IDLE));
        studio.set_idle(None);
        assert_eq!(studio.idle(), None);
    }

    #[test]
    fn a_loaded_thing_is_shared_then_let_go() {
        let slot: Slot<u32> = Slot::default();
        let one = slot.get(|| Ok(1));
        let two = slot.get(|| Ok(2));
        assert_eq!(one.as_deref(), Ok(&1));
        assert_eq!(two.as_deref(), Ok(&1), "loaded once while held");
        let held = |s: &Slot<u32>| s.0.lock().unwrap().strong_count() > 0;
        assert!(held(&slot));
        drop((one, two));
        assert!(!held(&slot));
        assert_eq!(slot.get(|| Ok(3)).as_deref(), Ok(&3));
        assert!(slot.get(|| Err::<u32, _>("gone".into())).is_err());
    }
}
