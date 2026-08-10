use std::time::Duration;

use thiserror::Error;

/// Provider name used for errors that do not originate from a provider.
///
/// Configuration problems, local JSON parse failures, and auth-file loading
/// have no upstream vendor to blame, but every variant carries a `provider`
/// field so a caller can log or match on it without special cases. Those use
/// this.
///
/// # Examples
///
/// ```
/// use cosmos_llm::{CosmosError, SYSTEM};
///
/// let err = CosmosError::Configuration {
///     provider: SYSTEM.to_owned(),
///     message: "no API key configured".into(),
/// };
/// assert_eq!(err.provider(), SYSTEM);
/// ```
pub const SYSTEM: &str = "SYSTEM";

/// All errors that can be produced by this library.
///
/// The variants mirror the Ruby client's error hierarchy
/// (`Cosmos::Llm::Errors`) so a team running both against one deployment gets
/// the same classification from either. Two variants have no Ruby equivalent
/// yet — [`CosmosError::ContentFiltered`] and [`CosmosError::ContextLength`] —
/// because a caller can act on both and folding them into a generic API error
/// loses that.
///
/// Use [`CosmosError::is_retryable`] rather than matching on variants to decide
/// whether to retry; that keeps callers working when a variant is added.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use cosmos_llm::CosmosError;
///
/// let err = CosmosError::RateLimit {
///     provider: "openai".into(),
///     message: "too many requests".into(),
///     retry_after: Some(Duration::from_secs(20)),
/// };
/// assert!(err.is_retryable());
/// assert_eq!(err.retry_after(), Some(Duration::from_secs(20)));
/// ```
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CosmosError {
    /// An API failure that fits no more specific variant. Ruby: `APIError`.
    #[error("{provider} api error: {message}")]
    Api {
        /// Provider that produced the error.
        provider: String,
        /// Message reported by the provider.
        message: String,
    },

    /// The provider returned 429. Ruby: `RateLimitError`.
    ///
    /// Retryable. `retry_after` carries the `Retry-After` header when the
    /// provider sent one; a retry layer should prefer it over computed backoff,
    /// since the provider knows its own limits.
    #[error("{provider} rate limit exceeded: {message}")]
    RateLimit {
        /// Provider that produced the error.
        provider: String,
        /// Message reported by the provider.
        message: String,
        /// Delay requested by the provider's `Retry-After` header, if any.
        retry_after: Option<Duration>,
    },

    /// The API key is missing, invalid, or rejected (401/403).
    /// Ruby: `AuthenticationError`. Never retryable — retrying spends time and
    /// changes nothing.
    #[error("{provider} authentication error: {message}")]
    Authentication {
        /// Provider that produced the error.
        provider: String,
        /// Message reported by the provider.
        message: String,
    },

    /// The request was malformed or violated a constraint (400/422).
    /// Ruby: `InvalidRequestError`. Never retryable.
    #[error("{provider} invalid request: {message}")]
    InvalidRequest {
        /// Provider that produced the error.
        provider: String,
        /// Message reported by the provider.
        message: String,
        /// Offending parameter, when the provider identifies one.
        param: Option<String>,
    },

    /// A requested resource does not exist. Ruby: `ResourceNotFoundError`.
    #[error("{provider} resource not found: {resource}")]
    ResourceNotFound {
        /// Provider that produced the error.
        provider: String,
        /// Resource that was not found.
        resource: String,
    },

    /// The request timed out. Ruby: `TimeoutError`. Retryable.
    #[error("{provider} request timed out after {elapsed:?}")]
    Timeout {
        /// Provider that produced the error.
        provider: String,
        /// How long the request ran before timing out.
        elapsed: Duration,
    },

    /// The provider returned 5xx. Ruby: `ServerError`. Retryable.
    #[error("{provider} server error (status {status}): {message}")]
    Server {
        /// Provider that produced the error.
        provider: String,
        /// HTTP status code.
        status: u16,
        /// Message reported by the provider.
        message: String,
    },

    /// The provider name is not recognised. Ruby: `UnsupportedProviderError`.
    #[error("unsupported provider: {name}")]
    UnsupportedProvider {
        /// The unrecognised name.
        name: String,
    },

    /// A configuration value is missing or invalid.
    /// Ruby: `ConfigurationError`.
    #[error("{provider} configuration error: {message}")]
    Configuration {
        /// Provider the configuration belongs to, or [`SYSTEM`].
        provider: String,
        /// What is wrong with the configuration.
        message: String,
    },

    /// The requested model does not exist or is not accessible.
    /// Ruby: `ModelNotFoundError`.
    ///
    /// Distinct from [`CosmosError::ResourceNotFound`] because a caller can act
    /// on it specifically, by suggesting or falling back to another model.
    #[error("{provider} model not found: {model}")]
    ModelNotFound {
        /// Provider that produced the error.
        provider: String,
        /// Model identifier that was rejected.
        model: String,
    },

    /// The account is out of quota or credits.
    /// Ruby: `InsufficientQuotaError`. Not retryable — retrying spends nothing
    /// but time.
    #[error("{provider} insufficient quota: {message}")]
    InsufficientQuota {
        /// Provider that produced the error.
        provider: String,
        /// Message reported by the provider.
        message: String,
    },

    /// The response could not be parsed. Ruby: `InvalidResponseError`.
    ///
    /// Not retryable: a response this client cannot read will not read
    /// differently next time.
    #[error("{provider} invalid response: {message}")]
    InvalidResponse {
        /// Provider that produced the error, or [`SYSTEM`] for a local parse.
        provider: String,
        /// What could not be parsed.
        message: String,
    },

    /// The request could not reach the provider. Ruby: `NetworkError`.
    /// Retryable — connection resets and DNS failures are usually transient.
    #[error("{provider} network error: {source}")]
    Network {
        /// Provider that was being contacted.
        provider: String,
        /// Underlying transport error.
        source: reqwest::Error,
    },

    /// A streaming response failed. Ruby: `StreamingError`.
    #[error("{provider} streaming error: {message}")]
    Streaming {
        /// Provider that produced the error.
        provider: String,
        /// What went wrong with the stream.
        message: String,
    },

    /// A content filter or safety system blocked the request or response.
    ///
    /// No Ruby equivalent yet; scheduled for the gem in phase 080. Not
    /// retryable — the same input produces the same block.
    #[error("{provider} content filtered: {reason}")]
    ContentFiltered {
        /// Provider that produced the error.
        provider: String,
        /// Reason reported by the provider.
        reason: String,
    },

    /// The request exceeded the model's context window.
    ///
    /// No Ruby equivalent yet; scheduled for the gem in phase 080. Not
    /// retryable as-is, but a caller that trims the context can retry — which
    /// is why this is distinct from a generic invalid request.
    #[error("{provider} context length exceeded{}", match limit {
        Some(n) => format!(" (limit {n} tokens)"),
        None => String::new(),
    })]
    ContextLength {
        /// Provider that produced the error.
        provider: String,
        /// Token limit, when the provider reports it.
        limit: Option<u32>,
    },

    /// An HTTP transport error with no provider attribution.
    ///
    /// Prefer [`CosmosError::Network`], which names the provider. This variant
    /// exists so `?` still works on `reqwest` calls.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// A JSON serialisation or deserialisation error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

