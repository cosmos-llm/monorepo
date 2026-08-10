use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::error::CosmosError;
use crate::providers::{
    map_openai_error, retry_after_from_headers, stream_from_response, transport_error,
    CompletionStream, Provider,
};
use crate::types::{
    Choice, CompletionRequest, CompletionResponse, Message, StreamChunk, ToolCall, ToolCallDelta,
    Usage,
};

/// Default API root, used unless overridden by
/// [`OpenAiProvider::with_base_url`] or the `OPENAI_BASE_URL` environment
/// variable.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// Name this provider reports from [`Provider::name`] and attaches to errors.
const PROVIDER_NAME: &str = "openai";

/// Provider implementation for the OpenAI API.
///
/// Supports chat completions, streaming, tool calling, and model listing.
/// Reads the API key from the `OPENAI_API_KEY` or `CLLM__OPENAI__API_KEY`
/// environment variable when none is supplied at construction.
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::providers::openai::OpenAiProvider;
/// use cosmos_llm::providers::Provider;
/// use cosmos_llm::types::{CompletionRequest, Message};
///
/// # tokio_test::block_on(async {
/// let provider = OpenAiProvider::new(Some("sk-test".into()));
/// let req = CompletionRequest::new("gpt-4o", vec![Message::user("Hello")]);
/// // let resp = provider.completion(&req).await.unwrap();
/// # })
/// ```
pub struct OpenAiProvider {
    api_key: Option<String>,
    base_url: String,
    http: HttpClient,
}

