pub mod anthropic;
pub mod openai;

use crate::error::CosmosError;
use crate::types::{CompletionRequest, CompletionResponse};
use std::future::Future;
use std::pin::Pin;

/// The common interface every provider must implement.
///
/// Providers are typically constructed with their API key and then called
/// through a [`Client`](crate::Client). Implement this trait to add support
/// for a new LLM backend.
pub trait Provider: Send + Sync {
    /// Sends a completion request and returns the full response.
    ///
    /// # Errors
    ///
    /// Returns a [`CosmosError`] on authentication failure, network error,
    /// rate limiting, or an invalid response from the provider.
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>>;

    /// Returns the list of model identifiers available from this provider.
    ///
    /// # Errors
    ///
    /// Returns a [`CosmosError`] on authentication failure or network error.
    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, CosmosError>> + Send + 'a>>;

    /// Returns `true` if this provider supports streaming completions.
    fn supports_streaming(&self) -> bool {
        false
    }
}

/// Resolves a provider name string to a boxed [`Provider`].
///
/// Reads the API key from the supplied key string. Looks up known providers
/// by name (case-insensitive).
///
/// # Errors
///
/// Returns [`CosmosError::UnsupportedProvider`] when `name` is not
/// recognised.
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::providers::resolve;
///
/// let provider = resolve("openai", Some("sk-test")).unwrap();
/// assert!(provider.supports_streaming());
/// ```
pub fn resolve(name: &str, api_key: Option<&str>) -> Result<Box<dyn Provider>, CosmosError> {
    match name.to_lowercase().as_str() {
        "openai" => Ok(Box::new(openai::OpenAiProvider::new(
            api_key.map(str::to_owned),
        ))),
        "anthropic" => Ok(Box::new(anthropic::AnthropicProvider::new(
            api_key.map(str::to_owned),
        ))),
        other => Err(CosmosError::UnsupportedProvider(other.to_owned())),
    }
}
