use thiserror::Error;

/// Errors produced by the context builder and renderers.
#[derive(Debug, Error, PartialEq)]
pub enum ContextError {
    /// A block name was empty or otherwise invalid.
    #[error("invalid block name: {0}")]
    InvalidName(String),

    /// Metadata failed validation.
    #[error("validation error: {0}")]
    Validation(String),

    /// The requested renderer is not registered.
    #[error("unknown renderer: {0}")]
    UnknownRenderer(String),
}
