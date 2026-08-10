use retry_policy::RetryPolicy;

use crate::config::Config;
use crate::error::CosmosError;
use crate::providers::{resolve, resolve_with_base_url, CompletionStream, Provider};
use crate::types::{CompletionRequest, CompletionResponse, Message, StreamAccumulator};

/// High-level client for interacting with LLM providers.
///
/// `Client` delegates to the configured [`Provider`] and exposes both a
/// simple one-shot completion helper ([`Client::complete`]) and full control
/// via [`Client::completion`].
///
/// The provider and model can be changed at any time using the fluent builder
/// methods [`Client::with_provider`] and [`Client::with_model`].
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::{Client, Config};
///
/// # tokio_test::block_on(async {
/// let mut config = Config::new();
/// config.set_api_key("openai", "sk-...");
///
/// let client = Client::from_config(config, "openai").unwrap();
/// let text = client.complete("What is 2 + 2?").await.unwrap();
/// println!("{text}");
/// # })
/// ```
pub struct Client {
    provider: Box<dyn Provider>,
    default_model: Option<String>,
    /// Retry schedule, or `None` to make exactly one attempt.
    ///
    /// Off unless a caller opts in with [`Client::with_retry`]. See that method
    /// for why the default is off.
    retry: Option<RetryPolicy>,
}

impl Client {
    /// Creates a [`Client`] for the named provider using a [`Config`].
    ///
    /// The API key is read from `config` for the given provider name.
    ///
    /// # Arguments
    ///
    /// * `config` — library configuration.
    /// * `provider_name` — lowercase provider name, e.g. `"openai"`.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::UnsupportedProvider`] when the name is not
    /// recognised.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::{Client, Config};
    ///
    /// let mut config = Config::new();
    /// config.set_api_key("openai", "sk-test");
    /// let client = Client::from_config(config, "openai").unwrap();
    /// ```
    pub fn from_config(config: Config, provider_name: &str) -> Result<Self, CosmosError> {
        let key = config.api_key(provider_name).map(str::to_owned);
        let model = config.model(provider_name).map(str::to_owned);
        let provider = resolve(provider_name, key.as_deref())?;
        Ok(Self {
            provider,
            default_model: model,
            retry: None,
        })
    }

    /// Creates a [`Client`] for the named provider with an explicit API key.
    ///
    /// # Arguments
    ///
    /// * `provider_name` — lowercase provider name.
    /// * `api_key` — API key string.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::UnsupportedProvider`] when the name is not
    /// recognised.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::Client;
    /// let client = Client::new("openai", "sk-test").unwrap();
    /// ```
    pub fn new(provider_name: &str, api_key: impl Into<String>) -> Result<Self, CosmosError> {
        let key = api_key.into();
        let provider = resolve(provider_name, Some(&key))?;
        Ok(Self {
            provider,
            default_model: None,
            retry: None,
        })
    }

    /// Wraps an already-constructed [`Provider`] in a [`Client`].
    ///
    /// This is the entry point for a provider that [`resolve`] cannot name: a
    /// [`MockProvider`](crate::providers::mock::MockProvider) in a test, or an
    /// implementation living outside this crate. Infallible, since there is no
    /// name to look up and no key to read.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "mock")] {
    /// use cosmos_llm::providers::mock::MockProvider;
    /// use cosmos_llm::Client;
    ///
    /// # tokio_test::block_on(async {
    /// let client = Client::from_provider(Box::new(MockProvider::new().respond("hi")))
    ///     .with_model("mock-model");
    /// assert_eq!(client.complete("anything").await.unwrap(), "hi");
    /// # })
    /// # }
    /// ```
    pub fn from_provider(provider: Box<dyn Provider>) -> Self {
        Self {
            provider,
            default_model: None,
            retry: None,
        }
    }

    /// Creates a [`Client`] pointed at a non-default API root.
    ///
    /// Use this to talk to an OpenAI-compatible server (Azure OpenAI, a local
    /// llama.cpp or vLLM instance), route through a proxy, or point tests at a
    /// mock server.
    ///
    /// # Arguments
    ///
    /// * `provider_name` — lowercase provider name; selects the wire format.
    /// * `api_key` — API key string.
    /// * `base_url` — API root, replacing the provider's default.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::UnsupportedProvider`] when the name is not
    /// recognised.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::Client;
    ///
    /// // A local vLLM server speaking the OpenAI protocol.
    /// let client = Client::new_with_base_url(
    ///     "openai",
    ///     "not-checked-locally",
    ///     "http://localhost:8000/v1",
    /// )
    /// .unwrap()
    /// .with_model("meta-llama/Llama-3-8b");
    /// ```
    pub fn new_with_base_url(
        provider_name: &str,
        api_key: impl Into<String>,
        base_url: impl AsRef<str>,
    ) -> Result<Self, CosmosError> {
        let key = api_key.into();
        let provider = resolve_with_base_url(provider_name, Some(&key), Some(base_url.as_ref()))?;
        Ok(Self {
            provider,
            default_model: None,
            retry: None,
        })
    }

