use cosmos_llm_signature::SignatureError;
use thiserror::Error;

/// Errors produced while formatting a prompt or parsing a reply.
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum AdapterError {
    /// The model's reply could not be turned into the signature's outputs.
    ///
    /// Carries the reply verbatim, because the usual recovery is to show the
    /// model what it wrote and ask again in the right format.
    #[error("{message}")]
    Parse {
        /// What went wrong, worded so it can be fed back to the model.
        message: String,
        /// The reply that failed to parse.
        completion: String,
    },

    /// The signature could not be rendered, e.g. a field type is unknown.
    #[error(transparent)]
    Signature(#[from] SignatureError),

    /// No adapter is registered under the requested name.
    #[error(transparent)]
    NotFound(#[from] cosmos_llm_registry::NotFound),
}

impl AdapterError {
    /// Builds an [`AdapterError::Parse`].
    pub fn parse(message: impl Into<String>, completion: impl Into<String>) -> Self {
        Self::Parse {
            message: message.into(),
            completion: completion.into(),
        }
    }
}
