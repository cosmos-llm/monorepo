use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::error::CosmosError;
use crate::model::ModelInfo;
use crate::providers::openai::{
    choices_from_body, parse_openai_chunk, usage_from_openai, OpenAiProvider, WireOptions,
};
use crate::providers::{
    map_openai_error, retry_after_from_headers, stream_from_response, transport_error,
    CompletionStream, Provider,
};
use crate::reasoning::Reasoning;
use crate::routing::OpenRouterRouting;
use crate::types::{CompletionRequest, CompletionResponse};

/// Default API root, used unless overridden by
/// [`OpenRouterProvider::with_base_url`] or the `OPENROUTER_BASE_URL`
/// environment variable.
pub const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Name this provider reports from [`Provider::name`] and attaches to errors.
const PROVIDER_NAME: &str = "openrouter";

/// Provider implementation for the OpenRouter API.
///
/// OpenRouter proxies many upstream models behind an OpenAI-compatible
/// interface, so request and response shapes match
/// [`OpenAiProvider`]. Model
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
    routing: Option<OpenRouterRouting>,
    report_route: bool,
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
            routing: None,
            report_route: false,
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

    /// Sets the provider-routing preference applied to every request
    /// (builder pattern).
    ///
    /// This is the default, not an override: a request carrying its own
    /// routing — see
    /// [`CompletionRequest::with_openrouter_routing`](crate::CompletionRequest::with_openrouter_routing)
    /// — replaces this wholesale rather than merging field by field. Merging
    /// would make a request that sets `sort` silently inherit an `only` list
    /// from the client, which is the opposite of what "this request routes
    /// like *this*" should mean.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::openrouter::OpenRouterProvider;
    /// use cosmos_llm::{DataCollection, OpenRouterRouting};
    ///
    /// // Every request from this provider avoids prompt-retaining upstreams.
    /// let provider = OpenRouterProvider::new(Some("sk-or-test".into()))
    ///     .with_routing(OpenRouterRouting::new().data_collection(DataCollection::Deny));
    /// assert!(provider.routing().is_some());
    /// ```
    pub fn with_routing(mut self, routing: OpenRouterRouting) -> Self {
        self.routing = Some(routing);
        self
    }

    /// Returns the default routing preference, if one is set.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::openrouter::OpenRouterProvider;
    ///
    /// let provider = OpenRouterProvider::new(Some("sk-or-test".into()));
    /// assert!(provider.routing().is_none());
    /// ```
    pub fn routing(&self) -> Option<&OpenRouterRouting> {
        self.routing.as_ref()
    }

    /// Asks OpenRouter to report which upstream provider served each request
    /// (builder pattern).
    ///
    /// Sends the `X-OpenRouter-Metadata` header, which makes the response
    /// carry the routing metadata that populates
    /// [`CompletionResponse::route`](crate::CompletionResponse::route).
    /// Off by default, because it enlarges every response for callers who
    /// never read it.
    ///
    /// Worth turning on whenever a routing preference is set: it is the only
    /// way to tell an `order` that was honoured from one that quietly fell
    /// through to a fallback. Note that a cache hit reports nothing even with
    /// this enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::openrouter::OpenRouterProvider;
    ///
    /// let provider = OpenRouterProvider::new(Some("sk-or-test".into()))
    ///     .with_route_reporting(true);
    /// ```
    pub fn with_route_reporting(mut self, report: bool) -> Self {
        self.report_route = report;
        self
    }

    fn resolved_key(&self) -> Result<&str, CosmosError> {
        self.api_key
            .as_deref()
            .ok_or_else(|| CosmosError::Authentication {
                provider: PROVIDER_NAME.to_owned(),
                message: "OpenRouter API key not set. Export OPENROUTER_API_KEY or \
                          CLLM__OPENROUTER__API_KEY."
                    .to_owned(),
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
        if self.report_route {
            req = req.header("X-OpenRouter-Metadata", "enabled");
        }
        req
    }

    /// Builds the JSON request body for the chat completions endpoint.
    ///
    /// Mirrors OpenAI's schema, including the `stream_options.include_usage`
    /// flag that makes the final streamed chunk carry token counts. Message
    /// translation — tool results and replayed tool calls — is shared with the
    /// OpenAI provider, since the wire format is identical.
    ///
    /// Adds one field OpenAI has no equivalent for: `usage.include`, which
    /// makes the response report the charged dollar cost. See
    /// [`crate::Usage::cost`].
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::InvalidRequest`] if a message cannot be
    /// represented, e.g. a tool result with no `tool_call_id`.
    fn build_body(
        req: &CompletionRequest,
        stream: bool,
        default_routing: Option<&OpenRouterRouting>,
    ) -> Result<Value, CosmosError> {
        // OpenRouter forwards `cache_control` to Anthropic models and
        // `reasoning_details` to the models that issue them.
        let messages = OpenAiProvider::messages_to_wire(
            &req.messages,
            WireOptions {
                cache_parts: true,
                reasoning_details: true,
            },
        )?;

        let mut body = json!({
            "model": req.model,
            "messages": messages,
            // Ask for the dollar figure the account was actually charged.
            // OpenRouter omits it otherwise, and a price derived locally from a
            // token count is a guess against a table that drifts every time a
            // provider repositions a model. Requesting it costs nothing.
            "usage": { "include": true },
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
        match req.reasoning {
            Some(Reasoning::Budget { tokens }) => {
                body["reasoning"] = json!({ "max_tokens": tokens });
            }
            Some(Reasoning::Effort { effort }) => {
                body["reasoning"] = json!({ "effort": effort.as_str() });
            }
            _ => {}
        }

        // The client-wide routing default. Written before `extra` so that a
        // request carrying its own `provider` block replaces it.
        if let Some(routing) = default_routing.filter(|r| !r.is_empty()) {
            body["provider"] = routing.to_value();
        }

        // Caller-supplied top-level fields, last so they win. This is what
        // carries a per-request routing block, and anything else OpenRouter
        // accepts that this crate has no typed builder for.
        for (key, value) in &req.extra {
            body[key.as_str()] = value.clone();
        }

        Ok(body)
    }

    fn map_response(body: Value) -> Result<CompletionResponse, CosmosError> {
        let id = body["id"].as_str().map(str::to_owned);
        let model = body["model"].as_str().map(str::to_owned);

        let choices = choices_from_body(PROVIDER_NAME, &body)?;

        let usage = body["usage"]
            .is_object()
            .then(|| usage_from_openai(&body["usage"]));

        Ok(CompletionResponse {
            id,
            model,
            choices,
            usage,
            // Present only when the request asked for it; see
            // `with_route_reporting`.
            route: crate::types::RouteInfo::from_metadata(&body["openrouter_metadata"]),
        })
    }

    /// Maps an error response to a [`CosmosError`].
    ///
    /// OpenRouter reports upstream provider failures with its own status code
    /// and an `error.message` body, matching OpenAI's error envelope, so the
    /// shared [`map_openai_error`] handles it — including the 402 it returns
    /// when the account is out of credits.
    fn handle_error(status: u16, body: &Value, retry_after: Option<Duration>) -> CosmosError {
        map_openai_error(PROVIDER_NAME, status, body, retry_after)
    }

    /// Attributes a transport failure to this provider.
    fn network_error(source: reqwest::Error) -> CosmosError {
        transport_error(PROVIDER_NAME, source)
    }
}

impl Provider for OpenRouterProvider {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            let key = self.resolved_key()?;
            let body = Self::build_body(req, false, self.routing.as_ref())?;

            let request = self.authorize(
                self.http
                    .post(format!("{}/chat/completions", self.base_url)),
                key,
            );
            let resp = request
                .json(&body)
                .send()
                .await
                .map_err(Self::network_error)?;

            let status = resp.status().as_u16();
            let retry_after = retry_after_from_headers(resp.headers());
            let json: Value = resp.json().await.map_err(Self::network_error)?;

            if (200..300).contains(&status) {
                Self::map_response(json)
            } else {
                Err(Self::handle_error(status, &json, retry_after))
            }
        })
    }

    fn stream_completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionStream, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            let key = self.resolved_key()?;
            let body = Self::build_body(req, true, self.routing.as_ref())?;

            let request = self
                .authorize(
                    self.http
                        .post(format!("{}/chat/completions", self.base_url)),
                    key,
                )
                .header("accept", "text/event-stream");
            let resp = request
                .json(&body)
                .send()
                .await
                .map_err(Self::network_error)?;

            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                let retry_after = retry_after_from_headers(resp.headers());
                let json: Value = resp.json().await.unwrap_or(Value::Null);
                return Err(Self::handle_error(status, &json, retry_after));
            }

            Ok(stream_from_response(PROVIDER_NAME, resp, |data| {
                parse_openai_chunk(PROVIDER_NAME, data)
            }))
        })
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        PROVIDER_NAME
    }

    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            let entries = self.fetch_models().await?;
            Ok(entries
                .iter()
                .filter_map(|m| m["id"].as_str().map(str::to_owned))
                .collect())
        })
    }

    fn lists_model_details(&self) -> bool {
        true
    }

    fn models_info<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ModelInfo>, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            let entries = self.fetch_models().await?;
            Ok(entries
                .iter()
                .filter_map(ModelInfo::from_openrouter)
                .collect())
        })
    }
}

impl OpenRouterProvider {
    /// Fetches the `/models` catalogue.
    ///
    /// The catalogue is public; the key is sent when available so per-account
    /// model visibility is respected.
    async fn fetch_models(&self) -> Result<Vec<Value>, CosmosError> {
        let mut request = self.http.get(format!("{}/models", self.base_url));
        if let Some(key) = self.api_key.as_deref() {
            request = self.authorize(request, key);
        }

        let resp = request.send().await.map_err(Self::network_error)?;

        let status = resp.status().as_u16();
        let retry_after = retry_after_from_headers(resp.headers());
        let json: Value = resp.json().await.map_err(Self::network_error)?;

        if (200..300).contains(&status) {
            Ok(json["data"].as_array().cloned().unwrap_or_default())
        } else {
            Err(Self::handle_error(status, &json, retry_after))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    /// Builds a provider with no environment fallback, so tests do not depend
    /// on the developer's shell.
    fn provider(key: Option<&str>) -> OpenRouterProvider {
        OpenRouterProvider {
            api_key: key.map(str::to_owned),
            base_url: DEFAULT_BASE_URL.to_owned(),
            referer: None,
            title: None,
            routing: None,
            report_route: false,
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

        let plain = OpenRouterProvider::build_body(&req, false, None).unwrap();
        assert!(plain.get("stream").is_none());

        let streamed = OpenRouterProvider::build_body(&req, true, None).unwrap();
        assert_eq!(streamed["stream"], serde_json::json!(true));
        assert_eq!(streamed["stream_options"]["include_usage"], true);
    }

    #[test]
    fn build_body_translates_tool_messages_like_openai() {
        use crate::types::ToolCall;

        let req = CompletionRequest::new(
            "openai/gpt-4o",
            vec![
                Message::user("q"),
                Message::assistant_with_tools(
                    "",
                    vec![ToolCall {
                        id: "call_1".into(),
                        name: "search".into(),
                        input: serde_json::json!({"q": "rust"}),
                    }],
                ),
                Message::tool_result("call_1", "found"),
            ],
        );
        let body = OpenRouterProvider::build_body(&req, false, None).unwrap();
        let msgs = body["messages"].as_array().unwrap();

        assert_eq!(msgs[1]["tool_calls"][0]["id"], "call_1");
        // OpenAI's JSON-string encoding of arguments, not a native object.
        assert!(msgs[1]["tool_calls"][0]["function"]["arguments"].is_string());
        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "call_1");
    }

    #[test]
    fn missing_key_yields_auth_error() {
        let err = provider(None).resolved_key().unwrap_err();
        assert!(matches!(err, CosmosError::Authentication { .. }));
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
        assert_eq!(resp.content().as_deref(), Some("Hello!"));
        assert_eq!(resp.model.as_deref(), Some("anthropic/claude-3.5-sonnet"));
        assert!(!resp.tool_use());
        let usage = resp.usage.unwrap();
        assert_eq!(usage.total_tokens, 15);
        // A usage block without a price leaves cost unknown, not zero.
        assert_eq!(usage.cost, None);
    }

    #[test]
    fn map_response_reads_the_charged_cost() {
        let body = serde_json::json!({
            "id": "gen-1",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hi" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15,
                "cost": 0.00042
            }
        });
        let usage = OpenRouterProvider::map_response(body)
            .unwrap()
            .usage
            .unwrap();
        assert_eq!(usage.cost, Some(0.00042));
    }

    #[test]
    fn request_body_asks_for_cost_accounting() {
        let req = CompletionRequest::new("openai/gpt-4o", vec![Message::user("hi")]);
        let body = OpenRouterProvider::build_body(&req, false, None).unwrap();
        // Without this flag OpenRouter omits the price entirely.
        assert_eq!(body["usage"]["include"], serde_json::json!(true));
    }

    #[test]
    fn build_body_omits_provider_when_no_routing_is_set() {
        let req = CompletionRequest::new("openai/gpt-4o", vec![Message::user("hi")]);
        let body = OpenRouterProvider::build_body(&req, false, None).unwrap();
        // A request with no preference must not constrain routing at all.
        assert!(body.get("provider").is_none());
    }

    #[test]
    fn build_body_writes_the_client_wide_routing_default() {
        let req = CompletionRequest::new("openai/gpt-4o", vec![Message::user("hi")]);
        let routing = OpenRouterRouting::new().sort(crate::routing::ProviderSort::Price);
        let body = OpenRouterProvider::build_body(&req, false, Some(&routing)).unwrap();
        assert_eq!(body["provider"]["sort"], serde_json::json!("price"));
    }

    #[test]
    fn an_empty_default_routing_is_not_sent() {
        let req = CompletionRequest::new("openai/gpt-4o", vec![Message::user("hi")]);
        let body =
            OpenRouterProvider::build_body(&req, false, Some(&OpenRouterRouting::new())).unwrap();
        // `provider: {}` is noise; omit it.
        assert!(body.get("provider").is_none());
    }

    #[test]
    fn per_request_routing_replaces_the_client_default() {
        // Merging instead of replacing would let a client-wide `only` leak
        // into a request that asked to route purely by price.
        let req = CompletionRequest::new("openai/gpt-4o", vec![Message::user("hi")])
            .with_openrouter_routing(
                OpenRouterRouting::new().sort(crate::routing::ProviderSort::Throughput),
            );
        let default = OpenRouterRouting::new().only(["azure"]);

        let body = OpenRouterProvider::build_body(&req, false, Some(&default)).unwrap();
        assert_eq!(body["provider"]["sort"], serde_json::json!("throughput"));
        assert!(body["provider"].get("only").is_none());
    }

    #[test]
    fn extra_fields_are_merged_into_the_body() {
        let req = CompletionRequest::new("openai/gpt-4o", vec![Message::user("hi")])
            .with_extra("models", serde_json::json!(["a/b", "c/d"]))
            .with_extra("user", serde_json::json!("acct-1"));
        let body = OpenRouterProvider::build_body(&req, false, None).unwrap();

        assert_eq!(body["models"], serde_json::json!(["a/b", "c/d"]));
        assert_eq!(body["user"], serde_json::json!("acct-1"));
        // The fields the provider builds itself survive alongside them.
        assert_eq!(body["model"], serde_json::json!("openai/gpt-4o"));
        assert!(body["messages"].is_array());
    }

    #[test]
    fn extra_can_override_a_field_the_provider_builds() {
        let req = CompletionRequest::new("openai/gpt-4o", vec![Message::user("hi")])
            .with_extra("usage", serde_json::json!({ "include": false }));
        let body = OpenRouterProvider::build_body(&req, false, None).unwrap();
        assert_eq!(body["usage"]["include"], serde_json::json!(false));
    }

    #[test]
    fn map_response_reads_the_serving_provider_from_metadata() {
        let body = serde_json::json!({
            "id": "gen-1",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hi" },
                "finish_reason": "stop"
            }],
            "openrouter_metadata": {
                "attempt": 2,
                "endpoints": {
                    "available": [
                        { "provider": "Together", "model": "x/y", "selected": false },
                        { "provider": "DeepInfra", "model": "x/y", "selected": true }
                    ]
                }
            }
        });
        let route = OpenRouterProvider::map_response(body)
            .unwrap()
            .route
            .unwrap();
        assert_eq!(route.provider.as_deref(), Some("DeepInfra"));
        assert_eq!(route.model.as_deref(), Some("x/y"));
        // Two attempts means the first choice failed — the signal that a
        // routing preference was not honoured.
        assert_eq!(route.attempts, 2);
    }

    #[test]
    fn map_response_leaves_route_unset_without_metadata() {
        let body = serde_json::json!({
            "id": "gen-1",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hi" },
                "finish_reason": "stop"
            }]
        });
        // Absent metadata is "not reported", not a parse failure.
        assert_eq!(OpenRouterProvider::map_response(body).unwrap().route, None);
    }

    #[test]
    fn builder_stores_routing_and_route_reporting() {
        let p = provider(Some("sk-or-test"))
            .with_routing(OpenRouterRouting::pinned_to("azure"))
            .with_route_reporting(true);
        assert_eq!(p.routing().unwrap().only, vec!["azure"]);
        assert!(p.report_route);
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
        assert!(matches!(err, CosmosError::InvalidResponse { .. }));
    }

    #[test]
    fn handle_error_maps_status_codes() {
        let body = serde_json::json!({"error": {"message": "nope"}});
        assert!(matches!(
            OpenRouterProvider::handle_error(401, &body, None),
            CosmosError::Authentication { .. }
        ));
        // OpenRouter's out-of-credits status.
        assert!(matches!(
            OpenRouterProvider::handle_error(402, &body, None),
            CosmosError::InsufficientQuota { .. }
        ));
        assert!(matches!(
            OpenRouterProvider::handle_error(404, &body, None),
            CosmosError::ResourceNotFound { .. }
        ));
        assert!(matches!(
            OpenRouterProvider::handle_error(429, &body, None),
            CosmosError::RateLimit { .. }
        ));
        assert!(matches!(
            OpenRouterProvider::handle_error(400, &body, None),
            CosmosError::InvalidRequest { .. }
        ));
        assert!(matches!(
            OpenRouterProvider::handle_error(503, &body, None),
            CosmosError::Server { .. }
        ));
    }

    #[test]
    fn errors_are_attributed_to_openrouter_not_openai() {
        // The error mapper is shared with OpenAI; a misattributed provider name
        // would send a caller debugging the wrong service.
        let body = serde_json::json!({"error": {"message": "nope"}});
        let err = OpenRouterProvider::handle_error(429, &body, None);
        assert_eq!(err.provider(), "openrouter");
    }

    #[test]
    fn handle_error_honors_retry_after() {
        let body = serde_json::json!({"error": {"message": "slow down"}});
        let err = OpenRouterProvider::handle_error(429, &body, Some(Duration::from_secs(7)));
        assert_eq!(err.retry_after(), Some(Duration::from_secs(7)));
        assert!(err.is_retryable());
    }
}

#[cfg(test)]
mod block_tests {
    use super::*;
    use crate::content::{CacheHint, ContentBlock};
    use crate::reasoning::{Reasoning, ReasoningEffort};
    use crate::types::Message;
    use serde_json::json;

    #[test]
    fn reasoning_maps_to_the_reasoning_object() {
        let req = CompletionRequest::new("anthropic/claude-sonnet-4", vec![Message::user("hi")])
            .with_reasoning(Reasoning::effort(ReasoningEffort::Low));
        let body = OpenRouterProvider::build_body(&req, false, None).unwrap();
        assert_eq!(body["reasoning"], json!({"effort": "low"}));

        let req = CompletionRequest::new("m", vec![Message::user("hi")])
            .with_reasoning(Reasoning::budget(3000));
        let body = OpenRouterProvider::build_body(&req, false, None).unwrap();
        assert_eq!(body["reasoning"], json!({"max_tokens": 3000}));
    }

    #[test]
    fn reasoning_and_details_become_replayable_blocks() {
        let resp = OpenRouterProvider::map_response(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "answer",
                    "reasoning": "plain",
                    "reasoning_details": [
                        { "type": "reasoning.text", "text": "plan", "signature": "sg" },
                        { "type": "reasoning.encrypted", "data": "blob" }
                    ]
                },
                "finish_reason": "stop"
            }]
        }))
        .unwrap();
        let msg = resp.to_assistant_message();
        assert_eq!(
            msg.content,
            vec![
                ContentBlock::Thinking {
                    text: "plan".into(),
                    signature: Some("sg".into())
                },
                ContentBlock::RedactedThinking {
                    data: "blob".into()
                },
                ContentBlock::text("answer"),
            ]
        );

        let req = CompletionRequest::new("m", vec![Message::user("q"), msg]);
        let body = OpenRouterProvider::build_body(&req, false, None).unwrap();
        assert_eq!(
            body["messages"][1]["reasoning_details"],
            json!([
                { "type": "reasoning.text", "text": "plan", "signature": "sg" },
                { "type": "reasoning.encrypted", "data": "blob" }
            ])
        );
    }

    #[test]
    fn cache_hints_are_forwarded_as_content_parts() {
        let req = CompletionRequest::new(
            "anthropic/claude-sonnet-4",
            vec![Message::system("long prompt").with_cache(CacheHint::Ephemeral)],
        );
        let body = OpenRouterProvider::build_body(&req, false, None).unwrap();
        assert_eq!(
            body["messages"][0]["content"],
            json!([{"type": "text", "text": "long prompt", "cache_control": {"type": "ephemeral"}}])
        );
    }

    #[test]
    fn usage_reports_cache_and_reasoning_counts_and_cost() {
        let resp = OpenRouterProvider::map_response(json!({
            "choices": [{ "message": { "role": "assistant", "content": "x" } }],
            "usage": {
                "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15, "cost": 0.5,
                "prompt_tokens_details": { "cached_tokens": 4, "cache_write_tokens": 2 },
                "completion_tokens_details": { "reasoning_tokens": 3 }
            }
        }))
        .unwrap();
        let usage = resp.usage.unwrap();
        assert_eq!(usage.cache_read_tokens, Some(4));
        assert_eq!(usage.cache_write_tokens, Some(2));
        assert_eq!(usage.reasoning_tokens, Some(3));
        assert_eq!(usage.cost, Some(0.5));
    }

    #[test]
    fn the_models_listing_becomes_model_info() {
        let entry = json!({
            "id": "anthropic/claude-sonnet-4",
            "context_length": 200000,
            "architecture": { "input_modalities": ["text", "image"] },
            "top_provider": { "max_completion_tokens": 64000 },
            "pricing": {
                "prompt": "0.000003", "completion": "0.000015",
                "input_cache_read": "0.0000003", "input_cache_write": "0.00000375"
            },
            "supported_parameters": ["tools", "reasoning", "temperature"]
        });
        let info = ModelInfo::from_openrouter(&entry).unwrap();
        assert_eq!(info.context_window, Some(200_000));
        assert_eq!(info.max_output_tokens, Some(64_000));
        assert!(info.supports_tools && info.supports_images && info.supports_reasoning);
        assert!(info.supports_prompt_cache);
        let p = info.pricing.unwrap();
        assert!((p.input - 3.0).abs() < 1e-9);
        assert!((p.output - 15.0).abs() < 1e-9);
        assert!((p.cache_write.unwrap() - 3.75).abs() < 1e-9);
    }
}
