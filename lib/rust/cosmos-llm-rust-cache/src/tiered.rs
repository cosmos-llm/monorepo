use serde_json::Value;

use crate::error::CacheError;
use crate::store::CacheStore;

/// A fast store in front of a durable one.
///
/// Reads check the front store first, then the back; a back-store hit is
/// copied into the front so the next read is fast. Writes go to both. This
/// is the Ruby gem's two-level cache: in-process LRU, then disk.
///
/// Either side can be any [`CacheStore`], so a memory store in front of a
/// shared Redis store works the same way.
///
/// # Examples
///
/// ```
/// use cosmos_llm_cache::{CacheStore, DiskStore, MemoryStore, TieredStore};
/// use serde_json::json;
///
/// let dir = std::env::temp_dir().join(format!("cosmos-llm-cache-tier-doc-{}", std::process::id()));
/// let store = TieredStore::new(MemoryStore::new(100), DiskStore::new(&dir).unwrap());
/// store.put("k1", &json!("v")).unwrap();
///
/// // A new process starts with an empty front store but finds the entry on disk.
/// let later = TieredStore::new(MemoryStore::new(100), DiskStore::new(&dir).unwrap());
/// assert_eq!(later.get("k1").unwrap(), Some(json!("v")));
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
pub struct TieredStore {
    front: Box<dyn CacheStore>,
    back: Box<dyn CacheStore>,
}

impl TieredStore {
    /// Puts `front` in front of `back`.
    pub fn new(front: impl CacheStore + 'static, back: impl CacheStore + 'static) -> Self {
        Self::from_boxed(Box::new(front), Box::new(back))
    }

    /// As [`TieredStore::new`], for stores already boxed, such as ones built
    /// from the registry.
    pub fn from_boxed(front: Box<dyn CacheStore>, back: Box<dyn CacheStore>) -> Self {
        Self { front, back }
    }

    /// The fast store.
    pub fn front(&self) -> &dyn CacheStore {
        self.front.as_ref()
    }

    /// The durable store.
    pub fn back(&self) -> &dyn CacheStore {
        self.back.as_ref()
    }
}

impl CacheStore for TieredStore {
    fn get(&self, key: &str) -> Result<Option<Value>, CacheError> {
        if let Some(hit) = self.front.get(key)? {
            return Ok(Some(hit));
        }
        let Some(hit) = self.back.get(key)? else {
            return Ok(None);
        };
        // Promotion is an optimization; failing it must not fail the read.
        let _ = self.front.put(key, &hit);
        Ok(Some(hit))
    }

    /// Writes the back store first, so a value is only ever in the fast store
    /// if it also reached the durable one.
    fn put(&self, key: &str, value: &Value) -> Result<(), CacheError> {
        self.back.put(key, value)?;
        self.front.put(key, value)
    }

    fn remove(&self, key: &str) -> Result<bool, CacheError> {
        let front = self.front.remove(key)?;
        let back = self.back.remove(key)?;
        Ok(front || back)
    }

    fn clear(&self) -> Result<(), CacheError> {
        self.front.clear()?;
        self.back.clear()
    }

    /// The back store's count, since it holds every entry.
    fn len(&self) -> Option<usize> {
        self.back.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStore;
    use serde_json::json;

    #[test]
    fn a_back_hit_is_promoted_to_the_front() {
        let back = MemoryStore::new(10);
        back.put("k", &json!(1)).unwrap();
        let store = TieredStore::new(MemoryStore::new(10), back);
        assert_eq!(store.front().get("k").unwrap(), None);
        assert_eq!(store.get("k").unwrap(), Some(json!(1)));
        assert_eq!(store.front().get("k").unwrap(), Some(json!(1)));
    }

    #[test]
    fn writes_and_removals_reach_both_levels() {
        let store = TieredStore::new(MemoryStore::new(10), MemoryStore::new(10));
        store.put("k", &json!(1)).unwrap();
        assert_eq!(store.back().get("k").unwrap(), Some(json!(1)));
        assert!(store.remove("k").unwrap());
        assert_eq!(store.front().get("k").unwrap(), None);
        assert_eq!(store.back().get("k").unwrap(), None);
        assert_eq!(store.len(), Some(0));
    }
}
