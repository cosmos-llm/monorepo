use crate::config::Config;
use crate::error::CosmosError;
use crate::providers::{resolve, Provider};
use crate::types::{CompletionRequest, CompletionResponse, Message};

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

    /// Returns `true` if the current provider supports streaming.
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
        let model = self.default_model.as_deref().ok_or_else(|| {
            CosmosError::Configuration(
                "no default model set; call .with_model() or set it in Config".to_owned(),
            )
        })?;

        let req = CompletionRequest::new(model, vec![Message::user(prompt)]);
        let resp = self.provider.completion(&req).await?;

        resp.content()
            .map(str::to_owned)
            .ok_or_else(|| CosmosError::InvalidResponse("provider returned no content".into()))
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
        if req.model.is_empty() {
            req.model = self
                .default_model
                .clone()
                .ok_or_else(|| CosmosError::Configuration("no model specified".into()))?;
        }
        self.provider.completion(&req).await
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
        assert!(matches!(result, Err(CosmosError::UnsupportedProvider(_))));
    }

    #[test]
    fn with_model_sets_default() {
        let client = Client::new("openai", "key").unwrap().with_model("gpt-4o");
        assert_eq!(client.default_model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn can_stream_openai() {
        let client = Client::new("openai", "key").unwrap();
        assert!(client.can_stream());
    }

    #[tokio::test]
    async fn complete_without_model_returns_config_error() {
        let client = Client::new("openai", "key").unwrap();
        let err = client.complete("hello").await.unwrap_err();
        assert!(matches!(err, CosmosError::Configuration(_)));
    }
}
