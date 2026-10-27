use std::path::PathBuf;

use cosmos_llm_registry::{meta, Meta, Plugin};

use crate::disk::DiskStore;
use crate::error::CacheError;
use crate::memory::{MemoryStore, DEFAULT_MAX_MEMORY_ENTRIES};
use crate::store::CacheStore;
use crate::tiered::TieredStore;

/// Settings a registered store is built from.
///
/// Deliberately small: a backend that needs more — a Redis URL, a table
/// name — reads it from its own environment variables or config, since
/// adding a field here for every backend would make every caller carry
/// options it does not use.
#[derive(Debug, Clone, Default)]
pub struct CacheOptions {
    /// Where a durable store keeps its entries.
    pub directory: Option<PathBuf>,
    /// The cap on in-memory entries; defaults to
    /// [`DEFAULT_MAX_MEMORY_ENTRIES`].
    pub max_memory_entries: Option<usize>,
}

impl CacheOptions {
    /// Options with a directory set.
    pub fn in_directory(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: Some(directory.into()),
            ..Self::default()
        }
    }

    /// The memory cap to use.
    pub fn memory_cap(&self) -> usize {
        self.max_memory_entries
            .unwrap_or(DEFAULT_MAX_MEMORY_ENTRIES)
    }

    /// The directory, or a configuration error naming the store that needs it.
    ///
    /// # Errors
    ///
    /// Returns [`CacheError::Config`] when no directory is set.
    pub fn require_directory(&self, store: &str) -> Result<&PathBuf, CacheError> {
        self.directory
            .as_ref()
            .ok_or_else(|| CacheError::Config(format!("the '{store}' store needs a directory")))
    }
}

/// A cache backend registration.
///
/// ```
/// use cosmos_llm_cache::{store, CacheError, CacheOptions, CachePlugin, CacheStore, MemoryStore};
/// use cosmos_llm_cache::registry::meta;
///
/// /// Stands in for a networked store.
/// fn shared(options: &CacheOptions) -> Result<Box<dyn CacheStore>, CacheError> {
///     Ok(Box::new(MemoryStore::new(options.memory_cap())))
/// }
///
/// cosmos_llm_cache::registry::inventory::submit! {
///     CachePlugin { meta: meta!("shared").summary("A shared store"), build: shared }
/// }
///
/// let built = store("shared", &CacheOptions::default()).unwrap();
/// assert_eq!(built.len(), Some(0));
/// ```
pub struct CachePlugin {
    /// The store's name and aliases.
    pub meta: Meta,
    /// Builds the store.
    pub build: fn(&CacheOptions) -> Result<Box<dyn CacheStore>, CacheError>,
}

impl Plugin for CachePlugin {
    fn meta(&self) -> &Meta {
        &self.meta
    }
}

cosmos_llm_registry::inventory::collect!(CachePlugin);

/// Builds the store registered under `name`.
///
/// # Errors
///
/// Returns [`CacheError::NotFound`] naming the registered stores, or
/// whatever the store's constructor returns.
///
/// # Examples
///
/// ```
/// use cosmos_llm_cache::{store, CacheOptions};
///
/// assert!(store("memory", &CacheOptions::default()).is_ok());
/// assert!(store("disk", &CacheOptions::default()).is_err()); // needs a directory
/// ```
pub fn store(name: &str, options: &CacheOptions) -> Result<Box<dyn CacheStore>, CacheError> {
    let plugin = cosmos_llm_registry::require::<CachePlugin>("cache store", name)?;
    (plugin.build)(options)
}

fn build_memory(options: &CacheOptions) -> Result<Box<dyn CacheStore>, CacheError> {
    Ok(Box::new(MemoryStore::new(options.memory_cap())))
}

fn build_disk(options: &CacheOptions) -> Result<Box<dyn CacheStore>, CacheError> {
    Ok(Box::new(DiskStore::new(
        options.require_directory("disk")?,
    )?))
}

fn build_tiered(options: &CacheOptions) -> Result<Box<dyn CacheStore>, CacheError> {
    let back = DiskStore::new(options.require_directory("tiered")?)?;
    Ok(Box::new(TieredStore::new(
        MemoryStore::new(options.memory_cap()),
        back,
    )))
}

cosmos_llm_registry::inventory::submit! {
    CachePlugin {
        meta: meta!("memory").builtin().aliases(&["lru"]).summary("In-process LRU; gone when the process exits"),
        build: build_memory,
    }
}

cosmos_llm_registry::inventory::submit! {
    CachePlugin {
        meta: meta!("disk").builtin().summary("Sharded directory of JSON files; needs a directory"),
        build: build_disk,
    }
}

cosmos_llm_registry::inventory::submit! {
    CachePlugin {
        meta: meta!("tiered").builtin().summary("Memory LRU in front of disk; needs a directory"),
        build: build_tiered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_stores_require_a_directory() {
        for name in ["disk", "tiered"] {
            let err = store(name, &CacheOptions::default()).err().unwrap();
            assert_eq!(
                err.to_string(),
                format!("cache configuration error: the '{name}' store needs a directory")
            );
        }
    }

    #[test]
    fn unknown_stores_list_the_registered_ones() {
        let err = store("redis", &CacheOptions::default()).err().unwrap();
        assert_eq!(
            err.to_string(),
            "unknown cache store 'redis'; available: disk, memory, tiered"
        );
    }

    #[test]
    fn the_memory_cap_is_honoured() {
        let options = CacheOptions {
            max_memory_entries: Some(1),
            ..CacheOptions::default()
        };
        let built = store("lru", &options).unwrap();
        built.put("a", &serde_json::json!(1)).unwrap();
        built.put("b", &serde_json::json!(2)).unwrap();
        assert_eq!(built.len(), Some(1));
    }
}
