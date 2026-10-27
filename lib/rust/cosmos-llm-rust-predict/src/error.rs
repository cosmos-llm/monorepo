use std::error::Error as StdError;

use thiserror::Error;

use cosmos_llm_adapter::AdapterError;
use cosmos_llm_cache::CacheError;
use cosmos_llm_signature::SignatureError;

/// A boxed error from a [`LanguageModel`](crate::LanguageModel)
/// implementation.
pub type BoxError = Box<dyn StdError + Send + Sync>;

/// Errors produced while running a predictor.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PredictError {
    /// The inputs did not fit the signature.
    #[error(transparent)]
    Signature(#[from] SignatureError),

    /// The adapter could not render the prompt.
    #[error(transparent)]
    Adapter(AdapterError),

    /// The model's reply could not be parsed, even after correction turns.
    #[error("could not parse the model's reply after {attempts} attempt(s): {message}")]
    Parse {
        /// The adapter's complaint about the last reply.
        message: String,
        /// The last reply, verbatim.
        completion: String,
        /// How many replies were tried.
        attempts: usize,
    },

    /// The language model failed.
    #[error("language model error: {0}")]
    Model(#[source] BoxError),

    /// The cache failed. Reads that fail are treated as misses by the
    /// built-in stores; this is raised only for a store that reports one.
    #[error(transparent)]
    Cache(#[from] CacheError),

    /// The predictor was configured inconsistently.
    #[error("{0}")]
    Config(String),

    /// A named strategy or backend was not found.
    #[error(transparent)]
    NotFound(#[from] cosmos_llm_registry::NotFound),

    /// No attempt of a sampling module produced a prediction.
    #[error("all {attempts} attempt(s) failed; last error: {last}")]
    AllAttemptsFailed {
        /// How many attempts were made.
        attempts: usize,
        /// The final attempt's error.
        last: Box<PredictError>,
    },
}

impl From<AdapterError> for PredictError {
    fn from(error: AdapterError) -> Self {
        match error {
            AdapterError::Signature(inner) => Self::Signature(inner),
            AdapterError::NotFound(inner) => Self::NotFound(inner),
            other => Self::Adapter(other),
        }
    }
}

#[cfg(feature = "client")]
impl From<cosmos_llm::CosmosError> for PredictError {
    fn from(error: cosmos_llm::CosmosError) -> Self {
        Self::Model(Box::new(error))
    }
}