impl OpenAiProvider {
    /// Creates a new [`OpenAiProvider`].
    ///
    /// When `api_key` is `None`, the provider falls back to the
    /// `OPENAI_API_KEY` or `CLLM__OPENAI__API_KEY` environment variables. The
    /// API root defaults to [`DEFAULT_BASE_URL`] unless `OPENAI_BASE_URL` or
    /// `CLLM__OPENAI__BASE_URL` is set.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::openai::OpenAiProvider;
    /// let provider = OpenAiProvider::new(None);
    /// ```
    pub fn new(api_key: Option<String>) -> Self {
        let key = api_key
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .or_else(|| std::env::var("CLLM__OPENAI__API_KEY").ok());
        Self {
            api_key: key,
            base_url: std::env::var("OPENAI_BASE_URL")
                .or_else(|_| std::env::var("CLLM__OPENAI__BASE_URL"))
                .unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned()),
            http: HttpClient::new(),
        }
    }

    /// Points the provider at a different API root (builder pattern).
    ///
    /// Useful for OpenAI-compatible servers — Azure OpenAI, a local
    /// llama.cpp or vLLM instance, a corporate proxy — and for pointing tests
    /// at a mock server. Any trailing slash is trimmed, so
    /// `"http://localhost:8080/v1/"` and `"http://localhost:8080/v1"` behave
    /// identically.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::openai::OpenAiProvider;
    ///
    /// let provider = OpenAiProvider::new(Some("sk-test".into()))
    ///     .with_base_url("http://localhost:8080/v1");
    /// assert_eq!(provider.base_url(), "http://localhost:8080/v1");
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
    /// use cosmos_llm::providers::openai::{OpenAiProvider, DEFAULT_BASE_URL};
    ///
    /// # std::env::remove_var("OPENAI_BASE_URL");
    /// # std::env::remove_var("CLLM__OPENAI__BASE_URL");
    /// let provider = OpenAiProvider::new(Some("sk-test".into()));
    /// assert_eq!(provider.base_url(), DEFAULT_BASE_URL);
    /// ```
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn resolved_key(&self) -> Result<&str, CosmosError> {
        self.api_key
            .as_deref()
            .ok_or_else(|| CosmosError::Authentication {
                provider: PROVIDER_NAME.to_owned(),
                message: "OpenAI API key not set. Export OPENAI_API_KEY or \
                          CLLM__OPENAI__API_KEY."
                    .to_owned(),
            })
    }

    /// Translates one [`Message`] into OpenAI's chat message shape.
    ///
    /// Three cases differ from a plain `{role, content}` object:
    ///
    /// * A tool result becomes `role: "tool"` with `tool_call_id`. OpenAI
    ///   rejects a tool message without it.
    /// * An assistant turn that requested tools carries a `tool_calls` array
    ///   whose `function.arguments` is a JSON **string**, not an object.
    /// * An assistant message with tool calls and no text sends
    ///   `content: null`, which is what OpenAI returned in the first place.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::InvalidRequest`] for a tool-role message with no
    /// `tool_call_id`. Catching it here names the offending message; letting it
    /// reach the API produces a 400 that does not.
    pub(crate) fn message_to_wire(msg: &Message) -> Result<Value, CosmosError> {
        if msg.is_tool_result() {
            let id = msg
                .tool_call_id
                .as_deref()
                .ok_or_else(|| CosmosError::InvalidRequest {
                    provider: PROVIDER_NAME.to_owned(),
                    message: "tool result message has no tool_call_id; OpenAI cannot \
                              associate it with a call. Build it with Message::tool_result."
                        .to_owned(),
                    param: Some("messages[].tool_call_id".to_owned()),
                })?;
            let mut wire = json!({
                "role": "tool",
                "tool_call_id": id,
                "content": msg.content,
            });
            if let Some(ref name) = msg.name {
                wire["name"] = json!(name);
            }
            return Ok(wire);
        }

        if msg.tool_calls.is_empty() {
            return Ok(json!({ "role": msg.role, "content": msg.content }));
        }

        let calls: Vec<Value> = msg
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {
                        "name": call.name,
                        // OpenAI encodes arguments as a JSON string, and
                        // expects the replayed call to match.
                        "arguments": call.input.to_string(),
                    }
                })
            })
            .collect();

        Ok(json!({
            "role": msg.role,
            // A tool-calling turn commonly has no text; OpenAI's own response
            // uses null there, so replaying an empty string would not match.
            "content": if msg.content.is_empty() { Value::Null } else { json!(msg.content) },
            "tool_calls": calls,
        }))
    }

    /// Builds the JSON request body for the chat completions endpoint.
    ///
    /// When `stream` is set, `stream_options.include_usage` is requested too,
    /// so the final chunk carries token counts; OpenAI omits usage from
    /// streamed responses otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::InvalidRequest`] if a message cannot be
    /// represented — see [`OpenAiProvider::message_to_wire`].
    fn build_body(req: &CompletionRequest, stream: bool) -> Result<Value, CosmosError> {
        let messages = req
            .messages
            .iter()
            .map(Self::message_to_wire)
            .collect::<Result<Vec<_>, _>>()?;

        let mut body = json!({
            "model": req.model,
            "messages": messages,
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

        Ok(body)
    }

    fn map_response(body: Value) -> Result<CompletionResponse, CosmosError> {
        let id = body["id"].as_str().map(str::to_owned);
        let model = body["model"].as_str().map(str::to_owned);

        let choices = body["choices"]
            .as_array()
            .ok_or_else(|| CosmosError::InvalidResponse {
                provider: PROVIDER_NAME.to_owned(),
                message: "missing 'choices' field".to_owned(),
            })?
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
                    message: Message::new(role, content),
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

    /// Classifies an error response, honouring `Retry-After` when present.
    ///
    /// Delegates to [`map_openai_error`], which reads the body's `type` and
    /// `code` — a 429 for an exhausted quota is not retryable, while an
    /// ordinary 429 is.
    fn handle_error(status: u16, body: &Value, retry_after: Option<Duration>) -> CosmosError {
        map_openai_error(PROVIDER_NAME, status, body, retry_after)
    }

    /// Attributes a transport failure to this provider.
    fn network_error(source: reqwest::Error) -> CosmosError {
        transport_error(PROVIDER_NAME, source)
    }
}

/// Parses one `chat.completions.chunk` payload into a [`StreamChunk`].
///
/// Shared by the OpenAI and OpenRouter providers, whose streaming wire formats
/// are identical. Returns `Ok(None)` for payloads that carry nothing this
/// crate models, such as the leading `{"role": "assistant"}` delta.
///
/// A chunk's `usage` arrives in a final payload whose `choices` array is
/// empty, so usage is read independently of the choice.
///
/// `provider` attributes the errors this can produce; OpenRouter passes its own
/// name so a failure is not misreported as OpenAI's.
pub(crate) fn parse_openai_chunk(
    provider: &str,
    data: &str,
) -> Result<Option<StreamChunk>, CosmosError> {
    let value: Value = serde_json::from_str(data).map_err(|e| CosmosError::Streaming {
        provider: provider.to_owned(),
        message: format!("malformed stream chunk: {e}"),
    })?;

    // An error can arrive mid-stream, after a 200 response has already
    // committed the connection to streaming. Classify it the same way as a
    // pre-stream error: a mid-stream 429 or overload is just as retryable, and
    // collapsing everything to `Streaming` would hide that.
    if value["error"].is_object() {
        // No status code is available here, so infer one from the body. A
        // provider that reports an error mid-stream without a code is treated
        // as a server-side failure, which is the common case (overload).
        let status = value["error"]["status"].as_u64().unwrap_or(500) as u16;
        return Err(map_openai_error(provider, status, &value, None));
    }

    let choice = &value["choices"][0];
    let delta = &choice["delta"];

    let tool_calls = delta["tool_calls"]
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .enumerate()
                .map(|(position, tc)| ToolCallDelta {
                    // `index` correlates fragments across chunks. It is
                    // always present in practice; fall back to the position
                    // within this chunk so a missing field cannot collapse
                    // parallel calls into one.
                    index: tc["index"].as_u64().unwrap_or(position as u64) as u32,
                    id: tc["id"].as_str().map(str::to_owned),
                    name: tc["function"]["name"].as_str().map(str::to_owned),
                    arguments: tc["function"]["arguments"].as_str().map(str::to_owned),
                })
                .collect()
        })
        .unwrap_or_default();

    let usage = if value["usage"].is_object() {
        Some(Usage {
            prompt_tokens: value["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: value["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: value["usage"]["total_tokens"].as_u64().unwrap_or(0) as u32,
        })
    } else {
        None
    };

    let chunk = StreamChunk {
        delta: delta["content"].as_str().unwrap_or("").to_owned(),
        finish_reason: choice["finish_reason"].as_str().map(str::to_owned),
        tool_calls,
        usage,
    };

    Ok(if chunk.is_empty() { None } else { Some(chunk) })
}

impl Provider for OpenAiProvider {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            let key = self.resolved_key()?;
            let body = Self::build_body(req, false)?;

            let resp = self
                .http
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(key)
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
            let body = Self::build_body(req, true)?;

            let resp = self
                .http
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(key)
                .header("accept", "text/event-stream")
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
            let key = self.resolved_key()?;

            let resp = self
                .http
                .get(format!("{}/models", self.base_url))
                .bearer_auth(key)
                .send()
                .await
                .map_err(Self::network_error)?;

            let status = resp.status().as_u16();
            let retry_after = retry_after_from_headers(resp.headers());
            let json: Value = resp.json().await.map_err(Self::network_error)?;

            if (200..300).contains(&status) {
                let ids = json["data"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_owned))
                    .collect();
                Ok(ids)
            } else {
                Err(Self::handle_error(status, &json, retry_after))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_streaming_support() {
        let p = OpenAiProvider::new(None);
        assert!(p.supports_streaming());
    }

    #[test]
    fn build_body_sets_stream_flags_only_when_streaming() {
        let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")]);

        let plain = OpenAiProvider::build_body(&req, false).unwrap();
        assert!(plain.get("stream").is_none());
        assert!(plain.get("stream_options").is_none());

        let streamed = OpenAiProvider::build_body(&req, true).unwrap();
        assert_eq!(streamed["stream"], serde_json::json!(true));
        assert_eq!(streamed["stream_options"]["include_usage"], true);
    }

    #[test]
    fn build_body_carries_sampling_parameters() {
        let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
            .with_temperature(0.4)
            .with_max_tokens(64)
            .with_top_p(0.8)
            .with_stop(vec!["END".into()]);
        let body = OpenAiProvider::build_body(&req, false).unwrap();
        // Compared as `f32` literals: widening to `f64` is not exact.
        assert_eq!(body["temperature"], serde_json::json!(0.4_f32));
        assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["top_p"], serde_json::json!(0.8_f32));
        assert_eq!(body["stop"], serde_json::json!(["END"]));
    }

    #[test]
    fn wire_plain_message_is_role_and_content_only() {
        let wire = OpenAiProvider::message_to_wire(&Message::user("hi")).unwrap();
        assert_eq!(wire, serde_json::json!({"role": "user", "content": "hi"}));
    }

    #[test]
    fn wire_tool_result_uses_tool_role_and_call_id() {
        let msg = Message::tool_result("call_1", "72F");
        let wire = OpenAiProvider::message_to_wire(&msg).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "role": "tool",
                "tool_call_id": "call_1",
                "content": "72F",
            })
        );
    }

    #[test]
    fn wire_tool_result_includes_optional_name() {
        let msg = Message::tool_result("call_1", "72F").with_name("get_weather");
        let wire = OpenAiProvider::message_to_wire(&msg).unwrap();
        assert_eq!(wire["name"], "get_weather");
    }

    #[test]
    fn wire_tool_result_without_id_fails_locally() {
        // Constructed by hand rather than via `tool_result`, which is the only
        // way to reach this state. The error must name the fix.
        let mut msg = Message::new("tool", "72F");
        msg.tool_call_id = None;
        let err = OpenAiProvider::message_to_wire(&msg).unwrap_err();
        assert!(
            matches!(err, CosmosError::InvalidRequest { message: ref m, .. } if m.contains("tool_call_id"))
        );
    }

    #[test]
    fn wire_assistant_encodes_arguments_as_json_string() {
        let msg = Message::assistant_with_tools(
            "",
            vec![ToolCall {
                id: "call_1".into(),
                name: "get_weather".into(),
                input: serde_json::json!({"city": "Boston"}),
            }],
        );
        let wire = OpenAiProvider::message_to_wire(&msg).unwrap();

        assert_eq!(wire["role"], "assistant");
        // No text on a pure tool-calling turn: null, matching what OpenAI sent.
        assert!(wire["content"].is_null());
        let call = &wire["tool_calls"][0];
        assert_eq!(call["id"], "call_1");
        assert_eq!(call["type"], "function");
        assert_eq!(call["function"]["name"], "get_weather");
        // A string, not an object — this is the part that silently breaks.
        let args = call["function"]["arguments"].as_str().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(args).unwrap(),
            serde_json::json!({"city": "Boston"})
        );
    }

    #[test]
    fn wire_assistant_keeps_text_alongside_tool_calls() {
        let msg = Message::assistant_with_tools(
            "Let me check.",
            vec![ToolCall {
                id: "c".into(),
                name: "t".into(),
                input: serde_json::json!({}),
            }],
        );
        let wire = OpenAiProvider::message_to_wire(&msg).unwrap();
        assert_eq!(wire["content"], "Let me check.");
    }

    #[test]
    fn parallel_tool_calls_round_trip_from_response_to_request() {
        // The property 020.020.040 names: a response with parallel calls,
        // converted to messages and replayed, must serialize to a body the
        // provider accepts — every id present, every result matched to a call.
        let body = serde_json::json!({
            "id": "chatcmpl-3",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {"id": "call_a", "type": "function",
                         "function": {"name": "search", "arguments": "{\"q\":\"rust\"}"}},
                        {"id": "call_b", "type": "function",
                         "function": {"name": "fetch", "arguments": "{\"url\":\"http://x\"}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }]
        });
        let resp = OpenAiProvider::map_response(body).unwrap();
        assert_eq!(resp.tool_calls().len(), 2);

        let mut messages = vec![Message::user("find rust docs")];
        messages.push(resp.to_assistant_message());
        for call in resp.tool_calls() {
            messages.push(Message::tool_result(
                &call.id,
                format!("result of {}", call.name),
            ));
        }

        let req = CompletionRequest::new("gpt-4o", messages);
        let wire = OpenAiProvider::build_body(&req, false).unwrap();
        let msgs = wire["messages"].as_array().unwrap();

        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[1]["tool_calls"].as_array().unwrap().len(), 2);
        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "call_a");
        assert_eq!(msgs[3]["tool_call_id"], "call_b");

        // Every tool result answers a call the assistant message declared —
        // the invariant OpenAI enforces with a 400.
        let declared: Vec<&str> = msgs[1]["tool_calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        for msg in &msgs[2..] {
            assert!(declared.contains(&msg["tool_call_id"].as_str().unwrap()));
        }
    }

    #[test]
    fn parse_chunk_extracts_text_delta() {
        let data = r#"{"choices":[{"index":0,"delta":{"content":"Hello"}}]}"#;
        let chunk = parse_openai_chunk(PROVIDER_NAME, data).unwrap().unwrap();
        assert_eq!(chunk.delta, "Hello");
        assert!(chunk.finish_reason.is_none());
    }

    #[test]
    fn parse_chunk_skips_role_preamble() {
        let data = r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":""}}]}"#;
        assert!(parse_openai_chunk(PROVIDER_NAME, data).unwrap().is_none());
    }

    #[test]
    fn parse_chunk_extracts_finish_reason() {
        let data = r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        let chunk = parse_openai_chunk(PROVIDER_NAME, data).unwrap().unwrap();
        assert_eq!(chunk.finish_reason.as_deref(), Some("stop"));
        assert_eq!(chunk.delta, "");
    }

    #[test]
    fn parse_chunk_extracts_usage_from_choiceless_payload() {
        let data =
            r#"{"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}}"#;
        let chunk = parse_openai_chunk(PROVIDER_NAME, data).unwrap().unwrap();
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 9);
        assert_eq!(usage.total_tokens, 12);
    }

    #[test]
    fn parse_chunk_extracts_tool_call_fragments() {
        let start = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_weather","arguments":""}}]}}]}"#;
        let chunk = parse_openai_chunk(PROVIDER_NAME, start).unwrap().unwrap();
        assert_eq!(chunk.tool_calls.len(), 1);
        assert_eq!(chunk.tool_calls[0].id.as_deref(), Some("call_1"));
        assert_eq!(chunk.tool_calls[0].name.as_deref(), Some("get_weather"));

        let args = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"city\":"}}]}}]}"#;
        let chunk = parse_openai_chunk(PROVIDER_NAME, args).unwrap().unwrap();
        assert_eq!(chunk.tool_calls[0].index, 0);
        assert_eq!(
            chunk.tool_calls[0].arguments.as_deref(),
            Some(r#"{"city":"#)
        );
        assert!(chunk.tool_calls[0].name.is_none());
    }

    #[test]
    fn parse_chunk_keeps_parallel_tool_calls_distinct() {
        let data = r#"{"choices":[{"index":0,"delta":{"tool_calls":[
            {"index":0,"id":"a","function":{"name":"first","arguments":""}},
            {"index":1,"id":"b","function":{"name":"second","arguments":""}}
        ]}}]}"#;
        let chunk = parse_openai_chunk(PROVIDER_NAME, data).unwrap().unwrap();
        assert_eq!(chunk.tool_calls.len(), 2);
        assert_eq!(chunk.tool_calls[0].index, 0);
        assert_eq!(chunk.tool_calls[1].index, 1);
        assert_eq!(chunk.tool_calls[1].name.as_deref(), Some("second"));
    }

    #[test]
    fn parse_chunk_surfaces_mid_stream_error() {
        // No code in the body: an error arriving mid-stream with nothing to
        // classify it is treated as a server-side failure, which is retryable.
        let data = r#"{"error":{"message":"upstream exploded"}}"#;
        let err = parse_openai_chunk(PROVIDER_NAME, data).unwrap_err();
        assert!(
            matches!(err, CosmosError::Server { message: ref m, .. } if m.contains("exploded"))
        );
        assert!(err.is_retryable());
    }

    #[test]
    fn parse_chunk_classifies_mid_stream_errors_by_code() {
        // A stream that dies on a context overflow is not a transport problem,
        // and a caller that trims and retries needs to be told which it is.
        let data =
            r#"{"error":{"message":"too long","code":"context_length_exceeded","status":400}}"#;
        let err = parse_openai_chunk(PROVIDER_NAME, data).unwrap_err();
        assert!(matches!(err, CosmosError::ContextLength { .. }));
        assert!(!err.is_retryable());
    }

    #[test]
    fn parse_chunk_mid_stream_rate_limit_is_retryable() {
        let data = r#"{"error":{"message":"slow down","status":429}}"#;
        let err = parse_openai_chunk(PROVIDER_NAME, data).unwrap_err();
        assert!(matches!(err, CosmosError::RateLimit { .. }));
        assert!(err.is_retryable());
    }

    #[test]
    fn parse_chunk_rejects_malformed_json() {
        let err = parse_openai_chunk(PROVIDER_NAME, "{not json").unwrap_err();
        assert!(matches!(err, CosmosError::Streaming { .. }));
    }

    #[test]
    fn parsed_chunks_accumulate_into_response() {
        use crate::types::StreamAccumulator;

        let payloads = [
            r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":""}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"content":"Hello"}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"content":", world"}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":6}}"#,
        ];

        let mut acc = StreamAccumulator::new();
        for payload in payloads {
            if let Some(chunk) = parse_openai_chunk(PROVIDER_NAME, payload).unwrap() {
                acc.push(&chunk);
            }
        }

        let resp = acc.into_response();
        assert_eq!(resp.text(), "Hello, world");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
        assert_eq!(resp.usage.unwrap().total_tokens, 6);
    }

    #[test]
    fn missing_key_yields_auth_error() {
        // Clear both env vars to ensure no key leaks in.
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("CLLM__OPENAI__API_KEY");
        let p = OpenAiProvider::new(None);
        let err = p.resolved_key().unwrap_err();
        assert!(matches!(err, CosmosError::Authentication { .. }));
    }

    #[test]
    fn map_response_parses_openai_body() {
        let body = serde_json::json!({
            "id": "chatcmpl-1",
            "model": "gpt-4o",
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
        let resp = OpenAiProvider::map_response(body).unwrap();
        assert_eq!(resp.content(), Some("Hello!"));
        assert!(!resp.tool_use());
        assert_eq!(resp.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn map_response_parses_tool_calls() {
        let body = serde_json::json!({
            "id": "chatcmpl-2",
            "model": "gpt-4o",
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
            }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 8, "total_tokens": 20 }
        });
        let resp = OpenAiProvider::map_response(body).unwrap();
        assert!(resp.tool_use());
        let calls = resp.tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].input, serde_json::json!({"city": "Boston"}));
    }
}