    /// Sets the default model for all subsequent requests (builder pattern).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::Client;
    /// let client = Client::new("openai", "sk-test")
    ///     .unwrap()
    ///     .with_model("gpt-4o");
    /// ```
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = Some(model.into());
        self
    }

    /// Retries failed requests according to `policy` (builder pattern).
    ///
    /// **Off by default, and deliberately so.** A library that silently retries
    /// surprises a caller who is already retrying itself, turning three attempts
    /// into nine. It also spends money: every retry against a metered API is
    /// another charge on someone's account. Opting in is the caller saying they
    /// have accounted for both.
    ///
    /// Only errors that [`CosmosError::is_retryable`] accepts are retried, and a
    /// provider's `Retry-After` is honored when it sent one. Applies to
    /// [`Client::completion`] and [`Client::complete`]. It does **not** apply to
    /// the streaming methods: re-running a stream that already yielded chunks
    /// would replay output the caller has seen, and deciding what to do about
    /// that belongs to the caller.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use cosmos_llm::Client;
    /// use retry_policy::RetryPolicy;
    ///
    /// let client = Client::new("openai", "sk-test")
    ///     .unwrap()
    ///     .with_model("gpt-4o")
    ///     .with_retry(
    ///         RetryPolicy::new()
    ///             .max_attempts(4)
    ///             .initial_backoff(Duration::from_millis(500)),
    ///     );
    /// ```
    pub fn with_retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = Some(policy);
        self
    }

    /// Stops retrying failed requests, returning to the default.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::Client;
    /// use retry_policy::RetryPolicy;
    ///
    /// let client = Client::new("openai", "sk-test")
    ///     .unwrap()
    ///     .with_retry(RetryPolicy::new())
    ///     .without_retry();
    /// assert!(client.retry_policy().is_none());
    /// ```
    pub fn without_retry(mut self) -> Self {
        self.retry = None;
        self
    }

    /// Returns the configured retry policy, if any.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::Client;
    ///
    /// let client = Client::new("openai", "sk-test").unwrap();
    /// assert!(client.retry_policy().is_none());
    /// ```
    pub fn retry_policy(&self) -> Option<&RetryPolicy> {
        self.retry.as_ref()
    }

    /// Switches to a different provider, preserving the default model.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::UnsupportedProvider`] when the name is not
    /// recognised.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::Client;
    /// let client = Client::new("openai", "sk-test").unwrap();
    /// let client = client.with_provider("anthropic", Some("sk-ant-test")).unwrap();
    /// ```
    pub fn with_provider(mut self, name: &str, api_key: Option<&str>) -> Result<Self, CosmosError> {
        self.provider = resolve(name, api_key)?;
        Ok(self)
    }

    /// Switches to a different provider at a non-default API root, preserving
    /// the default model.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::UnsupportedProvider`] when the name is not
    /// recognised.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::Client;
    ///
    /// let client = Client::new("openai", "sk-test").unwrap();
    /// let client = client
    ///     .with_provider_at("openai", Some("local"), "http://localhost:8000/v1")
    ///     .unwrap();
    /// ```
    pub fn with_provider_at(
        mut self,
        name: &str,
        api_key: Option<&str>,
        base_url: impl AsRef<str>,
    ) -> Result<Self, CosmosError> {
        self.provider = resolve_with_base_url(name, api_key, Some(base_url.as_ref()))?;
        Ok(self)
    }

    /// Returns `true` if the current provider implements streaming.
    ///
    /// See [`Provider::supports_streaming`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::Client;
    /// let client = Client::new("openai", "sk-test").unwrap();
    /// assert!(client.can_stream());
    /// ```
    pub fn can_stream(&self) -> bool {
        self.provider.supports_streaming()
    }

    /// Sends a plain text prompt and returns the generated text.
    ///
    /// This is a convenience wrapper around [`Client::completion`]. The
    /// prompt is sent as a single `user` message. The default model must be
    /// set (via [`Client::with_model`] or the config) before calling.
    ///
    /// # Arguments
    ///
    /// * `prompt` — user prompt text.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::Configuration`] when no default model is set.
    /// Returns any provider error on failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::Client;
    ///
    /// # tokio_test::block_on(async {
    /// let client = Client::new("openai", "sk-test").unwrap()
    ///     .with_model("gpt-4o");
    /// let text = client.complete("What is the capital of France?").await.unwrap();
    /// println!("{text}");
    /// # })
    /// ```
    pub async fn complete(&self, prompt: impl Into<String>) -> Result<String, CosmosError> {
        let model = self
            .default_model
            .as_deref()
            .ok_or_else(|| CosmosError::Configuration {
                provider: self.provider.name().to_owned(),
                message: "no default model set; call .with_model() or set it in Config".to_owned(),
            })?;

        // Through `completion` rather than straight to the provider, so a
        // configured retry policy applies here too.
        let req = CompletionRequest::new(model, vec![Message::user(prompt)]);
        let resp = self.completion(req).await?;

        resp.content()
            .map(str::to_owned)
            .ok_or_else(|| CosmosError::InvalidResponse {
                provider: self.provider.name().to_owned(),
                message: "provider returned no content".to_owned(),
            })
    }

    /// Sends a full [`CompletionRequest`] and returns the provider response.
    ///
    /// When the request's `model` field is empty and a default model is
    /// configured, the default is injected automatically.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::Configuration`] when no model is available.
    /// Returns any provider error on failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::{Client, CompletionRequest, Message};
    ///
    /// # tokio_test::block_on(async {
    /// let client = Client::new("openai", "sk-test").unwrap();
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("Hi")])
    ///     .with_temperature(0.5);
    /// let resp = client.completion(req).await.unwrap();
    /// println!("{}", resp.content().unwrap_or(""));
    /// # })
    /// ```
    pub async fn completion(
        &self,
        mut req: CompletionRequest,
    ) -> Result<CompletionResponse, CosmosError> {
        self.fill_default_model(&mut req)?;

        let Some(policy) = self.retry.as_ref() else {
            return self.provider.completion(&req).await;
        };

        // The attempt history is discarded here because `completion` returns
        // `CosmosError`, not a retry error. Callers who need the history should
        // drive `RetryPolicy` themselves; folding a second error type into this
        // signature would be a breaking change for every existing caller.
        policy
            .run(|| self.provider.completion(&req))
            .await
            .map_err(retry_policy::RetryError::into_last_error)
    }

    /// Sends a chat conversation and returns the provider response.
    ///
    /// Alias for [`Client::completion`] — identical in behaviour.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::{Client, CompletionRequest, Message};
    ///
    /// # tokio_test::block_on(async {
    /// let client = Client::new("openai", "sk-test").unwrap();
    /// let req = CompletionRequest::new(
    ///     "gpt-4o",
    ///     vec![
    ///         Message::system("You are a pirate."),
    ///         Message::user("Where is the treasure?"),
    ///     ],
    /// );
    /// let resp = client.chat(req).await.unwrap();
    /// # })
    /// ```
    pub async fn chat(&self, req: CompletionRequest) -> Result<CompletionResponse, CosmosError> {
        self.completion(req).await
    }

    /// Sends a full [`CompletionRequest`] and returns a stream of chunks.
    ///
    /// As with [`Client::completion`], an empty `model` is filled in from the
    /// configured default. The returned stream is `'static` and `Send`, so it
    /// can be moved into a spawned task.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::Configuration`] when no model is available, or
    /// [`CosmosError::Streaming`] when the provider does not support
    /// streaming. Errors that occur once the stream is open are yielded as
    /// stream items rather than returned here.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::{Client, CompletionRequest, Message};
    /// use futures_util::StreamExt;
    ///
    /// # tokio_test::block_on(async {
    /// let client = Client::new("openai", "sk-test").unwrap();
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("Tell me a story")]);
    ///
    /// let mut stream = client.stream_completion(req).await.unwrap();
    /// while let Some(chunk) = stream.next().await {
    ///     print!("{}", chunk.unwrap().delta);
    /// }
    /// # })
    /// ```
    pub async fn stream_completion(
        &self,
        mut req: CompletionRequest,
    ) -> Result<CompletionStream, CosmosError> {
        self.fill_default_model(&mut req)?;
        self.provider.stream_completion(&req).await
    }

    /// Sends a plain text prompt and returns a stream of chunks.
    ///
    /// The streaming counterpart of [`Client::complete`]: the prompt is sent
    /// as a single `user` message, and the default model must be set.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::Configuration`] when no default model is set,
    /// or any error from [`Client::stream_completion`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::Client;
    /// use futures_util::StreamExt;
    ///
    /// # tokio_test::block_on(async {
    /// let client = Client::new("openai", "sk-test").unwrap().with_model("gpt-4o");
    ///
    /// let mut stream = client.stream("Count to ten").await.unwrap();
    /// while let Some(chunk) = stream.next().await {
    ///     print!("{}", chunk.unwrap().delta);
    /// }
    /// # })
    /// ```
    pub async fn stream(&self, prompt: impl Into<String>) -> Result<CompletionStream, CosmosError> {
        let model = self
            .default_model
            .as_deref()
            .ok_or_else(|| CosmosError::Configuration {
                provider: self.provider.name().to_owned(),
                message: "no default model set; call .with_model() or set it in Config".to_owned(),
            })?;

        self.stream_completion(CompletionRequest::new(model, vec![Message::user(prompt)]))
            .await
    }

    /// Substitutes the client's default model when the request names none.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::Configuration`] when the request has no model and
    /// the client has no default to supply.
    fn fill_default_model(&self, req: &mut CompletionRequest) -> Result<(), CosmosError> {
        if req.model.is_empty() {
            req.model = self
                .default_model
                .clone()
                .ok_or_else(|| CosmosError::Configuration {
                    provider: self.provider.name().to_owned(),
                    message: "no model specified".to_owned(),
                })?;
        }
        Ok(())
    }

    /// Streams a completion, invoking `on_chunk` per chunk, and returns the
    /// assembled response.
    ///
    /// Useful when the caller wants live output *and* the final response —
    /// printing tokens as they arrive while still getting tool calls and usage
    /// at the end. The response is built by a
    /// [`StreamAccumulator`], so it has the same shape a non-streaming call
    /// would have returned.
    ///
    /// # Errors
    ///
    /// Returns any error from [`Client::stream_completion`], or the first
    /// error yielded mid-stream. Text received before an error is discarded.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::{Client, CompletionRequest, Message};
    /// use std::io::Write;
    ///
    /// # tokio_test::block_on(async {
    /// let client = Client::new("openai", "sk-test").unwrap();
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("Hi")]);
    ///
    /// let resp = client.stream_to_completion(req, |chunk| {
    ///     print!("{}", chunk.delta);
    ///     let _ = std::io::stdout().flush();
    /// }).await.unwrap();
    ///
    /// println!("\nfinished: {:?}", resp.choices[0].finish_reason);
    /// # })
    /// ```
    pub async fn stream_to_completion<F>(
        &self,
        req: CompletionRequest,
        mut on_chunk: F,
    ) -> Result<CompletionResponse, CosmosError>
    where
        F: FnMut(&crate::types::StreamChunk),
    {
        use futures_util::StreamExt;

        let mut stream = self.stream_completion(req).await?;
        let mut acc = StreamAccumulator::new();

        while let Some(item) = stream.next().await {
            let chunk = item?;
            on_chunk(&chunk);
            acc.push(&chunk);
        }

        Ok(acc.into_response())
    }

    /// Returns the list of models available from the current provider.
    ///
    /// # Errors
    ///
    /// Returns any provider error on failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cosmos_llm::Client;
    ///
    /// # tokio_test::block_on(async {
    /// let client = Client::new("anthropic", "sk-ant-test").unwrap();
    /// let models = client.models().await.unwrap();
    /// println!("{models:#?}");
    /// # })
    /// ```
    pub async fn models(&self) -> Result<Vec<String>, CosmosError> {
        self.provider.models().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_provider_returns_error() {
        let result = Client::new("unknown-provider", "key");
        assert!(matches!(
            result,
            Err(CosmosError::UnsupportedProvider { .. })
        ));
    }

    #[test]
    fn with_model_sets_default() {
        let client = Client::new("openai", "key").unwrap().with_model("gpt-4o");
        assert_eq!(client.default_model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn can_stream_reports_provider_support() {
        for name in ["openai", "anthropic", "openrouter"] {
            assert!(Client::new(name, "key").unwrap().can_stream(), "{name}");
        }
    }

    #[tokio::test]
    async fn complete_without_model_returns_config_error() {
        let client = Client::new("openai", "key").unwrap();
        let err = client.complete("hello").await.unwrap_err();
        assert!(matches!(err, CosmosError::Configuration { .. }));
    }

    // A `CompletionStream` is not `Debug`, so these assert on the error arm
    // directly rather than via `unwrap_err`.

    #[tokio::test]
    async fn stream_without_model_returns_config_error() {
        let client = Client::new("openai", "key").unwrap();
        assert!(matches!(
            client.stream("hello").await,
            Err(CosmosError::Configuration { .. })
        ));
    }

    #[tokio::test]
    async fn stream_completion_without_model_returns_config_error() {
        let client = Client::new("openai", "key").unwrap();
        let req = CompletionRequest::new("", vec![Message::user("hi")]);
        assert!(matches!(
            client.stream_completion(req).await,
            Err(CosmosError::Configuration { .. })
        ));
    }
}
