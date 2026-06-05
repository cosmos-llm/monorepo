use std::collections::HashMap;
use std::env;

/// Per-provider settings: API key, default model, and arbitrary extra fields.
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    /// API key for this provider.
    pub api_key: Option<String>,
    /// Default model name to use when none is specified in the request.
    pub model: Option<String>,
    /// Additional provider-specific settings.
    pub extra: HashMap<String, String>,
}

/// Global library configuration.
///
/// Holds per-provider settings and a default provider name. Configuration can
/// be built programmatically or loaded from environment variables automatically.
///
/// # Environment Variables
///
/// The library reads `CLLM__<PROVIDER>__<SETTING>` variables on construction:
///
/// ```text
/// CLLM__OPENAI__API_KEY=sk-...
/// CLLM__ANTHROPIC__API_KEY=sk-ant-...
/// CLLM__OPENAI__MODEL=gpt-4o
/// ```
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::Config;
///
/// let mut config = Config::new();
/// config.set_api_key("openai", "sk-...");
/// config.set_default_provider("openai");
/// ```
#[derive(Debug, Clone)]
pub struct Config {
    pub(crate) default_provider: String,
    providers: HashMap<String, ProviderConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl Config {
    /// Creates a new [`Config`] and loads settings from environment variables.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Config;
    /// let config = Config::new();
    /// ```
    pub fn new() -> Self {
        let mut cfg = Self {
            default_provider: "openai".into(),
            providers: HashMap::new(),
        };
        cfg.load_from_env();
        cfg
    }

    /// Sets the default provider name.
    ///
    /// # Arguments
    ///
    /// * `provider` — lowercase provider name, e.g. `"anthropic"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Config;
    /// let mut config = Config::new();
    /// config.set_default_provider("anthropic");
    /// assert_eq!(config.default_provider(), "anthropic");
    /// ```
    pub fn set_default_provider(&mut self, provider: impl Into<String>) {
        self.default_provider = provider.into();
    }

    /// Returns the default provider name.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Config;
    /// let config = Config::new();
    /// assert_eq!(config.default_provider(), "openai");
    /// ```
    pub fn default_provider(&self) -> &str {
        &self.default_provider
    }

    /// Sets the API key for a provider.
    ///
    /// # Arguments
    ///
    /// * `provider` — lowercase provider name.
    /// * `key` — API key string.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Config;
    /// let mut config = Config::new();
    /// config.set_api_key("openai", "sk-test");
    /// assert_eq!(config.api_key("openai"), Some("sk-test"));
    /// ```
    pub fn set_api_key(&mut self, provider: impl Into<String>, key: impl Into<String>) {
        self.provider_mut(provider).api_key = Some(key.into());
    }

    /// Returns the API key for a provider, if configured.
    ///
    /// # Arguments
    ///
    /// * `provider` — lowercase provider name.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Config;
    /// let config = Config::new();
    /// // Returns None when no key is configured for an unknown provider.
    /// assert_eq!(config.api_key("unknown"), None);
    /// ```
    pub fn api_key(&self, provider: &str) -> Option<&str> {
        self.providers.get(provider)?.api_key.as_deref()
    }

    /// Sets the default model for a provider.
    ///
    /// # Arguments
    ///
    /// * `provider` — lowercase provider name.
    /// * `model` — model identifier string.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Config;
    /// let mut config = Config::new();
    /// config.set_model("openai", "gpt-4o");
    /// assert_eq!(config.model("openai"), Some("gpt-4o"));
    /// ```
    pub fn set_model(&mut self, provider: impl Into<String>, model: impl Into<String>) {
        self.provider_mut(provider).model = Some(model.into());
    }

    /// Returns the default model for a provider, if configured.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Config;
    /// let config = Config::new();
    /// assert_eq!(config.model("unknown"), None);
    /// ```
    pub fn model(&self, provider: &str) -> Option<&str> {
        self.providers.get(provider)?.model.as_deref()
    }

    /// Returns a reference to the full [`ProviderConfig`] for a provider.
    pub fn provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }

    fn provider_mut(&mut self, name: impl Into<String>) -> &mut ProviderConfig {
        self.providers.entry(name.into()).or_default()
    }

    fn load_from_env(&mut self) {
        for (key, value) in env::vars() {
            if !key.starts_with("CLLM__") {
                continue;
            }
            let parts: Vec<&str> = key.splitn(3, "__").collect();
            if parts.len() < 3 {
                continue;
            }
            let provider = parts[1].to_lowercase();
            let setting = parts[2].to_lowercase();
            let pc = self.provider_mut(provider);
            match setting.as_str() {
                "api_key" => pc.api_key = Some(value),
                "model" => pc.model = Some(value),
                _ => {
                    pc.extra.insert(setting, value);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_is_openai() {
        let config = Config::new();
        assert_eq!(config.default_provider(), "openai");
    }

    #[test]
    fn set_and_get_api_key() {
        let mut config = Config::new();
        config.set_api_key("anthropic", "sk-ant-test");
        assert_eq!(config.api_key("anthropic"), Some("sk-ant-test"));
    }

    #[test]
    fn set_and_get_model() {
        let mut config = Config::new();
        config.set_model("openai", "gpt-4o");
        assert_eq!(config.model("openai"), Some("gpt-4o"));
    }

    #[test]
    fn unknown_provider_returns_none() {
        let config = Config::new();
        assert_eq!(config.api_key("unknown"), None);
        assert_eq!(config.model("unknown"), None);
    }

    #[test]
    fn set_default_provider() {
        let mut config = Config::new();
        config.set_default_provider("anthropic");
        assert_eq!(config.default_provider(), "anthropic");
    }
}
