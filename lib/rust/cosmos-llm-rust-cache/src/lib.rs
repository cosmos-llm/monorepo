//! # cosmos-llm-cache
//!
//! Swappable caches for JSON values, keyed by a stable digest of the request
//! that produced them.
//!
//! Mirrors the cache in the Ruby `cosmos-llm-predict` gem. Evaluation loops
//! and optimizers replay identical calls constantly, so a cache is the
//! difference between a devset sweep costing money every run and costing
//! money once. This crate knows nothing about LLMs: a request is any JSON
//! value, a cached response is any JSON value, and the predictor that uses it
//! decides what goes in each.
//!
//! ## Quick start
//!
//! ```
//! use cosmos_llm_cache::{fetch, store, CacheError, CacheOptions};
//! use serde_json::json;
//!
//! let dir = std::env::temp_dir().join(format!("cosmos-llm-cache-quick-{}", std::process::id()));
//! let cache = store("tiered", &CacheOptions::in_directory(&dir)).unwrap();
//!
//! let request = json!({ "model": "m", "messages": [{ "role": "user", "content": "2+2?" }] });
//! let (reply, hit) = fetch(cache.as_ref(), &request, || {
//!     // A real caller would send the request to a provider here.
//!     Ok::<_, CacheError>(json!({ "completion": "4" }))
//! })
//! .unwrap();
//! assert!(!hit);
//!
//! let (again, hit) = fetch(cache.as_ref(), &request, || Ok::<_, CacheError>(json!(null))).unwrap();
//! assert!(hit);
//! assert_eq!(again, reply);
//! # std::fs::remove_dir_all(&dir).unwrap();
//! ```
//!
//! ## Stores
//!
//! | Name     | Type            | Keeps entries        |
//! |----------|-----------------|----------------------|
//! | `memory` | [`MemoryStore`] | until the process exits |
//! | `disk`   | [`DiskStore`]   | on disk, across runs |
//! | `tiered` | [`TieredStore`] | both, memory first   |
//!
//! ## Swapping pieces
//!
//! [`CacheStore`] is the trait; [`CachePlugin`] registers an implementation
//! by name through [`cosmos_llm_registry`]. Register under a new name to add
//! a backend, or under `memory`, `disk`, or `tiered` to replace a built-in
//! everywhere it is looked up by name. Code that takes a `&dyn CacheStore`
//! never needs the registry at all.
//!
//! ## Command line
//!
//! With the `cli` feature this crate builds a `cosmos-llm-cache` binary for
//! computing keys and inspecting or clearing a cache directory. [`cli::main`]
//! is public, so a binary that links your own stores can reuse it.

#![warn(missing_docs)]

mod disk;
mod error;
mod key;
mod memory;
mod plugin;
mod store;
mod tiered;

#[cfg(feature = "cli")]
pub mod cli;

pub use disk::{DiskStats, DiskStore};
pub use error::CacheError;
pub use key::{key_for, validate_key, MAX_KEY_LEN};
pub use memory::{MemoryStore, DEFAULT_MAX_MEMORY_ENTRIES};
pub use plugin::{store, CacheOptions, CachePlugin};
pub use store::{fetch, CacheStore};
pub use tiered::TieredStore;

/// The registry crate, re-exported so plugins can register through it.
pub use cosmos_llm_registry as registry;
