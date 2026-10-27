use serde_json::Value;

use crate::error::CacheError;
use crate::key::key_for;

/// A key-value store for JSON values.
///
/// This is the seam for swapping cache backends. The built-ins are
/// [`MemoryStore`](crate::MemoryStore), [`DiskStore`](crate::DiskStore), and
/// [`TieredStore`](crate::TieredStore); a Redis or SQLite store implements
/// the same five methods and registers itself with
/// [`CachePlugin`](crate::CachePlugin).
///
/// Implementations are shared across threads, so they synchronize
/// internally.
pub trait CacheStore: Send + Sync {
    /// Reads the value stored under `key`, or `None` on a miss.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend cannot be consulted at all. An
    /// entry that exists but cannot be decoded should be reported as a miss,
    /// not an error: a damaged cache entry is a reason to recompute, never a
    /// reason to fail the call it was meant to speed up.
    fn get(&self, key: &str) -> Result<Option<Value>, CacheError>;

    /// Stores `value` under `key`, replacing any previous value.
    ///
    /// # Errors
    ///
    /// Returns an error when the value could not be stored.
    fn put(&self, key: &str, value: &Value) -> Result<(), CacheError>;

    /// Removes the entry under `key`, returning whether one existed.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend could not remove it.
    fn remove(&self, key: &str) -> Result<bool, CacheError>;

    /// Removes every entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend could not be cleared.
    fn clear(&self) -> Result<(), CacheError>;

    /// The number of entries, when that is cheap to know.
    ///
    /// `None` means unknown or expensive to compute, not empty.
    fn len(&self) -> Option<usize> {
        None
    }

    /// Whether the store is known to be empty.
    ///
    /// `false` when [`CacheStore::len`] is `None`.
    fn is_empty(&self) -> bool {
        self.len() == Some(0)
    }
}

/// Returns the cached value for `request`, computing and storing it on a miss.
///
/// The key is [`key_for`]`(request)`. The second element of the result is
/// `true` on a hit.
///
/// A failed *write* does not fail the call: the value was already computed —
/// for a completion, already paid for — and throwing it away because the disk
/// is full would be the worst possible outcome. Call [`CacheStore::get`] and
/// [`CacheStore::put`] directly to observe write failures. A failed *read*
/// is returned as an error, since it means the store is unusable.
///
/// # Errors
///
/// Returns the read error converted into `E`, or whatever `compute` returns.
///
/// # Examples
///
/// ```
/// use cosmos_llm_cache::{fetch, CacheError, MemoryStore};
/// use serde_json::json;
///
/// let store = MemoryStore::new(100);
/// let request = json!({ "model": "m", "prompt": "2+2" });
///
/// let (value, hit) = fetch(&store, &request, || Ok::<_, CacheError>(json!("4"))).unwrap();
/// assert_eq!((value, hit), (json!("4"), false));
///
/// let (value, hit) = fetch(&store, &request, || -> Result<_, CacheError> {
///     unreachable!("a hit does not recompute")
/// })
/// .unwrap();
/// assert_eq!((value, hit), (json!("4"), true));
/// ```
pub fn fetch<E, F>(store: &dyn CacheStore, request: &Value, compute: F) -> Result<(Value, bool), E>
where
    E: From<CacheError>,
    F: FnOnce() -> Result<Value, E>,
{
    let key = key_for(request);
    if let Some(hit) = store.get(&key)? {
        return Ok((hit, true));
    }
    let value = compute()?;
    let _ = store.put(&key, &value);
    Ok((value, false))
}
