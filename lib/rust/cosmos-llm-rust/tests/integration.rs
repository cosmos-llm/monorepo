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
    // Streaming is not implemented for any provider yet.
    assert!(!p.unwrap().supports_streaming());
}

#[test]
fn resolve_anthropic() {
    let p = cosmos_llm::providers::resolve("anthropic", Some("key"));
    assert!(p.is_ok());
}

#[test]
fn resolve_openrouter() {
    let p = cosmos_llm::providers::resolve("openrouter", Some("sk-or-test"));
    assert!(p.is_ok());
    assert!(!p.unwrap().supports_streaming());
}

#[test]
fn resolve_is_case_insensitive() {
    assert!(cosmos_llm::providers::resolve("OpenRouter", Some("sk-or-test")).is_ok());
}

#[test]
fn resolve_unknown() {
    let p = cosmos_llm::providers::resolve("grok", None);
    assert!(matches!(p, Err(CosmosError::UnsupportedProvider(_))));
}

// ── opencode auth interop ─────────────────────────────────────────────────────

/// Writes an `auth.json` under a unique temp dir and returns its path.
fn write_auth_json(name: &str, contents: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cosmos-llm-test-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("auth.json");
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn config_loads_keys_from_opencode_auth_file() {
    let path = write_auth_json(
        "load",
        r#"{
            "openrouter": { "type": "api", "key": "sk-or-from-opencode" },
            "anthropic":  { "type": "oauth", "access": "tok" }
        }"#,
    );

    let mut config = Config::new();
    let loaded = config.load_opencode_auth_from(&path).unwrap();

    assert_eq!(loaded, vec!["openrouter"]);
    assert_eq!(config.api_key("openrouter"), Some("sk-or-from-opencode"));
    // OAuth credentials are not reusable, so nothing is stored for anthropic.
    assert_eq!(config.api_key("anthropic"), None);

    let client = Client::from_config(config, "openrouter");
    assert!(client.is_ok());

    std::fs::remove_file(&path).ok();
}

#[test]
fn opencode_auth_does_not_override_explicit_keys() {
    let path = write_auth_json(
        "no-override",
        r#"{ "openrouter": { "type": "api", "key": "sk-or-from-opencode" } }"#,
    );

    let mut config = Config::new();
    config.set_api_key("openrouter", "sk-or-explicit");
    let loaded = config.load_opencode_auth_from(&path).unwrap();

    assert!(loaded.is_empty());
    assert_eq!(config.api_key("openrouter"), Some("sk-or-explicit"));

    std::fs::remove_file(&path).ok();
}

#[test]
fn opencode_auth_missing_explicit_path_errors() {
    let mut config = Config::new();
    let err = config
        .load_opencode_auth_from("/nonexistent/opencode/auth.json")
        .unwrap_err();
    assert!(matches!(err, CosmosError::Configuration(_)));
}

#[test]
fn opencode_auth_invalid_json_errors() {
    let path = write_auth_json("invalid", "{ this is not json");

    let mut config = Config::new();
    let err = config.load_opencode_auth_from(&path).unwrap_err();
    assert!(matches!(err, CosmosError::Json(_)));

    std::fs::remove_file(&path).ok();
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
