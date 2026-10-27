//! The language model a predictor calls, as a swappable trait.
//!
//! [`LanguageModel`] is deliberately small: messages and options in, text
//! and usage out. That is all a signature-driven call needs, and it keeps
//! the door open for backends that are not HTTP APIs at all — a local model,
//! a recorded fixture, another process.
//!
//! With the `client` feature, [`cosmos_llm::Client`] implements it, and the
//! `cosmos` backend registered with [`BackendPlugin`] builds one by provider
//! name.

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use cosmos_llm_adapter::Message;
use cosmos_llm_registry::{Meta, Plugin};

use crate::error::PredictError;

/// Per-call settings, layered over a predictor's defaults.
///
/// Every field is optional so that layering is a plain "set fields win":
/// see [`CallOptions::over`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CallOptions {
    /// The model identifier; `None` defers to the backend's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Extra top-level request fields, such as an adapter's `response_format`.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
    /// Mixed into the cache key and never sent to the model.
    ///
    /// Sampling the same request several times — as best-of-N does — would
    /// otherwise hit the cache on every draw after the first and return the
    /// same reply N times. A distinct salt per draw keeps each one cached on
    /// its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_salt: Option<String>,
}

impl CallOptions {
    /// These options layered over `base`: fields set here win, and `extra`
    /// maps are merged key by key.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm_predict::CallOptions;
    ///
    /// let base = CallOptions { model: Some("m".into()), temperature: Some(0.0), ..Default::default() };
    /// let call = CallOptions { temperature: Some(1.0), ..Default::default() };
    /// let merged = call.over(&base);
    /// assert_eq!((merged.model.as_deref(), merged.temperature), (Some("m"), Some(1.0)));
    /// ```
    #[must_use]
    pub fn over(&self, base: &CallOptions) -> CallOptions {
        let mut extra = base.extra.clone();
        for (key, value) in &self.extra {
            extra.insert(key.clone(), value.clone());
        }
        CallOptions {
            model: self.model.clone().or_else(|| base.model.clone()),
            temperature: self.temperature.or(base.temperature),
            max_tokens: self.max_tokens.or(base.max_tokens),
            extra,
            cache_salt: self.cache_salt.clone().or_else(|| base.cache_salt.clone()),
        }
    }
}

/// Token counts and cost for one call.
///
/// A crate-local type rather than the client's, so that it serializes into a
/// cache entry and so that a backend without the client can report it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens consumed by the prompt.
    #[serde(default)]
    pub prompt_tokens: u32,
    /// Tokens generated.
    #[serde(default)]
    pub completion_tokens: u32,
    /// What the call was charged, in US dollars, when the backend reports it.
    /// `None` means unknown, not free.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

impl Usage {
    /// The sum of two usages. A cost is summed only when both are known.
    #[must_use]
    pub fn plus(&self, other: &Usage) -> Usage {
        Usage {
            prompt_tokens: self.prompt_tokens + other.prompt_tokens,
            completion_tokens: self.completion_tokens + other.completion_tokens,
            cost: self.cost.zip(other.cost).map(|(a, b)| a + b),
        }
    }
}

/// One model reply.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Completion {
    /// The reply text.
    pub text: String,
    /// What the call cost, if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// Anything that turns messages into a reply.
pub trait LanguageModel: Send + Sync {
    /// Sends `messages` and returns the reply.
    ///
    /// # Errors
    ///
    /// Returns [`PredictError::Model`] for any backend failure.
    fn complete<'a>(
        &'a self,
        messages: &'a [Message],
        options: &'a CallOptions,
    ) -> BoxFuture<'a, Result<Completion, PredictError>>;

    /// Identifies the backend in cache keys, e.g. `"anthropic"`.
    ///
    /// Two models with the same identity and the same request must be
    /// interchangeable, since a cached reply from one is served for the
    /// other. The model name is already part of the key via
    /// [`CallOptions::model`]; this distinguishes backends.
    fn identity(&self) -> String;
}

/// Settings for building a backend by name.
#[derive(Debug, Clone, Default)]
pub struct BackendOptions {
    /// The provider, for backends that front several, e.g. `"anthropic"`.
    pub provider: Option<String>,
    /// The default model.
    pub model: Option<String>,
    /// An API key; backends should fall back to their usual environment
    /// variables when it is absent.
    pub api_key: Option<String>,
    /// An alternative API root, for proxies and compatible servers.
    pub base_url: Option<String>,
}

