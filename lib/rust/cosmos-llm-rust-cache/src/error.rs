use std::path::PathBuf;

use thiserror::Error;

/// Errors produced by cache stores.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CacheError {
    /// A file operation failed.
    #[error("cache I/O error at {}: {message}", path.display())]
    Io {
        /// The file or directory involved.
        path: PathBuf,
        /// What the operating system reported.
        message: String,
    },

    /// A store was built with options it cannot work with, such as a disk
    /// store with no directory.
    #[error("cache configuration error: {0}")]
    Config(String),

    /// A key contains characters that cannot safely name a file.
    ///
    /// Keys become file names in a [`DiskStore`](crate::DiskStore), so they
    /// are limited to ASCII letters, digits, `-`, and `_`. Every built-in
    /// store enforces the same rule, so swapping one for another never
    /// changes which keys are accepted.
    #[error("invalid cache key {0:?}: keys are 1-200 characters of [A-Za-z0-9_-]")]
    InvalidKey(String),

    /// No store is registered under the requested name.
    #[error(transparent)]
    NotFound(#[from] cosmos_llm_registry::NotFound),
}

impl CacheError {
    pub(crate) fn io(path: impl Into<PathBuf>, error: &std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            message: error.to_string(),
        }
    }
}
