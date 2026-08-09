use std::future::Future;
use std::pin::Pin;

use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::error::CosmosError;
use crate::providers::openai::parse_openai_chunk;
use crate::providers::{stream_from_response, CompletionStream, Provider};
use crate::types::{Choice, CompletionRequest, CompletionResponse, Message, ToolCall, Usage};

/// Default API root, used unless overridden by
/// [`OpenRouterProvider::with_base_url`] or the `OPENROUTER_BASE_URL`
/// environment variable.
pub const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Provider implementation for the OpenRouter API.
///
/// OpenRouter proxies many upstream models behind an OpenAI-compatible
/// interface, so request and response shapes match
/// [`OpenAiProvider`](crate::providers::openai::OpenAiProvider). Model
/// identifiers are namespaced by vendor, e.g. `"anthropic/claude-3.5-sonnet"`
/// or `"openai/gpt-4o"`.
///
/// Reads the API key from `OPENROUTER_API_KEY` or
/// `CLLM__OPENROUTER__API_KEY` when none is supplied at construction.
///
/// # Attribution headers
///
/// OpenRouter uses the optional `HTTP-Referer` and `X-Title` headers to
/// attribute traffic on public leaderboards. Set them with
/// [`OpenRouterProvider::with_referer`] and [`OpenRouterProvider::with_title`],
/// or via the `OPENROUTER_REFERER` / `OPENROUTER_TITLE` environment
/// variables. Both are optional and omitted when unset.
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::providers::openrouter::OpenRouterProvider;
/// use cosmos_llm::providers::Provider;
/// use cosmos_llm::types::{CompletionRequest, Message};
///
/// # tokio_test::block_on(async {
/// let provider = OpenRouterProvider::new(Some("sk-or-test".into()))
///     .with_title("my-app");
/// let req = CompletionRequest::new(
///     "anthropic/claude-3.5-sonnet",
///     vec![Message::user("Hello")],
/// );
/// // let resp = provider.completion(&req).await.unwrap();
/// # })
/// ```
pub struct OpenRouterProvider {
    api_key: Option<String>,
    base_url: String,
    referer: Option<String>,
    title: Option<String>,
    http: HttpClient,
}

