//! A map that marks every key changed through it; saves write only those.

use std::collections::BTreeSet;
use std::collections::btree_map::{self, BTreeMap, Entry};
use std::ops::{Deref, RangeBounds};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug)]
pub(crate) struct Tracked<K, V> {
    map: BTreeMap<K, V>,
    changed: BTreeSet<K>,
    /// New whenever a value may have changed, so caches can key on it.
    stamp: u64,
}

/// Unique across maps, so one loaded again never repeats a stamp.
fn next_stamp() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl<K: Ord + Copy, V> Default for Tracked<K, V> {
    fn default() -> Self {
        Self::new(BTreeMap::new())
    }
}

impl<K: Ord + Copy, V> Tracked<K, V> {
    /// Values as they are on disk: nothing changed yet.
    pub fn new(map: BTreeMap<K, V>) -> Self {
        Self {
            map,
            changed: BTreeSet::new(),
            stamp: next_stamp(),
        }
    }

    /// Changes whenever a value may have, even unsaved ones.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.stamp = next_stamp();
        let value = self.map.get_mut(key)?;
        self.changed.insert(*key);
        Some(value)
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.stamp = next_stamp();
        self.changed.insert(key);
        self.map.insert(key, value)
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.stamp = next_stamp();
        let old = self.map.remove(key)?;
        self.changed.insert(*key);
        Some(old)
    }

    pub fn entry(&mut self, key: K) -> Entry<'_, K, V> {
        self.stamp = next_stamp();
        self.changed.insert(key);
        self.map.entry(key)
    }

    /// Each value `pick` keeps, to change; only those are marked.
    pub fn values_mut_where(&mut self, pick: impl Fn(&V) -> bool) -> impl Iterator<Item = &mut V> {
        self.stamp = next_stamp();
        let picked = self.map.iter().filter(|(_, v)| pick(v)).map(|(k, _)| *k);
        let picked: BTreeSet<K> = picked.collect();
        self.changed.extend(picked.iter().copied());
        let map = self.map.iter_mut();
        map.filter(move |(k, _)| picked.contains(k)).map(|(_, v)| v)
    }

    /// The values with keys in `range`, to change; all are marked.
    pub fn range_mut<R: RangeBounds<K> + Clone>(
        &mut self,
        range: R,
    ) -> btree_map::RangeMut<'_, K, V> {
        self.stamp = next_stamp();
        let keys = self.map.range(range.clone()).map(|(k, _)| *k);
        self.changed.extend(keys);
        self.map.range_mut(range)
    }

    /// For what is never saved, such as whether a file is there.
    pub fn quiet_get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.stamp = next_stamp();
        self.map.get_mut(key)
    }

    /// As [`Self::quiet_get_mut`], for every value.
    pub fn quiet_values_mut(&mut self) -> btree_map::ValuesMut<'_, K, V> {
        self.stamp = next_stamp();
        self.map.values_mut()
    }

    /// The keys changed since the last call, gone ones included.
    pub fn take_changed(&mut self) -> BTreeSet<K> {
        std::mem::take(&mut self.changed)
    }

    pub fn changed(&self) -> &BTreeSet<K> {
        &self.changed
    }

    pub fn has_changes(&self) -> bool {
        !self.changed.is_empty()
    }

    /// Marks everything, so the next save writes it all.
    pub fn mark_all(&mut self) {
        self.changed.extend(self.map.keys().copied());
    }
}

impl<K, V> Deref for Tracked<K, V> {
    type Target = BTreeMap<K, V>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl<K: Ord + Copy, V> FromIterator<(K, V)> for Tracked<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self::new(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_change_moves_the_stamp_and_reads_do_not() {
        let mut map: Tracked<u8, u8> = (0..5).map(|k| (k, k)).collect();
        let first = map.stamp();
        let _ = map.get(&1);
        assert_eq!(map.stamp(), first);
        map.insert(9, 9);
        let inserted = map.stamp();
        assert_ne!(inserted, first);
        map.quiet_get_mut(&2);
        assert_ne!(map.stamp(), inserted);
        let other: Tracked<u8, u8> = Tracked::default();
        assert_ne!(other.stamp(), map.stamp(), "a new map never repeats one");
    }

    #[test]
    fn only_what_was_changed_is_marked() {
        let mut map: Tracked<u8, u8> = (0..5).map(|k| (k, k)).collect();
        assert!(!map.has_changes());
        let _ = map.get(&1);
        if let Some(v) = map.get_mut(&2) {
            *v = 20;
        }
        map.values_mut_where(|v| *v == 4).for_each(|v| *v = 40);
        map.remove(&0);
        map.remove(&9);
        if let Some(v) = map.quiet_get_mut(&3) {
            *v = 30;
        }
        assert_eq!(map.take_changed(), BTreeSet::from([0, 2, 4]));
        assert!(!map.has_changes());
        assert_eq!(map.get(&3), Some(&30));
    }
}
