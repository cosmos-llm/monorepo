use thiserror::Error;

/// All errors that can be produced by this library.
#[derive(Debug, Error)]
pub enum CosmosError {
    /// The API key is missing or was rejected by the provider.
    #[error("authentication error: {0}")]
    Authentication(String),

    /// The provider returned a 429 Too Many Requests response.
    #[error("rate limit exceeded: {0}")]
    RateLimit(String),

    /// The request was malformed or contained invalid parameters.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// The requested resource (e.g. model) was not found.
    #[error("resource not found: {0}")]
    NotFound(String),

    /// The provider's server returned a 5xx error.
    #[error("server error: {0}")]
    Server(String),

    /// The provider name is not recognised.
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),

    /// A configuration value is missing or invalid.
    #[error("configuration error: {0}")]
    Configuration(String),

    /// The account has insufficient quota or credits.
    #[error("insufficient quota: {0}")]
    InsufficientQuota(String),

    /// The response from the provider could not be parsed.
    #[error("invalid response: {0}")]
    InvalidResponse(String),

    /// An HTTP transport error occurred.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// A JSON serialisation or deserialisation error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// An error occurred during a streaming response.
    #[error("streaming error: {0}")]
    Streaming(String),
}
