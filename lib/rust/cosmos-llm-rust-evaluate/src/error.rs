use thiserror::Error;

/// Errors produced while configuring or running an evaluation.
///
/// A program or metric failing on one example is *not* one of these: that is
/// recorded on the example's [`Row`](crate::Row) and the run continues. These
/// are the failures that stop a run.
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum EvaluateError {
    /// The evaluation was configured with invalid arguments: zero
    /// concurrency, mismatched weights, an unknown metric parameter.
    #[error("{0}")]
    Config(String),

    /// More examples failed than [`Evaluator::max_errors`] allows.
    ///
    /// [`Evaluator::max_errors`]: crate::Evaluator::max_errors
    #[error(
        "evaluation stopped after {count} failures (max_errors: {max}); first failure: {first}"
    )]
    TooManyErrors {
        /// How many examples had failed when the run stopped.
        count: usize,
        /// The limit that was exceeded.
        max: usize,
        /// The first failure's message, which is usually the one to fix.
        first: String,
    },

    /// A metric could not score an example.
    #[error("metric failed: {0}")]
    Metric(String),

    /// A named metric was not found in the registry.
    #[error(transparent)]
    NotFound(#[from] cosmos_llm_registry::NotFound),
}
