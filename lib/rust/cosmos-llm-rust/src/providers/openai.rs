use std::future::Future;
use std::pin::Pin;

use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::error::CosmosError;
use crate::providers::{stream_from_response, CompletionStream, Provider};
use crate::types::{
    Choice, CompletionRequest, CompletionResponse, Message, StreamChunk, ToolCall, ToolCallDelta,
    Usage,
};

/// Default API root, used unless overridden by
/// [`OpenAiProvider::with_base_url`] or the `OPENAI_BASE_URL` environment
/// variable.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

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
        self.api_key.as_deref().ok_or_else(|| {
            CosmosError::Authentication(
                "OpenAI API key not set. Export OPENAI_API_KEY or CLLM__OPENAI__API_KEY."
                    .to_owned(),
            )
        })
    }

    /// Builds the JSON request body for the chat completions endpoint.
    ///
    /// When `stream` is set, `stream_options.include_usage` is requested too,
    /// so the final chunk carries token counts; OpenAI omits usage from
    /// streamed responses otherwise.
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

    fn handle_error(status: u16, body: &Value) -> CosmosError {
        let msg = body["error"]["message"]
            .as_str()
            .unwrap_or("unknown error")
            .to_owned();
        match status {
            401 => CosmosError::Authentication(msg),
            429 => CosmosError::RateLimit(msg),
            400 | 404 => CosmosError::InvalidRequest(msg),
            s if s >= 500 => CosmosError::Server(msg),
            _ => CosmosError::InvalidResponse(format!("unexpected status {status}: {msg}")),
        }
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
pub(crate) fn parse_openai_chunk(data: &str) -> Result<Option<StreamChunk>, CosmosError> {
    let value: Value = serde_json::from_str(data)
        .map_err(|e| CosmosError::Streaming(format!("malformed stream chunk: {e}")))?;

    // An error can arrive mid-stream, after a 200 response has already
    // committed the connection to streaming.
    if value["error"].is_object() {
        let msg = value["error"]["message"]
            .as_str()
            .unwrap_or("unknown error");
        return Err(CosmosError::Streaming(msg.to_owned()));
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
            let body = Self::build_body(req, false);

            let resp = self
                .http
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(key)
                .json(&body)
                .send()
                .await?;

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

            let resp = self
                .http
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(key)
                .header("accept", "text/event-stream")
                .json(&body)
                .send()
                .await?;

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
            let key = self.resolved_key()?;

            let resp = self
                .http
                .get(format!("{}/models", self.base_url))
                .bearer_auth(key)
                .send()
                .await?;

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

    #[test]
    fn reports_streaming_support() {
        let p = OpenAiProvider::new(None);
        assert!(p.supports_streaming());
    }

    #[test]
    fn build_body_sets_stream_flags_only_when_streaming() {
        let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")]);

        let plain = OpenAiProvider::build_body(&req, false);
        assert!(plain.get("stream").is_none());
        assert!(plain.get("stream_options").is_none());

        let streamed = OpenAiProvider::build_body(&req, true);
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
        let body = OpenAiProvider::build_body(&req, false);
        // Compared as `f32` literals: widening to `f64` is not exact.
        assert_eq!(body["temperature"], serde_json::json!(0.4_f32));
        assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["top_p"], serde_json::json!(0.8_f32));
        assert_eq!(body["stop"], serde_json::json!(["END"]));
    }

    #[test]
    fn parse_chunk_extracts_text_delta() {
        let data = r#"{"choices":[{"index":0,"delta":{"content":"Hello"}}]}"#;
        let chunk = parse_openai_chunk(data).unwrap().unwrap();
        assert_eq!(chunk.delta, "Hello");
        assert!(chunk.finish_reason.is_none());
    }

    #[test]
    fn parse_chunk_skips_role_preamble() {
        let data = r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":""}}]}"#;
        assert!(parse_openai_chunk(data).unwrap().is_none());
    }

    #[test]
    fn parse_chunk_extracts_finish_reason() {
        let data = r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        let chunk = parse_openai_chunk(data).unwrap().unwrap();
        assert_eq!(chunk.finish_reason.as_deref(), Some("stop"));
        assert_eq!(chunk.delta, "");
    }

    #[test]
    fn parse_chunk_extracts_usage_from_choiceless_payload() {
        let data =
            r#"{"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}}"#;
        let chunk = parse_openai_chunk(data).unwrap().unwrap();
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 9);
        assert_eq!(usage.total_tokens, 12);
    }

    #[test]
    fn parse_chunk_extracts_tool_call_fragments() {
        let start = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_weather","arguments":""}}]}}]}"#;
        let chunk = parse_openai_chunk(start).unwrap().unwrap();
        assert_eq!(chunk.tool_calls.len(), 1);
        assert_eq!(chunk.tool_calls[0].id.as_deref(), Some("call_1"));
        assert_eq!(chunk.tool_calls[0].name.as_deref(), Some("get_weather"));

        let args = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"city\":"}}]}}]}"#;
        let chunk = parse_openai_chunk(args).unwrap().unwrap();
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
        let chunk = parse_openai_chunk(data).unwrap().unwrap();
        assert_eq!(chunk.tool_calls.len(), 2);
        assert_eq!(chunk.tool_calls[0].index, 0);
        assert_eq!(chunk.tool_calls[1].index, 1);
        assert_eq!(chunk.tool_calls[1].name.as_deref(), Some("second"));
    }

    #[test]
    fn parse_chunk_surfaces_mid_stream_error() {
        let data = r#"{"error":{"message":"context length exceeded"}}"#;
        let err = parse_openai_chunk(data).unwrap_err();
        assert!(matches!(err, CosmosError::Streaming(ref m) if m.contains("context length")));
    }

    #[test]
    fn parse_chunk_rejects_malformed_json() {
        let err = parse_openai_chunk("{not json").unwrap_err();
        assert!(matches!(err, CosmosError::Streaming(_)));
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
            if let Some(chunk) = parse_openai_chunk(payload).unwrap() {
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
        assert!(matches!(err, CosmosError::Authentication(_)));
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
