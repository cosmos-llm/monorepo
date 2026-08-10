pub mod anthropic;
#[cfg(feature = "mock")]
pub mod mock;
pub mod openai;
pub mod openrouter;

use crate::error::CosmosError;
use crate::types::{CompletionRequest, CompletionResponse, StreamChunk};
use futures_util::Stream;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

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
        let provider = self.name().to_owned();
        Box::pin(async move {
            Err(CosmosError::Streaming {
                provider,
                message: "this provider does not implement streaming".to_owned(),
            })
        })
    }

    /// Returns this provider's canonical name, as accepted by [`resolve`].
    ///
    /// Used to attribute errors ([`CosmosError::provider`]) and to identify a
    /// provider in logs. The default is `"unknown"` so an out-of-tree
    /// implementation still compiles, but every provider should override it.
    fn name(&self) -> &str {
        "unknown"
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

/// Reads a `Retry-After` header into a [`Duration`].
///
/// The header comes in two forms. The delay-seconds form is what every provider
/// this crate supports actually sends; the HTTP-date form is legal and is
/// treated as absent rather than parsed, since acting on a wrong date is worse
/// than falling back to computed backoff.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use cosmos_llm::providers::retry_after_from_headers;
///
/// let mut headers = reqwest::header::HeaderMap::new();
/// headers.insert("retry-after", "20".parse().unwrap());
/// assert_eq!(retry_after_from_headers(&headers), Some(Duration::from_secs(20)));
/// ```
pub fn retry_after_from_headers(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Classifies a `reqwest` failure as a timeout or a network error.
///
/// A bare `?` on a `reqwest` call yields [`CosmosError::Http`], which names no
/// provider and reports no elapsed time. Routing transport failures through
/// this instead means a caller can see which provider failed and whether
/// retrying is sensible.
///
/// The timeout's `elapsed` is not recoverable from a `reqwest::Error`, so it is
/// reported as zero; the variant carries the field because a caller measuring
/// its own deadline can supply a real one.
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::providers::transport_error;
///
/// # tokio_test::block_on(async {
/// let result = reqwest::get("http://127.0.0.1:1/nope")
///     .await
///     .map_err(|e| transport_error("openai", e));
/// assert!(result.unwrap_err().is_retryable());
/// # })
/// ```
pub fn transport_error(provider: &str, source: reqwest::Error) -> CosmosError {
    if source.is_timeout() {
        return CosmosError::Timeout {
            provider: provider.to_owned(),
            elapsed: Duration::ZERO,
        };
    }
    CosmosError::Network {
        provider: provider.to_owned(),
        source,
    }
}

/// Maps an OpenAI-shaped error response onto a [`CosmosError`].
///
/// Shared by the OpenAI and OpenRouter providers, which use the same
/// `{"error": {"message", "type", "code", "param"}}` envelope.
///
/// The status code alone is not enough. OpenAI reports an exhausted quota as
/// 429 with `code: "insufficient_quota"` — retrying that is pointless, while
/// retrying an ordinary 429 is exactly right. Likewise a 400 may be a bad
/// parameter, a context-length overflow the caller can trim and retry, or a
/// content filter. Reading `type` and `code` is what makes those distinctions
/// available to a caller.
///
/// # Examples
///
/// ```
/// use cosmos_llm::providers::map_openai_error;
/// use serde_json::json;
///
/// let body = json!({"error": {"message": "quota exceeded", "code": "insufficient_quota"}});
/// let err = map_openai_error("openai", 429, &body, None);
/// // A 429 that a retry cannot fix.
/// assert!(!err.is_retryable());
/// ```
pub fn map_openai_error(
    provider: &str,
    status: u16,
    body: &Value,
    retry_after: Option<Duration>,
) -> CosmosError {
    let err = &body["error"];
    let message = err["message"]
        .as_str()
        .unwrap_or("unknown error")
        .to_owned();
    let code = err["code"].as_str().unwrap_or("");
    let kind = err["type"].as_str().unwrap_or("");
    let param = err["param"].as_str().map(str::to_owned);
    let provider = provider.to_owned();

    // Codes take priority over status: the same 429 means two different things
    // depending on whether the account is throttled or empty.
    match code {
        "insufficient_quota" | "billing_hard_limit_reached" => {
            return CosmosError::InsufficientQuota { provider, message }
        }
        "context_length_exceeded" => {
            return CosmosError::ContextLength {
                provider,
                limit: None,
            }
        }
        "content_filter" => {
            return CosmosError::ContentFiltered {
                provider,
                reason: message,
            }
        }
        "model_not_found" => {
            return CosmosError::ModelNotFound {
                provider,
                model: param.unwrap_or(message),
            }
        }
        _ => {}
    }

    match status {
        401 | 403 => CosmosError::Authentication { provider, message },
        // OpenRouter charges per request and reports an empty balance as 402.
        402 => CosmosError::InsufficientQuota { provider, message },
        429 => CosmosError::RateLimit {
            provider,
            message,
            retry_after,
        },
        404 => {
            // A 404 from the completions endpoint is nearly always an unknown
            // model, and saying so is more useful than "not found".
            if kind == "invalid_request_error" && message.to_lowercase().contains("model") {
                CosmosError::ModelNotFound {
                    provider,
                    model: param.unwrap_or(message),
                }
            } else {
                CosmosError::ResourceNotFound {
                    provider,
                    resource: message,
                }
            }
        }
        400 | 422 => CosmosError::InvalidRequest {
            provider,
            message,
            param,
        },
        s if s >= 500 => CosmosError::Server {
            provider,
            status: s,
            message,
        },
        s => CosmosError::Api {
            provider,
            message: format!("unexpected status {s}: {message}"),
        },
    }
}

/// Maps an Anthropic error response onto a [`CosmosError`].
///
/// Anthropic's envelope is `{"type": "error", "error": {"type", "message"}}`,
/// and its `error.type` is more specific than the status code: an
/// `overloaded_error` arrives as 529, and a context overflow arrives as a 400
/// `invalid_request_error` whose message mentions the token limit. Classifying
/// on `type` first is what lets a caller tell those apart.
///
/// # Examples
///
/// ```
/// use cosmos_llm::providers::map_anthropic_error;
/// use serde_json::json;
///
/// let body = json!({"error": {"type": "overloaded_error", "message": "Overloaded"}});
/// let err = map_anthropic_error(529, &body, None);
/// assert!(err.is_retryable());
/// ```
pub fn map_anthropic_error(
    status: u16,
    body: &Value,
    retry_after: Option<Duration>,
) -> CosmosError {
    const PROVIDER: &str = "anthropic";

    let err = &body["error"];
    let message = err["message"]
        .as_str()
        .unwrap_or("unknown error")
        .to_owned();
    let kind = err["type"].as_str().unwrap_or("");
    let provider = PROVIDER.to_owned();

    match kind {
        "authentication_error" | "permission_error" => {
            return CosmosError::Authentication { provider, message }
        }
        "rate_limit_error" => {
            return CosmosError::RateLimit {
                provider,
                message,
                retry_after,
            }
        }
        // 529. Transient by definition, and the reason retrying Anthropic
        // works more often than retrying most providers.
        "overloaded_error" | "api_error" => {
            return CosmosError::Server {
                provider,
                status,
                message,
            }
        }
        "not_found_error" => {
            return CosmosError::ResourceNotFound {
                provider,
                resource: message,
            }
        }
        "invalid_request_error" => {
            let lowered = message.to_lowercase();
            // Anthropic reports both of these as invalid_request_error; the
            // message is the only signal, and both are actionable.
            if lowered.contains("prompt is too long") || lowered.contains("max_tokens") {
                return CosmosError::ContextLength {
                    provider,
                    limit: None,
                };
            }
            if lowered.contains("model") && lowered.contains("not") {
                return CosmosError::ModelNotFound {
                    provider,
                    model: message,
                };
            }
            return CosmosError::InvalidRequest {
                provider,
                message,
                param: None,
            };
        }
        _ => {}
    }

    match status {
        401 | 403 => CosmosError::Authentication { provider, message },
        429 => CosmosError::RateLimit {
            provider,
            message,
            retry_after,
        },
        404 => CosmosError::ResourceNotFound {
            provider,
            resource: message,
        },
        400 | 422 => CosmosError::InvalidRequest {
            provider,
            message,
            param: None,
        },
        s if s >= 500 => CosmosError::Server {
            provider,
            status: s,
            message,
        },
        s => CosmosError::Api {
            provider,
            message: format!("unexpected status {s}: {message}"),
        },
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
///
/// `provider` attributes transport failures mid-stream, which are reported as
/// [`CosmosError::Network`] and are retryable: a connection dropped halfway
/// through a stream is the case a retry layer most needs to recognise.
pub(crate) fn stream_from_response<F>(
    provider: &str,
    resp: reqwest::Response,
    mut parse: F,
) -> CompletionStream
where
    F: FnMut(&str) -> Result<Option<StreamChunk>, CosmosError> + Send + 'static,
{
    use futures_util::StreamExt;

    let provider = provider.to_owned();
    let mut decoder = crate::sse::SseDecoder::new();
    let mut bytes = resp.bytes_stream();

    Box::pin(async_stream::stream! {
        while let Some(item) = bytes.next().await {
            let chunk = match item {
                Ok(bytes) => bytes,
                Err(e) => {
                    yield Err(CosmosError::Network { provider, source: e });
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
        // Resolvable by name so config-driven code can select it without a
        // compile-time branch. It has no script, so every request reports the
        // script as exhausted; build one directly to script responses.
        #[cfg(feature = "mock")]
        "mock" => Ok(Box::new(mock::MockProvider::new())),
        other => Err(CosmosError::UnsupportedProvider {
            name: other.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn openai_body(fields: Value) -> Value {
        json!({ "error": fields })
    }

    #[test]
    fn openai_status_codes_map_to_variants() {
        let body = openai_body(json!({"message": "nope"}));
        for (status, retryable) in [
            (401, false),
            (403, false),
            (400, false),
            (422, false),
            (429, true),
            (500, true),
            (503, true),
        ] {
            let err = map_openai_error("openai", status, &body, None);
            assert_eq!(err.is_retryable(), retryable, "status {status}: {err}");
            assert_eq!(err.provider(), "openai");
        }
    }

    #[test]
    fn openai_quota_exhaustion_is_a_429_that_is_not_retryable() {
        // The distinction the status code alone cannot make: throttling clears
        // on its own, an empty account does not.
        let body = openai_body(json!({"message": "quota", "code": "insufficient_quota"}));
        let err = map_openai_error("openai", 429, &body, Some(Duration::from_secs(30)));
        assert!(matches!(err, CosmosError::InsufficientQuota { .. }));
        assert!(!err.is_retryable());
        // No retry_after either: there is nothing to wait for.
        assert_eq!(err.retry_after(), None);
    }

    #[test]
    fn openai_context_overflow_is_distinguished_from_a_bad_request() {
        let body = openai_body(json!({
            "message": "maximum context length",
            "code": "context_length_exceeded",
        }));
        let err = map_openai_error("openai", 400, &body, None);
        assert!(matches!(err, CosmosError::ContextLength { .. }));
    }

    #[test]
    fn openai_content_filter_is_reported_as_such() {
        let body = openai_body(json!({"message": "flagged", "code": "content_filter"}));
        let err = map_openai_error("openai", 400, &body, None);
        assert!(
            matches!(err, CosmosError::ContentFiltered { reason: ref r, .. } if r == "flagged")
        );
    }

    #[test]
    fn openai_unknown_model_names_the_model() {
        let body = openai_body(json!({
            "message": "The model 'gpt-9' does not exist",
            "type": "invalid_request_error",
            "param": "gpt-9",
        }));
        let err = map_openai_error("openai", 404, &body, None);
        assert!(matches!(err, CosmosError::ModelNotFound { model: ref m, .. } if m == "gpt-9"));
    }

    #[test]
    fn openai_invalid_request_keeps_the_offending_param() {
        let body = openai_body(json!({
            "message": "unsupported value",
            "type": "invalid_request_error",
            "param": "temperature",
        }));
        let err = map_openai_error("openai", 400, &body, None);
        assert!(
            matches!(err, CosmosError::InvalidRequest { param: Some(ref p), .. } if p == "temperature")
        );
    }

    #[test]
    fn openai_rate_limit_carries_retry_after() {
        let body = openai_body(json!({"message": "slow down"}));
        let err = map_openai_error("openai", 429, &body, Some(Duration::from_secs(12)));
        assert_eq!(err.retry_after(), Some(Duration::from_secs(12)));
    }

    #[test]
    fn anthropic_error_types_map_to_variants() {
        for (kind, retryable) in [
            ("authentication_error", false),
            ("permission_error", false),
            ("rate_limit_error", true),
            ("overloaded_error", true),
            ("api_error", true),
            ("not_found_error", false),
            ("invalid_request_error", false),
        ] {
            let body = json!({"error": {"type": kind, "message": "m"}});
            let err = map_anthropic_error(500, &body, None);
            assert_eq!(err.is_retryable(), retryable, "{kind}: {err}");
            assert_eq!(err.provider(), "anthropic");
        }
    }

    #[test]
    fn anthropic_prompt_too_long_is_a_context_error() {
        // Anthropic reports this as invalid_request_error; only the message
        // separates a trimmable overflow from a genuinely malformed request.
        let body = json!({
            "error": {"type": "invalid_request_error", "message": "prompt is too long: 250000 tokens"}
        });
        let err = map_anthropic_error(400, &body, None);
        assert!(matches!(err, CosmosError::ContextLength { .. }));
    }

    #[test]
    fn anthropic_falls_back_to_status_when_type_is_unknown() {
        let body = json!({"error": {"type": "some_future_error", "message": "m"}});
        assert!(matches!(
            map_anthropic_error(503, &body, None),
            CosmosError::Server { .. }
        ));
        assert!(matches!(
            map_anthropic_error(401, &body, None),
            CosmosError::Authentication { .. }
        ));
    }

    #[test]
    fn missing_error_body_still_classifies_by_status() {
        // Providers return HTML or an empty body on some failures; the status
        // is all there is to go on and must still produce a usable error.
        let err = map_openai_error("openai", 502, &Value::Null, None);
        assert!(matches!(err, CosmosError::Server { status: 502, .. }));
        assert!(err.is_retryable());
    }

    #[test]
    fn retry_after_reads_delay_seconds() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "20".parse().unwrap());
        assert_eq!(
            retry_after_from_headers(&headers),
            Some(Duration::from_secs(20))
        );
    }

    #[test]
    fn retry_after_ignores_http_date_form() {
        // Legal but unhandled: acting on a misparsed date is worse than
        // falling back to computed backoff.
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "retry-after",
            "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after_from_headers(&headers), None);
    }

    #[test]
    fn retry_after_absent_is_none() {
        assert_eq!(
            retry_after_from_headers(&reqwest::header::HeaderMap::new()),
            None
        );
    }
}
