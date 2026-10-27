use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};

use crate::error::CacheError;
use crate::key::validate_key;
use crate::store::CacheStore;

/// Distinguishes temp files written concurrently by threads of one process.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A directory of JSON files, one per entry.
///
/// Entries live at `<directory>/<first two key characters>/<key>.json`, so a
/// large cache does not put a hundred thousand files in one directory. Each
/// file holds `{"key": ..., "value": ...}`; the key is checked on read, so a
/// file copied to the wrong name is a miss rather than a wrong answer.
///
/// Entries are plain JSON rather than a serialized object graph, which
/// matters because a cache directory is a file a later process trusts.
///
/// # Failure behaviour
///
/// - **Writes are atomic.** A value is written to a temp file beside its
///   destination and renamed into place, so a reader sees the old entry or
///   the new one, never half of one, and a crash mid-write leaves at worst a
///   stray `.tmp` file that [`DiskStore::clear`] removes.
/// - **Unreadable entries are misses.** A missing, truncated, or otherwise
///   undecodable file — or one that cannot be read at all — makes
///   [`get`](CacheStore::get) return `Ok(None)`. The caller recomputes and
///   the next write replaces the damaged entry. This matches the Ruby gem.
/// - **Write failures are errors.** [`put`](CacheStore::put) reports them;
///   [`fetch`](crate::fetch) deliberately ignores them so a computed value is
///   never lost to a full disk.
///
/// # Examples
///
/// ```
/// use cosmos_llm_cache::{CacheStore, DiskStore};
/// use serde_json::json;
///
/// let dir = std::env::temp_dir().join(format!("cosmos-llm-cache-doc-{}", std::process::id()));
/// let store = DiskStore::new(&dir).unwrap();
/// store.put("ab12", &json!({ "completion": "4" })).unwrap();
///
/// assert!(dir.join("ab").join("ab12.json").exists());
/// assert_eq!(store.get("ab12").unwrap(), Some(json!({ "completion": "4" })));
/// # std::fs::remove_dir_all(&dir).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct DiskStore {
    directory: PathBuf,
}

/// What [`DiskStore::stats`] found on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiskStats {
    /// The number of entry files.
    pub entries: usize,
    /// Their combined size in bytes.
    pub bytes: u64,
}

impl DiskStore {
    /// Opens a store rooted at `directory`, creating it if needed.
    ///
    /// # Errors
    ///
    /// Returns [`CacheError::Io`] when the directory cannot be created.
    pub fn new(directory: impl Into<PathBuf>) -> Result<Self, CacheError> {
        let directory = directory.into();
        fs::create_dir_all(&directory).map_err(|e| CacheError::io(&directory, &e))?;
        Ok(Self { directory })
    }

    /// The directory the store lives in.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The file an entry is stored in.
    ///
    /// # Errors
    ///
    /// Returns [`CacheError::InvalidKey`] for a key that cannot name a file.
    pub fn path_for(&self, key: &str) -> Result<PathBuf, CacheError> {
        validate_key(key)?;
        let shard = &key[..key.len().min(2)];
        Ok(self.directory.join(shard).join(format!("{key}.json")))
    }

    /// Counts the entries and their size by walking the directory.
    ///
    /// This is a full scan, which is why [`CacheStore::len`] does not call it.
    ///
    /// # Errors
    ///
    /// Returns [`CacheError::Io`] when the directory cannot be listed.
    pub fn stats(&self) -> Result<DiskStats, CacheError> {
        let mut stats = DiskStats::default();
        for file in self.files(|name| name.ends_with(".json"))? {
            stats.entries += 1;
            stats.bytes += fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        }
        Ok(stats)
    }

    /// Files in shard directories whose names satisfy `wanted`.
    ///
    /// Only the layout this store writes is visited: directories of one or
    /// two key characters directly under the root. Anything else in the
    /// directory is left alone, which is what makes [`CacheStore::clear`]
    /// safe to point at a directory that also holds other files.
    fn files(&self, wanted: impl Fn(&str) -> bool) -> Result<Vec<PathBuf>, CacheError> {
        let root = match fs::read_dir(&self.directory) {
            Ok(root) => root,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(CacheError::io(&self.directory, &e)),
        };
        let mut found = Vec::new();
        for shard in root.flatten() {
            let shard_path = shard.path();
            let is_shard = shard_path.is_dir()
                && shard.file_name().to_str().is_some_and(|name| {
                    (1..=2).contains(&name.len()) && validate_key(name).is_ok()
                });
            if !is_shard {
                continue;
            }
            let entries = fs::read_dir(&shard_path).map_err(|e| CacheError::io(&shard_path, &e))?;
            for entry in entries.flatten() {
                if entry.file_name().to_str().is_some_and(&wanted) && entry.path().is_file() {
                    found.push(entry.path());
                }
            }
        }
        Ok(found)
    }
}

impl CacheStore for DiskStore {
    fn get(&self, key: &str) -> Result<Option<Value>, CacheError> {
        let path = self.path_for(key)?;
        let Ok(text) = fs::read_to_string(&path) else {
            return Ok(None);
        };
        let Ok(Value::Object(mut entry)) = serde_json::from_str::<Value>(&text) else {
            return Ok(None);
        };
        if entry.get("key").and_then(Value::as_str) != Some(key) {
            return Ok(None);
        }
        Ok(entry.remove("value"))
    }