impl CosmosError {
    /// Returns `true` if retrying the request could plausibly succeed.
    ///
    /// Retryable: rate limits, timeouts, 5xx, network failures. Everything else
    /// is a condition that a second identical request will hit again.
    ///
    /// Prefer this over matching on variants — the enum is
    /// `#[non_exhaustive]`, and a caller matching by hand has to be revisited
    /// every time a variant lands.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CosmosError, SYSTEM};
    ///
    /// let rate_limited = CosmosError::RateLimit {
    ///     provider: "openai".into(),
    ///     message: "slow down".into(),
    ///     retry_after: None,
    /// };
    /// assert!(rate_limited.is_retryable());
    ///
    /// let bad_key = CosmosError::Authentication {
    ///     provider: "openai".into(),
    ///     message: "invalid api key".into(),
    /// };
    /// assert!(!bad_key.is_retryable());
    /// ```
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimit { .. }
            | Self::Timeout { .. }
            | Self::Server { .. }
            | Self::Network { .. } => true,

            // A transport-level `reqwest` error is retryable for the same
            // reason `Network` is, except for a timeout it already reports.
            Self::Http(e) => e.is_timeout() || e.is_connect() || e.is_request(),

            Self::Api { .. }
            | Self::Authentication { .. }
            | Self::InvalidRequest { .. }
            | Self::ResourceNotFound { .. }
            | Self::UnsupportedProvider { .. }
            | Self::Configuration { .. }
            | Self::ModelNotFound { .. }
            | Self::InsufficientQuota { .. }
            | Self::InvalidResponse { .. }
            | Self::Streaming { .. }
            | Self::ContentFiltered { .. }
            | Self::ContextLength { .. }
            | Self::Json(_) => false,
        }
    }

    /// Returns the delay the provider asked for, when it sent one.
    ///
    /// Only [`CosmosError::RateLimit`] carries this, populated from the
    /// `Retry-After` header. A retry layer should prefer it over its own
    /// backoff curve, and cap it — a provider asking for an hour is a failure,
    /// not a sleep.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use cosmos_llm::CosmosError;
    ///
    /// let err = CosmosError::RateLimit {
    ///     provider: "anthropic".into(),
    ///     message: "too many requests".into(),
    ///     retry_after: Some(Duration::from_secs(5)),
    /// };
    /// assert_eq!(err.retry_after(), Some(Duration::from_secs(5)));
    /// ```
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimit { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Returns the provider this error is attributed to.
    ///
    /// [`SYSTEM`] for errors with no upstream provider. [`CosmosError::Http`]
    /// and [`CosmosError::Json`] report [`SYSTEM`] too, since neither records
    /// which provider was being called.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::CosmosError;
    ///
    /// let err = CosmosError::Server {
    ///     provider: "openai".into(),
    ///     status: 503,
    ///     message: "overloaded".into(),
    /// };
    /// assert_eq!(err.provider(), "openai");
    /// ```
    pub fn provider(&self) -> &str {
        match self {
            Self::Api { provider, .. }
            | Self::RateLimit { provider, .. }
            | Self::Authentication { provider, .. }
            | Self::InvalidRequest { provider, .. }
            | Self::ResourceNotFound { provider, .. }
            | Self::Timeout { provider, .. }
            | Self::Server { provider, .. }
            | Self::Configuration { provider, .. }
            | Self::ModelNotFound { provider, .. }
            | Self::InsufficientQuota { provider, .. }
            | Self::InvalidResponse { provider, .. }
            | Self::Network { provider, .. }
            | Self::Streaming { provider, .. }
            | Self::ContentFiltered { provider, .. }
            | Self::ContextLength { provider, .. } => provider,

            Self::UnsupportedProvider { name } => name,
            Self::Http(_) | Self::Json(_) => SYSTEM,
        }
    }

    /// Builds a [`CosmosError::Configuration`] not tied to any provider.
    ///
    /// Shorthand for the common case of a local configuration problem.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CosmosError, SYSTEM};
    ///
    /// let err = CosmosError::config("CLLM__HOME is not a directory");
    /// assert_eq!(err.provider(), SYSTEM);
    /// ```
    pub fn config(message: impl Into<String>) -> Self {
        Self::Configuration {
            provider: SYSTEM.to_owned(),
            message: message.into(),
        }
    }

    /// Builds a [`CosmosError::Streaming`] for the given provider.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::CosmosError;
    ///
    /// let err = CosmosError::streaming("openai", "malformed chunk");
    /// assert_eq!(err.provider(), "openai");
    /// ```
    pub fn streaming(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Streaming {
            provider: provider.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate_limit(retry_after: Option<Duration>) -> CosmosError {
        CosmosError::RateLimit {
            provider: "openai".into(),
            message: "slow down".into(),
            retry_after,
        }
    }

    #[test]
    fn transient_conditions_are_retryable() {
        assert!(rate_limit(None).is_retryable());
        assert!(CosmosError::Timeout {
            provider: "openai".into(),
            elapsed: Duration::from_secs(30),
        }
        .is_retryable());
        assert!(CosmosError::Server {
            provider: "openai".into(),
            status: 503,
            message: "overloaded".into(),
        }
        .is_retryable());
    }

    #[test]
    fn caller_errors_are_not_retryable() {
        for err in [
            CosmosError::Authentication {
                provider: "openai".into(),
                message: "bad key".into(),
            },
            CosmosError::InvalidRequest {
                provider: "openai".into(),
                message: "bad param".into(),
                param: Some("temperature".into()),
            },
            // Retrying an out-of-quota request spends time and nothing else.
            CosmosError::InsufficientQuota {
                provider: "openai".into(),
                message: "add credits".into(),
            },
            CosmosError::ModelNotFound {
                provider: "openai".into(),
                model: "gpt-9".into(),
            },
            CosmosError::ContentFiltered {
                provider: "openai".into(),
                reason: "policy".into(),
            },
            // A caller that trims can retry, but the same request cannot.
            CosmosError::ContextLength {
                provider: "openai".into(),
                limit: Some(128_000),
            },
        ] {
            assert!(!err.is_retryable(), "{err} should not be retryable");
        }
    }

    #[test]
    fn retry_after_only_on_rate_limit() {
        assert_eq!(
            rate_limit(Some(Duration::from_secs(20))).retry_after(),
            Some(Duration::from_secs(20))
        );
        assert_eq!(rate_limit(None).retry_after(), None);
        assert_eq!(
            CosmosError::Server {
                provider: "openai".into(),
                status: 500,
                message: "boom".into(),
            }
            .retry_after(),
            None
        );
    }

    #[test]
    fn provider_is_reported_for_every_variant() {
        assert_eq!(rate_limit(None).provider(), "openai");
        assert_eq!(CosmosError::config("no key").provider(), SYSTEM);
        assert_eq!(
            CosmosError::UnsupportedProvider {
                name: "nope".into()
            }
            .provider(),
            "nope"
        );
    }

    #[test]
    fn messages_name_the_provider() {
        let text = rate_limit(None).to_string();
        assert!(text.contains("openai"), "{text}");
        assert!(text.contains("slow down"), "{text}");
    }

    #[test]
    fn context_length_message_includes_limit_when_known() {
        let with_limit = CosmosError::ContextLength {
            provider: "openai".into(),
            limit: Some(128_000),
        }
        .to_string();
        assert!(with_limit.contains("128000"), "{with_limit}");

        let without = CosmosError::ContextLength {
            provider: "openai".into(),
            limit: None,
        }
        .to_string();
        assert!(!without.contains("limit"), "{without}");
    }

    #[test]
    fn json_errors_are_not_retryable() {
        let err: CosmosError = serde_json::from_str::<serde_json::Value>("{oops")
            .unwrap_err()
            .into();
        assert!(!err.is_retryable());
        assert_eq!(err.provider(), SYSTEM);
    }
}