/// A language model backend registration.
///
/// ```
/// use cosmos_llm_predict::{BackendOptions, BackendPlugin, CallOptions, Completion, LanguageModel, PredictError};
/// use cosmos_llm_predict::registry::meta;
/// use cosmos_llm_adapter::Message;
/// use futures_util::future::BoxFuture;
///
/// /// Replies with the last message, for wiring tests.
/// struct Echo;
///
/// impl LanguageModel for Echo {
///     fn complete<'a>(&'a self, messages: &'a [Message], _: &'a CallOptions)
///         -> BoxFuture<'a, Result<Completion, PredictError>>
///     {
///         let text = messages.last().map(|m| m.content.clone()).unwrap_or_default();
///         Box::pin(async move { Ok(Completion { text, usage: None }) })
///     }
///
///     fn identity(&self) -> String {
///         "echo".to_owned()
///     }
/// }
///
/// fn build(_: &BackendOptions) -> Result<Box<dyn LanguageModel>, PredictError> {
///     Ok(Box::new(Echo))
/// }
///
/// cosmos_llm_predict::registry::inventory::submit! {
///     BackendPlugin { meta: meta!("echo").summary("Repeats the prompt"), build }
/// }
///
/// assert!(cosmos_llm_predict::backend("echo", &BackendOptions::default()).is_ok());
/// ```
pub struct BackendPlugin {
    /// The backend's name and aliases.
    pub meta: Meta,
    /// Builds the backend.
    pub build: fn(&BackendOptions) -> Result<Box<dyn LanguageModel>, PredictError>,
}

impl Plugin for BackendPlugin {
    fn meta(&self) -> &Meta {
        &self.meta
    }
}

cosmos_llm_registry::inventory::collect!(BackendPlugin);

/// Builds the backend registered under `name`.
///
/// # Errors
///
/// Returns [`PredictError::NotFound`] for an unregistered name, and whatever
/// the backend's builder returns.
pub fn backend(
    name: &str,
    options: &BackendOptions,
) -> Result<Box<dyn LanguageModel>, PredictError> {
    let plugin = cosmos_llm_registry::require::<BackendPlugin>("backend", name)?;
    (plugin.build)(options)
}

#[cfg(feature = "client")]
mod client {
    use super::*;
    use cosmos_llm::{Client, CompletionRequest, Config};
    use cosmos_llm_registry::meta;

    impl LanguageModel for Client {
        fn complete<'a>(
            &'a self,
            messages: &'a [Message],
            options: &'a CallOptions,
        ) -> BoxFuture<'a, Result<Completion, PredictError>> {
            Box::pin(async move {
                let messages = messages
                    .iter()
                    .map(|m| {
                        let role = cosmos_llm::Role::parse(&m.role).ok_or_else(|| {
                            PredictError::Config(format!("unknown message role: {}", m.role))
                        })?;
                        Ok(cosmos_llm::Message::new(role, m.content.clone()))
                    })
                    .collect::<Result<Vec<_>, PredictError>>()?;
                // An empty model name tells the client to use its default.
                let mut request =
                    CompletionRequest::new(options.model.clone().unwrap_or_default(), messages);
                request.temperature = options.temperature;
                request.max_tokens = options.max_tokens;
                for (key, value) in &options.extra {
                    request.extra.insert(key.clone(), value.clone());
                }
                let response = self.completion(request).await?;
                Ok(Completion {
                    text: response.text(),
                    usage: response.usage.as_ref().map(|u| Usage {
                        prompt_tokens: u.prompt_tokens,
                        completion_tokens: u.completion_tokens,
                        cost: u.cost,
                    }),
                })
            })
        }

        fn identity(&self) -> String {
            self.provider_name().to_owned()
        }
    }

    fn build_client(options: &BackendOptions) -> Result<Box<dyn LanguageModel>, PredictError> {
        let mut config = Config::new();
        let provider = options
            .provider
            .clone()
            .unwrap_or_else(|| config.default_provider().to_owned());
        if let Some(key) = &options.api_key {
            config.set_api_key(provider.clone(), key.clone());
        }
        if let Some(model) = &options.model {
            config.set_model(provider.clone(), model.clone());
        }
        let client = match &options.base_url {
            None => Client::from_config(config, &provider)?,
            Some(url) => {
                let provider_impl = cosmos_llm::resolve_with_base_url(
                    &provider,
                    config.api_key(&provider),
                    Some(url),
                )?;
                let client = Client::from_provider(provider_impl);
                match config.model(&provider) {
                    Some(model) => client.with_model(model),
                    None => client,
                }
            }
        };
        Ok(Box::new(client))
    }

    cosmos_llm_registry::inventory::submit! {
        BackendPlugin {
            meta: meta!("cosmos")
                .builtin()
                .aliases(&["cosmos_llm", "client"])
                .summary("cosmos-llm Client; --provider picks openai, anthropic, openrouter, ..."),
            build: build_client,
        }
    }
}