    fn put(&self, key: &str, value: &Value) -> Result<(), CacheError> {
        let path = self.path_for(key)?;
        let shard = path.parent().unwrap_or(&self.directory);
        fs::create_dir_all(shard).map_err(|e| CacheError::io(shard, &e))?;

        let payload = json!({ "key": key, "value": value }).to_string();
        let temp = path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        if let Err(e) = fs::write(&temp, payload) {
            let _ = fs::remove_file(&temp);
            return Err(CacheError::io(&temp, &e));
        }
        fs::rename(&temp, &path).map_err(|e| {
            let _ = fs::remove_file(&temp);
            CacheError::io(&path, &e)
        })
    }

    fn remove(&self, key: &str) -> Result<bool, CacheError> {
        let path = self.path_for(key)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
            Err(e) => Err(CacheError::io(&path, &e)),
        }
    }

    /// Deletes every entry file and stray temp file, then any shard
    /// directory left empty. Files the store did not write are kept.
    fn clear(&self) -> Result<(), CacheError> {
        for file in self.files(|name| name.ends_with(".json") || name.ends_with(".tmp"))? {
            match fs::remove_file(&file) {
                Ok(()) => {}
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(CacheError::io(&file, &e)),
            }
            if let Some(shard) = file.parent() {
                // Fails harmlessly while the shard still holds other files.
                let _ = fs::remove_dir(shard);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "cosmos-llm-cache-{name}-{}-{}",
                std::process::id(),
                TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn entries_survive_reopening_the_store() {
        let dir = Scratch::new("reopen");
        DiskStore::new(&dir.0)
            .unwrap()
            .put("k1", &json!([1, 2]))
            .unwrap();
        assert_eq!(
            DiskStore::new(&dir.0).unwrap().get("k1").unwrap(),
            Some(json!([1, 2]))
        );
    }

    #[test]
    fn single_character_keys_shard_by_themselves() {
        let dir = Scratch::new("short");
        let store = DiskStore::new(&dir.0).unwrap();
        store.put("z", &json!(true)).unwrap();
        assert!(dir.0.join("z").join("z.json").is_file());
        assert_eq!(store.get("z").unwrap(), Some(json!(true)));
    }

    #[test]
    fn corrupt_and_mismatched_entries_are_misses() {
        let dir = Scratch::new("corrupt");
        let store = DiskStore::new(&dir.0).unwrap();
        store.put("abc", &json!(1)).unwrap();
        fs::write(store.path_for("abc").unwrap(), "{\"key\": \"abc\", \"val").unwrap();
        assert_eq!(store.get("abc").unwrap(), None);

        // An entry renamed to another key's file does not answer for it.
        store.put("abd", &json!(2)).unwrap();
        fs::copy(
            store.path_for("abd").unwrap(),
            store.path_for("abe").unwrap(),
        )
        .unwrap();
        assert_eq!(store.get("abe").unwrap(), None);

        // A damaged entry is replaced by the next write.
        store.put("abc", &json!(3)).unwrap();
        assert_eq!(store.get("abc").unwrap(), Some(json!(3)));
    }

    #[test]
    fn writes_leave_no_temp_files_behind() {
        let dir = Scratch::new("atomic");
        let store = DiskStore::new(&dir.0).unwrap();
        for n in 0..5 {
            store.put("same", &json!(n)).unwrap();
        }
        let names: Vec<String> = fs::read_dir(dir.0.join("sa"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, ["same.json"]);
    }

    #[test]
    fn concurrent_writers_to_one_key_never_tear_an_entry() {
        let dir = Scratch::new("threads");
        let store = std::sync::Arc::new(DiskStore::new(&dir.0).unwrap());
        let big = "x".repeat(64 * 1024);
        let handles: Vec<_> = (0..8)
            .map(|n| {
                let store = store.clone();
                let value = json!({ "n": n, "pad": big });
                std::thread::spawn(move || {
                    for _ in 0..10 {
                        store.put("hot", &value).unwrap();
                        let read = store
                            .get("hot")
                            .unwrap()
                            .expect("never a miss after a write");
                        assert_eq!(read["pad"].as_str().unwrap().len(), 64 * 1024);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn clear_removes_only_what_the_store_wrote() {
        let dir = Scratch::new("clear");
        let store = DiskStore::new(&dir.0).unwrap();
        store.put("aa1", &json!(1)).unwrap();
        store.put("bb1", &json!(2)).unwrap();
        fs::write(
            store
                .path_for("aa1")
                .unwrap()
                .with_extension("json.9.9.tmp"),
            "",
        )
        .unwrap();
        fs::write(dir.0.join("README.txt"), "keep me").unwrap();
        fs::create_dir(dir.0.join("notes")).unwrap();
        fs::write(dir.0.join("notes").join("x.json"), "{}").unwrap();

        assert_eq!(store.stats().unwrap().entries, 2);
        store.clear().unwrap();

        assert_eq!(store.stats().unwrap(), DiskStats::default());
        assert!(!dir.0.join("aa").exists());
        assert!(dir.0.join("README.txt").exists());
        assert!(dir.0.join("notes").join("x.json").exists());
    }

    #[test]
    fn remove_reports_whether_an_entry_existed() {
        let dir = Scratch::new("remove");
        let store = DiskStore::new(&dir.0).unwrap();
        store.put("k1", &json!(1)).unwrap();
        assert!(store.remove("k1").unwrap());
        assert!(!store.remove("k1").unwrap());
    }

    #[test]
    fn path_traversal_keys_are_rejected() {
        let dir = Scratch::new("traverse");
        let store = DiskStore::new(&dir.0).unwrap();
        assert!(matches!(
            store.put("../x", &json!(1)),
            Err(CacheError::InvalidKey(_))
        ));
        assert!(matches!(
            store.get("a/../../b"),
            Err(CacheError::InvalidKey(_))
        ));
    }
}
