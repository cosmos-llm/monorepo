use thiserror::Error;

/// Errors produced by the virtual filesystem.
#[derive(Debug, Error, PartialEq)]
pub enum VfsError {
    /// Filename is empty, `None`, or contains illegal characters.
    #[error("invalid filename: {0}")]
    InvalidName(String),

    /// Path contains a separator or null byte where one is not expected.
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// A value failed structural validation (e.g. wrong type).
    #[error("validation error: {0}")]
    Validation(String),
}
