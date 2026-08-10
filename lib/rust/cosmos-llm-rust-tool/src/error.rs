use std::time::Duration;

use thiserror::Error;

/// Errors produced by the tool framework.
#[derive(Debug, Error)]
#[non_exhaustive]
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

    /// An async tool was run through the synchronous [`Executor::execute`]
    /// path.
    ///
    /// [`Executor::execute`]: crate::Executor::execute
    ///
    /// Reported rather than worked around: blocking to drive the future would
    /// deadlock on a current-thread runtime, and spawning a runtime inside a
    /// library is not a decision this crate should make for its caller. Use
    /// [`Executor::execute_async`](crate::Executor::execute_async).
    #[error(
        "tool '{0}' has an async handler and cannot be run synchronously; \
         use Executor::execute_async"
    )]
    AsyncHandler(String),

    /// The tool exceeded its [`ToolDefinition::timeout`].
    ///
    /// [`ToolDefinition::timeout`]: crate::ToolDefinition::timeout
    #[error("tool '{tool}' timed out after {elapsed:?}")]
    Timeout {
        /// Name of the tool that timed out.
        tool: String,
        /// The limit that was exceeded.
        elapsed: Duration,
    },
}
