use thiserror::Error;

/// Errors produced by the tool framework.
#[derive(Debug, Error)]
pub enum ToolError {
    /// A parameter failed type or presence validation.
    #[error("validation error for '{param}': {message}")]
    Validation { param: String, message: String },

    /// The tool has no execution handler registered.
    #[error("tool '{0}' has no handler")]
    NoHandler(String),

    /// The handler returned an error.
    #[error("execution error in '{tool}': {message}")]
    Execution { tool: String, message: String },

    /// Parameter type is not recognised.
    #[error("invalid parameter type: {0}")]
    InvalidType(String),

    /// A required parameter was missing.
    #[error("missing required parameter: {0}")]
    MissingParam(String),
}
