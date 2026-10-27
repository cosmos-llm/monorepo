//! The core's error type.

use std::path::PathBuf;

/// Why a core operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    /// A config file did not evaluate, parse, or match [`CoreConfig`](crate::CoreConfig).
    #[error("config error: {0}")]
    Config(String),
    /// TrongoScript evaluation failed (syntax, sandbox, fuel, host builtin).
    #[error("trongo evaluation failed: {0}")]
    Eval(String),
    /// An I/O operation failed.
    #[error("{path}: {source}")]
    Io {
        /// The file or directory involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A session log is unreadable or inconsistent.
    #[error("session error: {0}")]
    Session(String),
    /// Another process holds the session file's lock.
    #[error("session {0} is open in another process")]
    SessionLocked(PathBuf),
    /// No session matches the request.
    #[error("no such session: {0}")]
    NoSuchSession(String),
    /// A builder was missing something it needs.
    #[error("core setup error: {0}")]
    Setup(String),
    /// The client failed.
    #[error(transparent)]
    Client(#[from] cosmos_llm::CosmosError),
    /// JSON (de)serialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl CoreError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Shorthand for results in this crate.
pub type Result<T, E = CoreError> = std::result::Result<T, E>;