impl OpenRouterProvider {
    /// Creates a new [`OpenRouterProvider`].
    ///
    /// When `api_key` is `None`, the provider falls back to the
    /// `OPENROUTER_API_KEY` or `CLLM__OPENROUTER__API_KEY` environment
    /// variables. Attribution headers default to `OPENROUTER_REFERER` and
    /// `OPENROUTER_TITLE` when those are set. The API root defaults to
    /// [`DEFAULT_BASE_URL`] unless `OPENROUTER_BASE_URL` or
    /// `CLLM__OPENROUTER__BASE_URL` is set.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::openrouter::OpenRouterProvider;
    /// let provider = OpenRouterProvider::new(None);
    /// ```
    pub fn new(api_key: Option<String>) -> Self {
        let key = api_key
            .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
            .or_else(|| std::env::var("CLLM__OPENROUTER__API_KEY").ok());
        Self {
            api_key: key,
            base_url: std::env::var("OPENROUTER_BASE_URL")
                .or_else(|_| std::env::var("CLLM__OPENROUTER__BASE_URL"))
                .unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned()),
            referer: std::env::var("OPENROUTER_REFERER").ok(),
            title: std::env::var("OPENROUTER_TITLE").ok(),
            http: HttpClient::new(),
        }
    }

    /// Points the provider at a different API root (builder pattern).
    ///
    /// Useful for OpenRouter-compatible proxies and for pointing tests at a
    /// mock server. Any trailing slash is trimmed.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::openrouter::OpenRouterProvider;
    ///
    /// let provider = OpenRouterProvider::new(Some("sk-or-test".into()))
    ///     .with_base_url("http://localhost:8080/api/v1");
    /// assert_eq!(provider.base_url(), "http://localhost:8080/api/v1");
    /// ```
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let url = base_url.into();
        self.base_url = url.trim_end_matches('/').to_owned();
        self
    }

    /// Returns the API root this provider sends requests to.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::openrouter::{OpenRouterProvider, DEFAULT_BASE_URL};
    ///
    /// # std::env::remove_var("OPENROUTER_BASE_URL");
    /// # std::env::remove_var("CLLM__OPENROUTER__BASE_URL");
    /// let provider = OpenRouterProvider::new(Some("sk-or-test".into()));
    /// assert_eq!(provider.base_url(), DEFAULT_BASE_URL);
    /// ```
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Sets the `HTTP-Referer` attribution header (builder pattern).
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::openrouter::OpenRouterProvider;
    /// let provider = OpenRouterProvider::new(Some("sk-or-test".into()))
    ///     .with_referer("https://example.com");
    /// ```
    pub fn with_referer(mut self, referer: impl Into<String>) -> Self {
        self.referer = Some(referer.into());
        self
    }

    /// Sets the `X-Title` attribution header (builder pattern).
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::openrouter::OpenRouterProvider;
    /// let provider = OpenRouterProvider::new(Some("sk-or-test".into()))
    ///     .with_title("my-app");
    /// ```
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    fn resolved_key(&self) -> Result<&str, CosmosError> {
        self.api_key.as_deref().ok_or_else(|| {
            CosmosError::Authentication(
                "OpenRouter API key not set. Export OPENROUTER_API_KEY or \
                 CLLM__OPENROUTER__API_KEY."
                    .to_owned(),
            )
        })
    }

    /// Applies the bearer token and optional attribution headers to a request.
    fn authorize(&self, mut req: reqwest::RequestBuilder, key: &str) -> reqwest::RequestBuilder {
        req = req.bearer_auth(key);
        if let Some(ref referer) = self.referer {
            req = req.header("HTTP-Referer", referer);
        }
        if let Some(ref title) = self.title {
            req = req.header("X-Title", title);
        }
        req
    }

    /// Builds the JSON request body for the chat completions endpoint.
    ///
    /// Mirrors OpenAI's schema, including the `stream_options.include_usage`
    /// flag that makes the final streamed chunk carry token counts.
    fn build_body(req: &CompletionRequest, stream: bool) -> Value {
        let mut body = json!({
            "model": req.model,
            "messages": req.messages,
        });

        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(n) = req.max_tokens {
            body["max_tokens"] = json!(n);
        }
        if let Some(p) = req.top_p {
            body["top_p"] = json!(p);
        }
        if let Some(ref stop) = req.stop {
            body["stop"] = json!(stop);
        }
        if let Some(ref tools) = req.tools {
            body["tools"] = json!(tools);
        }
        if let Some(ref choice) = req.tool_choice {
            body["tool_choice"] = json!(choice);
        }
        if stream {
            body["stream"] = json!(true);
            body["stream_options"] = json!({ "include_usage": true });
        }

        body
    }

    fn map_response(body: Value) -> Result<CompletionResponse, CosmosError> {
        let id = body["id"].as_str().map(str::to_owned);
        let model = body["model"].as_str().map(str::to_owned);

        let choices = body["choices"]
            .as_array()
            .ok_or_else(|| CosmosError::InvalidResponse("missing 'choices' field".into()))?
            .iter()
            .map(|c| {
                let role = c["message"]["role"]
                    .as_str()
                    .unwrap_or("assistant")
                    .to_owned();
                let content = c["message"]["content"].as_str().unwrap_or("").to_owned();
                let finish_reason = c["finish_reason"].as_str().map(str::to_owned);
                let index = c["index"].as_u64().unwrap_or(0) as u32;
                let tool_calls = c["message"]["tool_calls"]
                    .as_array()
                    .map(|calls| {
                        calls
                            .iter()
                            .map(|tc| {
                                let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                                ToolCall {
                                    id: tc["id"].as_str().unwrap_or_default().to_owned(),
                                    name: tc["function"]["name"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .to_owned(),
                                    input: serde_json::from_str(args_str).unwrap_or(Value::Null),
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Choice {
                    index,
                    message: Message { role, content },
                    finish_reason,
                    tool_calls,
                }
            })
            .collect();

        let usage = if body["usage"].is_object() {
            Some(Usage {
                prompt_tokens: body["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: body["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: body["usage"]["total_tokens"].as_u64().unwrap_or(0) as u32,
            })
        } else {
            None
        };

        Ok(CompletionResponse {
            id,
            model,
            choices,
            usage,
        })
    }

    /// Maps an error response to a [`CosmosError`].
    ///
    /// OpenRouter reports upstream provider failures with its own status code
    /// and an `error.message` body, matching OpenAI's error envelope. A 402 is
    /// returned when the account is out of credits.
    fn handle_error(status: u16, body: &Value) -> CosmosError {
        let msg = body["error"]["message"]
            .as_str()
            .unwrap_or("unknown error")
            .to_owned();
        match status {
            401 | 403 => CosmosError::Authentication(msg),
            402 => CosmosError::InsufficientQuota(msg),
            404 => CosmosError::NotFound(msg),
            429 => CosmosError::RateLimit(msg),
            400 | 422 => CosmosError::InvalidRequest(msg),
            s if s >= 500 => CosmosError::Server(msg),
            _ => CosmosError::InvalidResponse(format!("unexpected status {status}: {msg}")),
        }
    }
}

impl Provider for OpenRouterProvider {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            let key = self.resolved_key()?;
            let body = Self::build_body(req, false);

            let request = self.authorize(
                self.http
                    .post(format!("{}/chat/completions", self.base_url)),
                key,
            );
            let resp = request.json(&body).send().await?;

            let status = resp.status().as_u16();
            let json: Value = resp.json().await?;

            if (200..300).contains(&status) {
                Self::map_response(json)
            } else {
                Err(Self::handle_error(status, &json))
            }
        })
    }

    fn stream_completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionStream, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            let key = self.resolved_key()?;
            let body = Self::build_body(req, true);

            let request = self
                .authorize(
                    self.http
                        .post(format!("{}/chat/completions", self.base_url)),
                    key,
                )
                .header("accept", "text/event-stream");
            let resp = request.json(&body).send().await?;

            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                let json: Value = resp.json().await.unwrap_or(Value::Null);
                return Err(Self::handle_error(status, &json));
            }

            Ok(stream_from_response(resp, parse_openai_chunk))
        })
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            // OpenRouter's model catalogue is public; the key is sent when
            // available so per-account model visibility is respected.
            let mut request = self.http.get(format!("{}/models", self.base_url));
            if let Some(key) = self.api_key.as_deref() {
                request = self.authorize(request, key);
            }

            let resp = request.send().await?;

            let status = resp.status().as_u16();
            let json: Value = resp.json().await?;

            if (200..300).contains(&status) {
                let ids = json["data"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_owned))
                    .collect();
                Ok(ids)
            } else {
                Err(Self::handle_error(status, &json))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a provider with no environment fallback, so tests do not depend
    /// on the developer's shell.
    fn provider(key: Option<&str>) -> OpenRouterProvider {
        OpenRouterProvider {
            api_key: key.map(str::to_owned),
            base_url: DEFAULT_BASE_URL.to_owned(),
            referer: None,
            title: None,
            http: HttpClient::new(),
        }
    }

    #[test]
    fn reports_streaming_support() {
        assert!(provider(Some("sk-or-test")).supports_streaming());
    }

    #[test]
    fn build_body_sets_stream_flags_only_when_streaming() {
        let req = CompletionRequest::new("openai/gpt-4o", vec![Message::user("hi")]);

        let plain = OpenRouterProvider::build_body(&req, false);
        assert!(plain.get("stream").is_none());

        let streamed = OpenRouterProvider::build_body(&req, true);
        assert_eq!(streamed["stream"], serde_json::json!(true));
        assert_eq!(streamed["stream_options"]["include_usage"], true);
    }

    #[test]
    fn missing_key_yields_auth_error() {
        let err = provider(None).resolved_key().unwrap_err();
        assert!(matches!(err, CosmosError::Authentication(_)));
        assert!(err.to_string().contains("OPENROUTER_API_KEY"));
    }

    #[test]
    fn builder_sets_attribution_headers() {
        let p = provider(Some("sk-or-test"))
            .with_referer("https://example.com")
            .with_title("my-app");
        assert_eq!(p.referer.as_deref(), Some("https://example.com"));
        assert_eq!(p.title.as_deref(), Some("my-app"));
    }

    #[test]
    fn map_response_parses_openrouter_body() {
        let body = serde_json::json!({
            "id": "gen-1",
            "model": "anthropic/claude-3.5-sonnet",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hello!" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });
        let resp = OpenRouterProvider::map_response(body).unwrap();
        assert_eq!(resp.content(), Some("Hello!"));
        assert_eq!(resp.model.as_deref(), Some("anthropic/claude-3.5-sonnet"));
        assert!(!resp.tool_use());
        assert_eq!(resp.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn map_response_parses_tool_calls() {
        let body = serde_json::json!({
            "id": "gen-2",
            "model": "openai/gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Boston\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });
        let resp = OpenRouterProvider::map_response(body).unwrap();
        assert!(resp.tool_use());
        let calls = resp.tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].input, serde_json::json!({"city": "Boston"}));
    }

    #[test]
    fn map_response_missing_choices_is_invalid() {
        let err = OpenRouterProvider::map_response(serde_json::json!({"id": "x"})).unwrap_err();
        assert!(matches!(err, CosmosError::InvalidResponse(_)));
    }

    #[test]
    fn handle_error_maps_status_codes() {
        let body = serde_json::json!({"error": {"message": "nope"}});
        assert!(matches!(
            OpenRouterProvider::handle_error(401, &body),
            CosmosError::Authentication(_)
        ));
        assert!(matches!(
            OpenRouterProvider::handle_error(402, &body),
            CosmosError::InsufficientQuota(_)
        ));
        assert!(matches!(
            OpenRouterProvider::handle_error(404, &body),
            CosmosError::NotFound(_)
        ));
        assert!(matches!(
            OpenRouterProvider::handle_error(429, &body),
            CosmosError::RateLimit(_)
        ));
        assert!(matches!(
            OpenRouterProvider::handle_error(400, &body),
            CosmosError::InvalidRequest(_)
        ));
        assert!(matches!(
            OpenRouterProvider::handle_error(503, &body),
            CosmosError::Server(_)
        ));
    }
}
