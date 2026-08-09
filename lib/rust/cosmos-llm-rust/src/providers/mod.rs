pub mod anthropic;
pub mod openai;
pub mod openrouter;

use crate::error::CosmosError;
use crate::types::{CompletionRequest, CompletionResponse, StreamChunk};
use futures_util::Stream;
use std::future::Future;
use std::pin::Pin;

/// A stream of incremental completion chunks.
///
/// Yielded by [`Provider::stream_completion`] and
/// [`Client::stream_completion`](crate::Client::stream_completion). Each item
/// is either a [`StreamChunk`] or the error that ended the stream; a stream
/// yields at most one error, after which it is exhausted.
pub type CompletionStream =
    Pin<Box<dyn Stream<Item = Result<StreamChunk, CosmosError>> + Send + 'static>>;

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

    /// Sends a completion request and returns a stream of incremental chunks.
    ///
    /// The default implementation returns [`CosmosError::Streaming`]. A
    /// provider that overrides this must also override
    /// [`Provider::supports_streaming`] to return `true`.
    ///
    /// The returned stream is `'static`: it owns everything it needs, so it
    /// can outlive the borrow of `req` and be moved across tasks.
    ///
    /// # Errors
    ///
    /// Returns a [`CosmosError`] if the request is rejected before the stream
    /// opens. Errors encountered mid-stream are yielded as stream items.
    fn stream_completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionStream, CosmosError>> + Send + 'a>> {
        let _ = req;
        Box::pin(async move {
            Err(CosmosError::Streaming(
                "this provider does not implement streaming".to_owned(),
            ))
        })
    }

    /// Returns `true` if this provider implements streaming completions.
    ///
    /// This reports what the provider can actually do here, not what the
    /// upstream API offers. Providers that leave
    /// [`Provider::stream_completion`] at its default must leave this `false`.
    fn supports_streaming(&self) -> bool {
        false
    }
}

/// Converts an open SSE response into a [`CompletionStream`].
///
/// Handles the parts every provider shares: buffering bytes so an event split
/// across packets is still parsed as one unit, skipping the `[DONE]` sentinel,
/// and dropping events that a provider's `parse` maps to nothing.
///
/// `parse` receives the `data` payload of one event and returns the chunk it
/// represents, `None` for events this crate does not model, or an error that
/// terminates the stream.
///
/// The `parse` closure is `FnMut` so stateful parsers — Anthropic's, which
/// tracks which content block each delta belongs to — can be used directly.
pub(crate) fn stream_from_response<F>(resp: reqwest::Response, mut parse: F) -> CompletionStream
where
    F: FnMut(&str) -> Result<Option<StreamChunk>, CosmosError> + Send + 'static,
{
    use futures_util::StreamExt;

    let mut decoder = crate::sse::SseDecoder::new();
    let mut bytes = resp.bytes_stream();

    Box::pin(async_stream::stream! {
        while let Some(item) = bytes.next().await {
            let chunk = match item {
                Ok(bytes) => bytes,
                Err(e) => {
                    yield Err(CosmosError::Http(e));
                    return;
                }
            };

            for event in decoder.push(&chunk) {
                if event.is_done() {
                    return;
                }
                match parse(&event.data) {
                    Ok(Some(chunk)) => yield Ok(chunk),
                    Ok(None) => {}
                    Err(e) => {
                        yield Err(e);
                        return;
                    }
                }
            }
        }

        // A stream cut off without its terminating blank line may still have
        // a usable final event buffered.
        if let Some(event) = decoder.finish() {
            if !event.is_done() {
                match parse(&event.data) {
                    Ok(Some(chunk)) => yield Ok(chunk),
                    Ok(None) => {}
                    Err(e) => yield Err(e),
                }
            }
        }
    })
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
    resolve_with_base_url(name, api_key, None)
}

/// Resolves a provider name, optionally overriding its API root.
///
/// When `base_url` is `None` this is identical to [`resolve`]. Otherwise the
/// resolved provider is pointed at `base_url` instead of its default endpoint
/// — for an OpenAI-compatible server, a proxy, or a mock server in tests.
///
/// # Errors
///
/// Returns [`CosmosError::UnsupportedProvider`] when `name` is not
/// recognised.
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::providers::resolve_with_base_url;
///
/// let provider = resolve_with_base_url(
///     "openai",
///     Some("sk-test"),
///     Some("http://localhost:8080/v1"),
/// )
/// .unwrap();
/// ```
pub fn resolve_with_base_url(
    name: &str,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> Result<Box<dyn Provider>, CosmosError> {
    let key = api_key.map(str::to_owned);

    // Each arm applies `with_base_url` before boxing, since the builder
    // methods are inherent to the concrete provider types.
    match name.to_lowercase().as_str() {
        "openai" => {
            let mut p = openai::OpenAiProvider::new(key);
            if let Some(url) = base_url {
                p = p.with_base_url(url);
            }
            Ok(Box::new(p))
        }
        "anthropic" => {
            let mut p = anthropic::AnthropicProvider::new(key);
            if let Some(url) = base_url {
                p = p.with_base_url(url);
            }
            Ok(Box::new(p))
        }
        "openrouter" => {
            let mut p = openrouter::OpenRouterProvider::new(key);
            if let Some(url) = base_url {
                p = p.with_base_url(url);
            }
            Ok(Box::new(p))
        }
        other => Err(CosmosError::UnsupportedProvider(other.to_owned())),
    }
}
