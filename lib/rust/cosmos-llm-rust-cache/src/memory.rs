use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, MutexGuard};

use serde_json::Value;

use crate::error::CacheError;
use crate::key::validate_key;
use crate::store::CacheStore;

/// The default cap on in-memory entries.
pub const DEFAULT_MAX_MEMORY_ENTRIES: usize = 5_000;

/// An in-process least-recently-used cache.
///
/// Reads and writes both count as use. When an insert takes the store past
/// its cap, the least recently used entries are evicted. Everything vanishes
/// with the process; put a [`DiskStore`](crate::DiskStore) behind it with
/// [`TieredStore`](crate::TieredStore) to keep entries across runs.
///
/// # Examples
///
/// ```
/// use cosmos_llm_cache::{CacheStore, MemoryStore};
/// use serde_json::json;
///
/// let store = MemoryStore::new(2);
/// store.put("a", &json!(1)).unwrap();
/// store.put("b", &json!(2)).unwrap();
/// store.get("a").unwrap();          // "a" is now the most recent
/// store.put("c", &json!(3)).unwrap(); // evicts "b"
///
/// assert_eq!(store.get("b").unwrap(), None);
/// assert_eq!(store.get("a").unwrap(), Some(json!(1)));
/// ```
#[derive(Debug)]
pub struct MemoryStore {
    max_entries: usize,
    state: Mutex<Lru>,
}

#[derive(Debug, Default)]
struct Lru {
    /// Value and last-use tick, by key.
    entries: HashMap<String, (Value, u64)>,
    /// Key by last-use tick; the first entry is the least recently used.
    order: BTreeMap<u64, String>,
    tick: u64,
}

impl Lru {
    fn touch(&mut self, key: &str) -> Option<Value> {
        self.tick += 1;
        let tick = self.tick;
        let (value, used) = self.entries.get_mut(key)?;
        self.order.remove(used);
        *used = tick;
        self.order.insert(tick, key.to_owned());
        Some(value.clone())
    }

    fn insert(&mut self, key: &str, value: Value, max_entries: usize) {
        self.tick += 1;
        if let Some((_, used)) = self.entries.insert(key.to_owned(), (value, self.tick)) {
            self.order.remove(&used);
        }
        self.order.insert(self.tick, key.to_owned());
        while self.entries.len() > max_entries {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }

    fn remove(&mut self, key: &str) -> bool {
        match self.entries.remove(key) {
            Some((_, used)) => {
                self.order.remove(&used);
                true
            }
            None => false,
        }
    }
}

impl MemoryStore {
    /// Builds a store holding at most `max_entries` entries.
    ///
    /// A cap of zero is raised to one: a cache that can hold nothing is a
    /// configuration mistake, not a useful setting.
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            state: Mutex::new(Lru::default()),
        }
    }

    /// The cap on entries.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    fn state(&self) -> MutexGuard<'_, Lru> {
        // A panic while holding the lock cannot leave the maps inconsistent
        // in a way that matters for a cache, so a poisoned lock is reused.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_MEMORY_ENTRIES)
    }
}

impl CacheStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<Value>, CacheError> {
        validate_key(key)?;
        Ok(self.state().touch(key))
    }

    fn put(&self, key: &str, value: &Value) -> Result<(), CacheError> {
        validate_key(key)?;
        self.state().insert(key, value.clone(), self.max_entries);
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<bool, CacheError> {
        validate_key(key)?;
        Ok(self.state().remove(key))
    }

    fn clear(&self) -> Result<(), CacheError> {
        let mut state = self.state();
        state.entries.clear();
        state.order.clear();
        Ok(())
    }

    fn len(&self) -> Option<usize> {
        Some(self.state().entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overwriting_a_key_does_not_grow_the_store() {
        let store = MemoryStore::new(2);
        store.put("a", &json!(1)).unwrap();
        store.put("a", &json!(2)).unwrap();
        store.put("b", &json!(3)).unwrap();
        assert_eq!(store.len(), Some(2));
        assert_eq!(store.get("a").unwrap(), Some(json!(2)));
    }

    #[test]
    fn eviction_follows_use_not_insertion() {
        let store = MemoryStore::new(3);
        for key in ["a", "b", "c"] {
            store.put(key, &json!(key)).unwrap();
        }
        store.get("a").unwrap();
        store.put("d", &json!("d")).unwrap();
        store.put("e", &json!("e")).unwrap();
        let present: Vec<&str> = ["a", "b", "c", "d", "e"]
            .into_iter()
            .filter(|key| store.get(key).unwrap().is_some())
            .collect();
        assert_eq!(present, ["a", "d", "e"]);
    }

    #[test]
    fn remove_and_clear() {
        let store = MemoryStore::default();
        store.put("a", &json!(1)).unwrap();
        assert!(store.remove("a").unwrap());
        assert!(!store.remove("a").unwrap());
        store.put("b", &json!(1)).unwrap();
        store.clear().unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn a_zero_cap_holds_one_entry() {
        let store = MemoryStore::new(0);
        store.put("a", &json!(1)).unwrap();
        assert_eq!(store.get("a").unwrap(), Some(json!(1)));
    }

    #[test]
    fn invalid_keys_are_rejected_like_on_disk() {
        assert!(MemoryStore::default().put("a/b", &json!(1)).is_err());
    }
}
