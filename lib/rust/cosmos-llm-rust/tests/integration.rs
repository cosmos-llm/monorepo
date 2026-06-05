use cosmos_llm::{Client, CompletionRequest, Config, CosmosError, Message};

// ── Config ────────────────────────────────────────────────────────────────────

#[test]
fn config_env_roundtrip() {
    let mut config = Config::new();
    config.set_api_key("openai", "sk-test");
    config.set_model("openai", "gpt-4o");
    config.set_default_provider("openai");
    assert_eq!(config.api_key("openai"), Some("sk-test"));
    assert_eq!(config.model("openai"), Some("gpt-4o"));
    assert_eq!(config.default_provider(), "openai");
}

// ── Client construction ───────────────────────────────────────────────────────

#[test]
fn client_from_config_openai() {
    let mut config = Config::new();
    config.set_api_key("openai", "sk-test");
    let client = Client::from_config(config, "openai");
    assert!(client.is_ok());
}

#[test]
fn client_from_config_anthropic() {
    let mut config = Config::new();
    config.set_api_key("anthropic", "sk-ant-test");
    let client = Client::from_config(config, "anthropic");
    assert!(client.is_ok());
}

#[test]
fn client_unsupported_provider() {
    let result = Client::new("llama-local", "key");
    assert!(matches!(result, Err(CosmosError::UnsupportedProvider(_))));
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[test]
fn message_roles() {
    assert_eq!(Message::system("s").role, "system");
    assert_eq!(Message::user("u").role, "user");
    assert_eq!(Message::assistant("a").role, "assistant");
}

#[test]
fn completion_request_builder_chain() {
    let req = CompletionRequest::new("m", vec![Message::user("hi")])
        .with_temperature(0.3)
        .with_max_tokens(512)
        .with_top_p(0.95)
        .with_stop(vec!["STOP".into()]);

    assert_eq!(req.temperature, Some(0.3));
    assert_eq!(req.max_tokens, Some(512));
    assert_eq!(req.top_p, Some(0.95));
    assert_eq!(req.stop.as_deref(), Some(["STOP".to_owned()].as_slice()));
}

// ── Error variants ────────────────────────────────────────────────────────────

#[test]
fn error_display() {
    let e = CosmosError::Authentication("bad key".into());
    assert!(e.to_string().contains("bad key"));

    let e = CosmosError::RateLimit("slow down".into());
    assert!(e.to_string().contains("slow down"));

    let e = CosmosError::UnsupportedProvider("groq".into());
    assert!(e.to_string().contains("groq"));
}

// ── Provider resolution ───────────────────────────────────────────────────────

#[test]
fn resolve_openai() {
    let p = cosmos_llm::providers::resolve("openai", Some("key"));
    assert!(p.is_ok());
    assert!(p.unwrap().supports_streaming());
}

#[test]
fn resolve_anthropic() {
    let p = cosmos_llm::providers::resolve("anthropic", Some("key"));
    assert!(p.is_ok());
}

#[test]
fn resolve_unknown() {
    let p = cosmos_llm::providers::resolve("grok", None);
    assert!(matches!(p, Err(CosmosError::UnsupportedProvider(_))));
}

// ── Async: no model error ─────────────────────────────────────────────────────

#[tokio::test]
async fn complete_without_model_returns_config_error() {
    let client = Client::new("openai", "sk-test").unwrap();
    let err = client.complete("hello").await.unwrap_err();
    assert!(matches!(err, CosmosError::Configuration(_)));
}

// ── Async: Anthropic model list ───────────────────────────────────────────────

#[tokio::test]
async fn anthropic_models_static_list() {
    use cosmos_llm::providers::anthropic::AnthropicProvider;
    use cosmos_llm::providers::Provider;

    let p = AnthropicProvider::new(Some("sk-ant-test".into()));
    let models = p.models().await.unwrap();
    assert!(!models.is_empty());
    assert!(models.iter().any(|m| m.contains("claude")));
}
