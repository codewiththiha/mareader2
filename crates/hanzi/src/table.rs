//! Words in byte order, for lookups and prefix checks without a trie.

/// Each word once, with a value; a repeated word keeps its last value.
pub struct Table<V> {
    /// Every word, end to end.
    text: String,
    /// Where each word sits in `text`, in byte order.
    spans: Vec<(u32, u32)>,
    values: Vec<V>,
}

impl<V: Copy> Table<V> {
    /// None past 4 GiB of words.
    pub fn new<K: AsRef<str>>(mut entries: Vec<(K, V)>) -> Option<Self> {
        // A stable sort, so the last of equal words comes last.
        entries.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
        let mut table = Self {
            text: String::new(),
            spans: Vec::with_capacity(entries.len()),
            values: Vec::with_capacity(entries.len()),
        };
        for (key, value) in entries {
            let key = key.as_ref();
            if !table.spans.is_empty() && table.key(table.spans.len() - 1) == key {
                if let Some(last) = table.values.last_mut() {
                    *last = value;
                }
                continue;
            }
            let start = u32::try_from(table.text.len()).ok()?;
            table.text.push_str(key);
            let end = u32::try_from(table.text.len()).ok()?;
            table.spans.push((start, end));
            table.values.push(value);
        }
        table.text.shrink_to_fit();
        table.spans.shrink_to_fit();
        table.values.shrink_to_fit();
        Some(table)
    }

    fn key(&self, at: usize) -> &str {
        self.spans
            .get(at)
            .and_then(|&(start, end)| self.text.get(start as usize..end as usize))
            .unwrap_or("")
    }

    /// The first place a word not before `key` sits.
    fn lower(&self, key: &str) -> usize {
        let (mut low, mut high) = (0, self.spans.len());
        while low < high {
            let mid = low + (high - low) / 2;
            if self.key(mid) < key {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        low
    }

    pub fn get(&self, key: &str) -> Option<V> {
        let at = self.lower(key);
        match at < self.spans.len() && self.key(at) == key {
            true => self.values.get(at).copied(),
            false => None,
        }
    }

    /// Whether some word starts with `prefix`, the word itself included.
    pub fn has_prefix(&self, prefix: &str) -> bool {
        let at = self.lower(prefix);
        at < self.spans.len() && self.key(at).starts_with(prefix)
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }
}
