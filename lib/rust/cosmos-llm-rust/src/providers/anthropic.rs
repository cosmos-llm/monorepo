use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::content::{ContentBlock, ImageSource, Role};
use crate::error::CosmosError;
use crate::providers::{
    map_anthropic_error, retry_after_from_headers, stream_from_response, transport_error,
    CompletionStream, Provider,
};
use crate::reasoning::ThinkingDelta;
use crate::types::{
    Choice, CompletionRequest, CompletionResponse, Message, StreamChunk, ToolCall, ToolCallDelta,
    Usage,
};

/// Default API root, used unless overridden by
/// [`AnthropicProvider::with_base_url`] or the `ANTHROPIC_BASE_URL`
/// environment variable.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Name this provider reports from [`Provider::name`] and attaches to errors.
const PROVIDER_NAME: &str = "anthropic";

/// Static list of known Claude models, returned by [`AnthropicProvider::models`].
///
/// Anthropic's models endpoint requires a paid account; this list is used as
/// a fallback so callers can enumerate models without an active subscription.
const KNOWN_MODELS: &[&str] = &[
    "claude-opus-4-8",
    "claude-sonnet-4-6",
    "claude-haiku-4-5-20251001",
    "claude-3-5-sonnet-20241022",
    "claude-3-5-haiku-20241022",
    "claude-3-opus-20240229",
    "claude-3-haiku-20240307",
];

/// Provider implementation for the Anthropic Messages API.
///
/// Supports chat completions (with optional system message) and model listing.
/// Reads the API key from the `ANTHROPIC_API_KEY` or
/// `CLLM__ANTHROPIC__API_KEY` environment variable when none is supplied at
/// construction.
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::providers::anthropic::AnthropicProvider;
/// use cosmos_llm::providers::Provider;
/// use cosmos_llm::types::{CompletionRequest, Message};
///
/// # tokio_test::block_on(async {
/// let provider = AnthropicProvider::new(Some("sk-ant-test".into()));
/// let req = CompletionRequest::new(
///     "claude-3-5-sonnet-20241022",
///     vec![Message::user("Hello!")],
/// );
/// // let resp = provider.completion(&req).await.unwrap();
/// # })
/// ```
pub struct AnthropicProvider {
    api_key: Option<String>,
    base_url: String,
    http: HttpClient,
}

impl AnthropicProvider {
    /// Creates a new [`AnthropicProvider`].
    ///
    /// When `api_key` is `None`, falls back to `ANTHROPIC_API_KEY` or
    /// `CLLM__ANTHROPIC__API_KEY` environment variables. The API root defaults
    /// to [`DEFAULT_BASE_URL`] unless `ANTHROPIC_BASE_URL` or
    /// `CLLM__ANTHROPIC__BASE_URL` is set.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::anthropic::AnthropicProvider;
    /// let provider = AnthropicProvider::new(None);
    /// ```
    pub fn new(api_key: Option<String>) -> Self {
        let key = api_key
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .or_else(|| std::env::var("CLLM__ANTHROPIC__API_KEY").ok());
        Self {
            api_key: key,
            base_url: std::env::var("ANTHROPIC_BASE_URL")
                .or_else(|_| std::env::var("CLLM__ANTHROPIC__BASE_URL"))
                .unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned()),
            http: HttpClient::new(),
        }
    }

    /// Points the provider at a different API root (builder pattern).
    ///
    /// Useful for Anthropic-compatible gateways and proxies, and for pointing
    /// tests at a mock server. Any trailing slash is trimmed.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::anthropic::AnthropicProvider;
    ///
    /// let provider = AnthropicProvider::new(Some("sk-ant-test".into()))
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
    /// use cosmos_llm::providers::anthropic::{AnthropicProvider, DEFAULT_BASE_URL};
    ///
    /// # std::env::remove_var("ANTHROPIC_BASE_URL");
    /// # std::env::remove_var("CLLM__ANTHROPIC__BASE_URL");
    /// let provider = AnthropicProvider::new(Some("sk-ant-test".into()));
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
                message: "Anthropic API key not set. Export ANTHROPIC_API_KEY or \
                          CLLM__ANTHROPIC__API_KEY."
                    .to_owned(),
            })
    }

    /// Splits messages into an optional system prompt and the remaining chat
    /// messages.
    ///
    /// Anthropic's API separates the system prompt from the message array. The
    /// prompt is a plain string, unless a system message carries a cache hint,
    /// in which case it is an array of text blocks so `cache_control` has
    /// somewhere to go. Several system messages are joined in order.
    fn split_system(messages: &[Message]) -> (Option<Value>, Vec<&Message>) {
        let mut blocks: Vec<Value> = Vec::new();
        let mut cached = false;
        let mut chat = Vec::with_capacity(messages.len());
        for msg in messages {
            if msg.role != Role::System {
                chat.push(msg);
                continue;
            }
            let text = msg.text();
            if text.is_empty() {
                continue;
            }
            let mut block = json!({ "type": "text", "text": text });
            if let Some(hint) = msg.cache {
                block["cache_control"] = hint.to_cache_control();
                cached = true;
            }
            blocks.push(block);
        }

        let system = if blocks.is_empty() {
            None
        } else if cached {
            Some(Value::Array(blocks))
        } else {
            let joined: Vec<&str> = blocks.iter().filter_map(|b| b["text"].as_str()).collect();
            Some(json!(joined.join("\n\n")))
        };
        (system, chat)
    }

    /// Translates one content block to its wire form.
    ///
    /// Returns `None` for a block Anthropic cannot take: empty text, and
    /// thinking without a signature (it can only be replayed signed).
    fn block_to_wire(block: &ContentBlock) -> Option<Value> {
        Some(match block {
            ContentBlock::Text { text } if text.is_empty() => return None,
            ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
            ContentBlock::Image { source } => match source {
                ImageSource::Base64 { media_type, data } => json!({
                    "type": "image",
                    "source": { "type": "base64", "media_type": media_type, "data": data },
                }),
                ImageSource::Url { url } => json!({
                    "type": "image",
                    "source": { "type": "url", "url": url },
                }),
            },
            ContentBlock::Thinking { text, signature } => json!({
                "type": "thinking",
                "thinking": text,
                "signature": signature.as_deref()?,
            }),
            ContentBlock::RedactedThinking { data } => {
                json!({ "type": "redacted_thinking", "data": data })
            }
            ContentBlock::ToolUse { id, name, input } => json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                // Anthropic takes arguments as a native object, unlike
                // OpenAI's JSON string.
                "input": input,
            }),
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let mut wire = json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": Self::tool_result_content(content),
                });
                if *is_error {
                    wire["is_error"] = json!(true);
                }
                wire
            }
        })
    }

    /// A tool result's `content`: a string when it is all text, which is what
    /// nearly every result is, else an array of blocks.
    fn tool_result_content(content: &[ContentBlock]) -> Value {
        if content
            .iter()
            .all(|b| matches!(b, ContentBlock::Text { .. }))
        {
            let text: String = content.iter().filter_map(ContentBlock::as_text).collect();
            return json!(text);
        }
        Value::Array(content.iter().filter_map(Self::block_to_wire).collect())
    }

    /// Translates messages into Anthropic's content-block format.
    ///
    /// Anthropic differs from OpenAI in two ways that matter here:
    ///
    /// * A tool result is a `tool_result` **content block** inside a `user`
    ///   message, keyed by `tool_use_id` — there is no `"tool"` role.
    /// * Parallel results must share one user message. Anthropic rejects two
    ///   consecutive `user` messages, so a turn answering three calls sends one
    ///   message holding three blocks. Consecutive tool results are therefore
    ///   merged.
    ///
    /// A message's cache hint becomes `cache_control` on its last block that
    /// can carry one (thinking blocks cannot).
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::InvalidRequest`] for a tool-role message with no
    /// `tool_result` block, since the result cannot be attributed to a call.
    pub(crate) fn messages_to_wire(messages: &[&Message]) -> Result<Vec<Value>, CosmosError> {
        let mut wire: Vec<Value> = Vec::with_capacity(messages.len());

        for msg in messages {
            if msg.role == Role::Tool && msg.tool_result_id().is_none() {
                return Err(CosmosError::InvalidRequest {
                    provider: PROVIDER_NAME.to_owned(),
                    message: "tool message has no tool_result block; Anthropic needs a \
                              tool_use_id. Build it with Message::tool_result."
                        .to_owned(),
                    param: Some("messages[].content[].tool_use_id".to_owned()),
                });
            }

            let mut blocks: Vec<Value> =
                msg.content.iter().filter_map(Self::block_to_wire).collect();
            if let Some(hint) = msg.cache {
                // Thinking blocks cannot carry a breakpoint; use the last
                // block that can.
                let target = blocks
                    .iter_mut()
                    .rev()
                    .find(|b| b["type"] != "thinking" && b["type"] != "redacted_thinking");
                if let Some(block) = target {
                    block["cache_control"] = hint.to_cache_control();
                }
            }

            if msg.is_tool_result() {
                // Fold into the preceding user message when there is one, so
                // parallel results arrive as one turn.
                match wire.last_mut() {
                    Some(prev) if prev["role"] == "user" && prev["content"].is_array() => {
                        prev["content"]
                            .as_array_mut()
                            .expect("checked is_array")
                            .extend(blocks);
                    }
                    _ => wire.push(json!({ "role": "user", "content": blocks })),
                }
                continue;
            }

            // A lone uncached text block goes as a plain string.
            if let [block] = blocks.as_slice() {
                if block["type"] == "text" && block.get("cache_control").is_none() {
                    wire.push(json!({ "role": msg.role, "content": block["text"] }));
                    continue;
                }
            }
            wire.push(json!({ "role": msg.role, "content": blocks }));
        }

        Ok(wire)
    }

    /// Trims cache hints to Anthropic's limit of four breakpoints per request.
    ///
    /// The tool list counts as one. Hints on the earliest chat messages go
    /// first, since the newest breakpoints are the ones the next turn reads;
    /// system hints go last of all.
    fn limit_cache_hints(req: &CompletionRequest) -> std::borrow::Cow<'_, [Message]> {
        const MAX: usize = 4;
        let used = usize::from(req.cache_tools.is_some() && req.tools.is_some());
        let allowed = MAX.saturating_sub(used);
        let total = req.messages.iter().filter(|m| m.cache.is_some()).count();
        if total <= allowed {
            return std::borrow::Cow::Borrowed(&req.messages);
        }

        let mut msgs = req.messages.clone();
        let mut excess = total - allowed;
        for system_pass in [false, true] {
            for msg in msgs
                .iter_mut()
                .filter(|m| (m.role == Role::System) == system_pass)
            {
                if excess > 0 && msg.cache.take().is_some() {
                    excess -= 1;
                }
            }
        }
        std::borrow::Cow::Owned(msgs)
    }

    /// Builds the JSON request body for the messages endpoint.
    ///
    /// Anthropic requires `max_tokens`, so a request that omits it gets a
    /// 1024-token default.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::InvalidRequest`] if a message cannot be
    /// represented — see [`AnthropicProvider::messages_to_wire`].
    fn build_body(req: &CompletionRequest, stream: bool) -> Result<Value, CosmosError> {
        let all = Self::limit_cache_hints(req);
        let (system, chat_msgs) = Self::split_system(&all);
        let messages = Self::messages_to_wire(&chat_msgs)?;

        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": req.max_tokens.unwrap_or(1024),
        });

        if let Some(sys) = system {
            body["system"] = sys;
        }
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(p) = req.top_p {
            body["top_p"] = json!(p);
        }
        if let Some(ref stop) = req.stop {
            body["stop_sequences"] = json!(stop);
        }
        if let Some(ref tools) = req.tools {
            let mut tools = tools.clone();
            if let (Some(hint), Some(last)) = (req.cache_tools, tools.last_mut()) {
                last["cache_control"] = hint.to_cache_control();
            }
            body["tools"] = json!(tools);
        }
        if let Some(ref choice) = req.tool_choice {
            body["tool_choice"] = json!(choice);
        }
        if stream {
            body["stream"] = json!(true);
        }

        // Extended thinking. Anthropic wants `max_tokens` above the budget, and
        // rejects `temperature` and `top_p` while thinking is on, so the first
        // is raised and the others dropped rather than failing the request.
        if let Some(budget) = req.reasoning.and_then(|r| r.anthropic_budget()) {
            body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
            let floor = u64::from(budget) + 1024;
            if body["max_tokens"].as_u64().unwrap_or(0) < floor {
                body["max_tokens"] = json!(floor);
            }
            if let Some(map) = body.as_object_mut() {
                map.remove("temperature");
                map.remove("top_p");
            }
        }

        // Caller-supplied top-level fields, last so they win. This carries
        // what the crate has no typed builder for yet: `output_config`
        // (effort, structured output), `thinking`, server tools' settings.
        for (key, value) in &req.extra {
            body[key.as_str()] = value.clone();
        }

        Ok(body)
    }

    fn map_response(body: Value) -> Result<CompletionResponse, CosmosError> {
        let id = body["id"].as_str().map(str::to_owned);
        let model = body["model"].as_str().map(str::to_owned);

        // Anthropic returns `content` as an array of blocks. Keep every one,
        // in order: the thinking blocks have to be replayed to Anthropic as
        // they came.
        let mut blocks = Vec::new();
        for b in body["content"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let block = match b["type"].as_str().unwrap_or("") {
                "text" => ContentBlock::text(b["text"].as_str().unwrap_or("")),
                "thinking" => ContentBlock::Thinking {
                    text: b["thinking"].as_str().unwrap_or("").to_owned(),
                    signature: b["signature"].as_str().map(str::to_owned),
                },
                "redacted_thinking" => ContentBlock::RedactedThinking {
                    data: b["data"].as_str().unwrap_or("").to_owned(),
                },
                "tool_use" => ContentBlock::ToolUse {
                    id: b["id"].as_str().unwrap_or_default().to_owned(),
                    name: b["name"].as_str().unwrap_or_default().to_owned(),
                    input: b["input"].clone(),
                },
                // Server-tool blocks and anything newer have no model here.
                _ => continue,
            };
            blocks.push(block);
        }

        let finish_reason = body["stop_reason"].as_str().map(str::to_owned);

        let tool_calls = blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                }),
                _ => None,
            })
            .collect();

        let usage = if body["usage"].is_object() {
            let mut usage = Self::usage_at_start(&body["usage"]);
            let out = body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;
            usage.completion_tokens = out;
            usage.total_tokens = usage.prompt_tokens.saturating_add(out);
            Some(usage)
        } else {
            None
        };

        Ok(CompletionResponse {
            id,
            model,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant_blocks(blocks),
                finish_reason,
                tool_calls,
            }],
            usage,
            route: None,
        })
    }

    /// Reads the input side of an Anthropic `usage` object.
    ///
    /// Anthropic's `input_tokens` excludes cache reads and writes; they are
    /// added back so `prompt_tokens` counts every token the model saw, the
    /// meaning it has for the other providers. Output counts are left at zero.
    fn usage_at_start(usage: &Value) -> Usage {
        let count = |key: &str| usage[key].as_u64().map(|n| n as u32);
        let read = count("cache_read_input_tokens");
        let write = count("cache_creation_input_tokens");
        let prompt = count("input_tokens")
            .unwrap_or(0)
            .saturating_add(read.unwrap_or(0))
            .saturating_add(write.unwrap_or(0));
        let mut out = Usage::from_tokens(prompt, 0);
        out.cache_read_tokens = read;
        out.cache_write_tokens = write;
        out
    }

    /// Classifies an error response, honouring `Retry-After` when present.
    ///
    /// Delegates to [`map_anthropic_error`], which reads `error.type` — the
    /// only place Anthropic distinguishes an overload from a bad request.
    fn handle_error(status: u16, body: &Value, retry_after: Option<Duration>) -> CosmosError {
        map_anthropic_error(status, body, retry_after)
    }

    /// Attributes a transport failure to this provider.
    fn network_error(source: reqwest::Error) -> CosmosError {
        transport_error(PROVIDER_NAME, source)
    }
}

/// Tracks cross-event state while decoding an Anthropic message stream.
///
/// Anthropic streams content as indexed blocks: `content_block_start`
/// announces a block's type (and, for `tool_use`, its id and name), and the
/// `content_block_delta` events that follow identify their block only by
/// index. Reassembling a tool call therefore requires remembering what each
/// index refers to.
///
/// Block indices are also not tool-call indices — a message whose first block
/// is text and second is a tool call has its only tool call at block index 1.
/// [`StreamState::tool_slots`] maps block index to a dense tool-call index so
/// [`StreamAccumulator`](crate::StreamAccumulator) sees the same numbering
/// OpenAI produces.
#[derive(Debug, Default)]
struct StreamState {
    /// Block index → tool-call index, for blocks of type `tool_use`.
    tool_slots: Vec<(u64, u32)>,
    /// Input-side usage from `message_start`, held until the final
    /// `message_delta` reports output tokens.
    start_usage: Option<Usage>,
}

impl StreamState {
    /// Registers a `tool_use` block and returns its tool-call index.
    fn register_tool(&mut self, block_index: u64) -> u32 {
        if let Some((_, slot)) = self.tool_slots.iter().find(|(b, _)| *b == block_index) {
            return *slot;
        }
        let slot = self.tool_slots.len() as u32;
        self.tool_slots.push((block_index, slot));
        slot
    }

    /// Returns the tool-call index for a block, if it is a `tool_use` block.
    fn tool_slot(&self, block_index: u64) -> Option<u32> {
        self.tool_slots
            .iter()
            .find(|(b, _)| *b == block_index)
            .map(|(_, slot)| *slot)
    }
}

/// Parses one Anthropic stream event into a [`StreamChunk`].
///
/// `state` carries the block bookkeeping across events; a single state must be
/// used for the whole stream. Returns `Ok(None)` for events that carry nothing
/// this crate models (`ping`, `content_block_stop`, `message_stop`).
///
/// Anthropic reports errors as an `error` event on an already-successful
/// response, which this maps to [`CosmosError::Streaming`].
fn parse_anthropic_event(
    state: &mut StreamState,
    data: &str,
) -> Result<Option<StreamChunk>, CosmosError> {
    let value: Value = serde_json::from_str(data).map_err(|e| CosmosError::Streaming {
        provider: PROVIDER_NAME.to_owned(),
        message: format!("malformed stream event: {e}"),
    })?;

    let block_index = value["index"].as_u64().unwrap_or(0);

    let chunk = match value["type"].as_str().unwrap_or("") {
        "message_start" => {
            state.start_usage = value["message"]["usage"]["input_tokens"]
                .is_u64()
                .then(|| AnthropicProvider::usage_at_start(&value["message"]["usage"]));
            return Ok(None);
        }

        "content_block_start" => {
            let block = &value["content_block"];
            // Thinking opens a block so the accumulator keeps its position
            // even if no text follows.
            match block["type"].as_str().unwrap_or("") {
                "thinking" => {
                    return Ok(Some(StreamChunk {
                        thinking: Some(ThinkingDelta::Text {
                            index: block_index as u32,
                            text: block["thinking"].as_str().unwrap_or("").to_owned(),
                        }),
                        ..Default::default()
                    }))
                }
                "redacted_thinking" => {
                    return Ok(Some(StreamChunk {
                        thinking: Some(ThinkingDelta::Redacted {
                            index: block_index as u32,
                            data: block["data"].as_str().unwrap_or("").to_owned(),
                        }),
                        ..Default::default()
                    }))
                }
                "tool_use" => {}
                _ => return Ok(None),
            }
            let slot = state.register_tool(block_index);
            StreamChunk {
                tool_calls: vec![ToolCallDelta {
                    index: slot,
                    id: block["id"].as_str().map(str::to_owned),
                    name: block["name"].as_str().map(str::to_owned),
                    arguments: None,
                }],
                ..Default::default()
            }
        }

        "content_block_delta" => {
            let delta = &value["delta"];
            match delta["type"].as_str().unwrap_or("") {
                "text_delta" => StreamChunk::text(delta["text"].as_str().unwrap_or("")),
                "input_json_delta" => {
                    // A delta for a block we never saw start cannot be
                    // attributed to a tool call; drop it rather than guess.
                    let Some(slot) = state.tool_slot(block_index) else {
                        return Ok(None);
                    };
                    StreamChunk {
                        tool_calls: vec![ToolCallDelta {
                            index: slot,
                            arguments: Some(
                                delta["partial_json"].as_str().unwrap_or("").to_owned(),
                            ),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }
                }
                "thinking_delta" => StreamChunk {
                    thinking: Some(ThinkingDelta::Text {
                        index: block_index as u32,
                        text: delta["thinking"].as_str().unwrap_or("").to_owned(),
                    }),
                    ..Default::default()
                },
                "signature_delta" => StreamChunk {
                    thinking: Some(ThinkingDelta::Signature {
                        index: block_index as u32,
                        signature: delta["signature"].as_str().unwrap_or("").to_owned(),
                    }),
                    ..Default::default()
                },
                // Citations and other delta types carry nothing this crate
                // surfaces yet.
                _ => return Ok(None),
            }
        }

        "message_delta" => {
            let output_tokens = value["usage"]["output_tokens"].as_u64().map(|n| n as u32);
            let usage = output_tokens.map(|completion_tokens| {
                let mut usage = state.start_usage.clone().unwrap_or_default();
                usage.completion_tokens = completion_tokens;
                usage.total_tokens = usage.prompt_tokens.saturating_add(completion_tokens);
                usage
            });
            StreamChunk {
                finish_reason: value["delta"]["stop_reason"].as_str().map(str::to_owned),
                usage,
                ..Default::default()
            }
        }

        "error" => {
            // Anthropic reports overloads as a mid-stream `overloaded_error`,
            // which is retryable. Classifying through the same mapper as a
            // pre-stream error is what surfaces that; collapsing it to
            // `Streaming` would make every mid-stream failure look permanent.
            // No status arrives with the event, so 500 stands in for the
            // server-side failure these events almost always describe.
            return Err(map_anthropic_error(500, &value, None));
        }

        // "ping", "content_block_stop", "message_stop", and any future event
        // type carry nothing to emit.
        _ => return Ok(None),
    };

    Ok(if chunk.is_empty() { None } else { Some(chunk) })
}

impl Provider for AnthropicProvider {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            let key = self.resolved_key()?;
            let body = Self::build_body(req, false)?;

            let resp = self
                .http
                .post(format!("{}/messages", self.base_url))
                .header("x-api-key", key)
                .header("anthropic-version", ANTHROPIC_VERSION)
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
                .post(format!("{}/messages", self.base_url))
                .header("x-api-key", key)
                .header("anthropic-version", ANTHROPIC_VERSION)
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

            let mut state = StreamState::default();
            Ok(stream_from_response(PROVIDER_NAME, resp, move |data| {
                parse_anthropic_event(&mut state, data)
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
        Box::pin(async move { Ok(KNOWN_MODELS.iter().map(|s| s.to_string()).collect()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_system_separates_messages() {
        let msgs = vec![
            Message::system("be helpful"),
            Message::user("hi"),
            Message::assistant("hello"),
        ];
        let (sys, chat) = AnthropicProvider::split_system(&msgs);
        assert_eq!(sys, Some(json!("be helpful")));
        assert_eq!(chat.len(), 2);
    }

    #[test]
    fn split_system_no_system() {
        let msgs = vec![Message::user("hi")];
        let (sys, chat) = AnthropicProvider::split_system(&msgs);
        assert!(sys.is_none());
        assert_eq!(chat.len(), 1);
    }

    #[test]
    fn missing_key_yields_auth_error() {
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("CLLM__ANTHROPIC__API_KEY");
        let p = AnthropicProvider::new(None);
        assert!(matches!(
            p.resolved_key(),
            Err(CosmosError::Authentication { .. })
        ));
    }

    #[test]
    fn map_response_parses_anthropic_body() {
        let body = serde_json::json!({
            "id": "msg_1",
            "model": "claude-3-5-sonnet-20241022",
            "content": [{ "type": "text", "text": "Hi there!" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 8, "output_tokens": 4 }
        });
        let resp = AnthropicProvider::map_response(body).unwrap();
        assert_eq!(resp.content().as_deref(), Some("Hi there!"));
        assert!(!resp.tool_use());
        assert_eq!(resp.usage.unwrap().total_tokens, 12);
    }

    #[test]
    fn map_response_parses_tool_use_blocks() {
        let body = serde_json::json!({
            "id": "msg_2",
            "model": "claude-3-5-sonnet-20241022",
            "content": [
                { "type": "text", "text": "Let me check that." },
                { "type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": { "city": "Boston" } }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "output_tokens": 6 }
        });
        let resp = AnthropicProvider::map_response(body).unwrap();
        assert!(resp.tool_use());
        assert_eq!(resp.text(), "Let me check that.");
        let calls = resp.tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "toolu_1");
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].input, serde_json::json!({"city": "Boston"}));
    }

    #[tokio::test]
    async fn models_returns_known_list() {
        let p = AnthropicProvider::new(Some("key".into()));
        let list = p.models().await.unwrap();
        assert!(list.contains(&"claude-sonnet-4-6".to_string()));
    }

    #[test]
    fn reports_streaming_support() {
        assert!(AnthropicProvider::new(Some("key".into())).supports_streaming());
    }

    #[test]
    fn build_body_sets_stream_flag_and_defaults_max_tokens() {
        let req = CompletionRequest::new("claude-3-5-sonnet-20241022", vec![Message::user("hi")]);

        let plain = AnthropicProvider::build_body(&req, false).unwrap();
        assert!(plain.get("stream").is_none());
        assert_eq!(plain["max_tokens"], 1024);

        let streamed = AnthropicProvider::build_body(&req, true).unwrap();
        assert_eq!(streamed["stream"], serde_json::json!(true));
    }

    #[test]
    fn extra_fields_are_merged_into_the_body() {
        let req = CompletionRequest::new("claude-opus-5-5", vec![Message::user("hi")])
            .with_extra("output_config", json!({ "effort": "high" }))
            .with_extra("max_tokens", json!(4096));
        let body = AnthropicProvider::build_body(&req, false).unwrap();
        assert_eq!(body["output_config"], json!({ "effort": "high" }));
        // A field the provider builds itself can be overridden.
        assert_eq!(body["max_tokens"], 4096);
    }

    #[test]
    fn build_body_hoists_system_message() {
        let req = CompletionRequest::new(
            "claude-3-5-sonnet-20241022",
            vec![Message::system("be brief"), Message::user("hi")],
        );
        let body = AnthropicProvider::build_body(&req, false).unwrap();
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn wire_plain_message_stays_a_string() {
        // Anthropic accepts a plain string for content; keep it, since the
        // block form is only needed when there is something to block up.
        let wire = AnthropicProvider::messages_to_wire(&[&Message::user("hi")]).unwrap();
        assert_eq!(wire[0], json!({"role": "user", "content": "hi"}));
    }

    #[test]
    fn wire_tool_result_becomes_user_tool_result_block() {
        let msg = Message::tool_result("toolu_1", "72F");
        let wire = AnthropicProvider::messages_to_wire(&[&msg]).unwrap();
        assert_eq!(
            wire[0],
            json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "toolu_1",
                    "content": "72F",
                }]
            })
        );
    }

    #[test]
    fn wire_merges_parallel_tool_results_into_one_turn() {
        // Anthropic rejects consecutive user messages, so three results must
        // arrive as three blocks in one message.
        let a = Message::tool_result("toolu_a", "first");
        let b = Message::tool_result("toolu_b", "second");
        let c = Message::tool_result("toolu_c", "third");
        let wire = AnthropicProvider::messages_to_wire(&[&a, &b, &c]).unwrap();

        assert_eq!(wire.len(), 1);
        let blocks = wire[0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0]["tool_use_id"], "toolu_a");
        assert_eq!(blocks[2]["tool_use_id"], "toolu_c");
    }

    #[test]
    fn wire_does_not_merge_results_into_a_plain_user_message() {
        // The preceding user message carries a bare string, not blocks;
        // appending to it would need it converted, and conflating a question
        // with an unrelated tool result is the wrong shape anyway.
        let user = Message::user("what is the weather");
        let result = Message::tool_result("toolu_a", "72F");
        let wire = AnthropicProvider::messages_to_wire(&[&user, &result]).unwrap();

        assert_eq!(wire.len(), 2);
        assert_eq!(wire[0]["content"], "what is the weather");
        assert!(wire[1]["content"].is_array());
    }

    #[test]
    fn wire_tool_result_without_id_fails_locally() {
        let msg = Message::new(Role::Tool, "72F");
        let err = AnthropicProvider::messages_to_wire(&[&msg]).unwrap_err();
        assert!(
            matches!(err, CosmosError::InvalidRequest { message: ref m, .. } if m.contains("tool_use_id"))
        );
    }

    #[test]
    fn wire_assistant_emits_text_then_tool_use_blocks() {
        let msg = Message::assistant_with_tools(
            "Let me check.",
            vec![ToolCall {
                id: "toolu_1".into(),
                name: "get_weather".into(),
                input: json!({"city": "Boston"}),
            }],
        );
        let wire = AnthropicProvider::messages_to_wire(&[&msg]).unwrap();
        let blocks = wire[0]["content"].as_array().unwrap();

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], json!({"type": "text", "text": "Let me check."}));
        assert_eq!(
            blocks[1],
            json!({
                "type": "tool_use",
                "id": "toolu_1",
                "name": "get_weather",
                // A native object, not OpenAI's JSON string.
                "input": {"city": "Boston"},
            })
        );
    }

    #[test]
    fn wire_assistant_omits_empty_text_block() {
        let msg = Message::assistant_with_tools(
            "",
            vec![ToolCall {
                id: "toolu_1".into(),
                name: "t".into(),
                input: json!({}),
            }],
        );
        let wire = AnthropicProvider::messages_to_wire(&[&msg]).unwrap();
        let blocks = wire[0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "tool_use");
    }

    #[test]
    fn parallel_tool_calls_round_trip_from_response_to_request() {
        // Response → messages → request, checked against the shape a real
        // Anthropic tool-use turn takes.
        let body = json!({
            "id": "msg_3",
            "model": "claude-3-5-sonnet-20241022",
            "content": [
                {"type": "text", "text": "Checking both."},
                {"type": "tool_use", "id": "toolu_a", "name": "search", "input": {"q": "rust"}},
                {"type": "tool_use", "id": "toolu_b", "name": "fetch", "input": {"url": "http://x"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 6}
        });
        let resp = AnthropicProvider::map_response(body).unwrap();
        assert_eq!(resp.tool_calls().len(), 2);

        let mut messages = vec![Message::user("find rust docs")];
        messages.push(resp.to_assistant_message());
        for call in resp.tool_calls() {
            messages.push(Message::tool_result(
                &call.id,
                format!("result of {}", call.name),
            ));
        }

        let req = CompletionRequest::new("claude-3-5-sonnet-20241022", messages);
        let wire = AnthropicProvider::build_body(&req, false).unwrap();
        let msgs = wire["messages"].as_array().unwrap();

        // user, assistant(text + 2 tool_use), user(2 tool_result)
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"].as_array().unwrap().len(), 3);
        assert_eq!(msgs[2]["role"], "user");

        let results = msgs[2]["content"].as_array().unwrap();
        assert_eq!(results.len(), 2);

        // Every tool_use_id refers to a tool_use block in the assistant turn.
        let declared: Vec<&str> = msgs[1]["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| b["type"] == "tool_use")
            .map(|b| b["id"].as_str().unwrap())
            .collect();
        for block in results {
            assert_eq!(block["type"], "tool_result");
            assert!(declared.contains(&block["tool_use_id"].as_str().unwrap()));
        }
    }

    #[test]
    fn stream_parses_text_deltas() {
        let mut state = StreamState::default();
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let chunk = parse_anthropic_event(&mut state, data).unwrap().unwrap();
        assert_eq!(chunk.delta, "Hello");
    }

    #[test]
    fn stream_skips_ping_and_stop_events() {
        let mut state = StreamState::default();
        for data in [
            r#"{"type":"ping"}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"message_stop"}"#,
        ] {
            assert!(parse_anthropic_event(&mut state, data).unwrap().is_none());
        }
    }

    #[test]
    fn stream_ignores_text_block_start() {
        let mut state = StreamState::default();
        let data =
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
        assert!(parse_anthropic_event(&mut state, data).unwrap().is_none());
    }

    #[test]
    fn stream_reassembles_tool_call_across_events() {
        use crate::types::StreamAccumulator;

        let events = [
            r#"{"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":10}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Checking."}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"get_weather","input":{}}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"Boston\"}"}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":5}}"#,
            r#"{"type":"message_stop"}"#,
        ];

        let mut state = StreamState::default();
        let mut acc = StreamAccumulator::new();
        for data in events {
            if let Some(chunk) = parse_anthropic_event(&mut state, data).unwrap() {
                acc.push(&chunk);
            }
        }

        let resp = acc.into_response();
        assert_eq!(resp.text(), "Checking.");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_use"));
        assert!(resp.tool_use());

        let calls = resp.tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "toolu_1");
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].input, serde_json::json!({"city": "Boston"}));

        // input_tokens come from message_start, output_tokens from
        // message_delta; the accumulator must see them combined.
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn stream_maps_block_indices_to_dense_tool_indices() {
        // Blocks 1 and 2 are tool calls; block 0 is text. The emitted tool
        // indices must be 0 and 1, matching OpenAI's numbering.
        let mut state = StreamState::default();

        let first = parse_anthropic_event(
            &mut state,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"a","name":"first"}}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(first.tool_calls[0].index, 0);

        let second = parse_anthropic_event(
            &mut state,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"b","name":"second"}}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(second.tool_calls[0].index, 1);

        // A delta for block 2 must route to tool index 1, not 2.
        let delta = parse_anthropic_event(
            &mut state,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(delta.tool_calls[0].index, 1);
    }

    #[test]
    fn stream_drops_json_delta_for_unknown_block() {
        let mut state = StreamState::default();
        let data = r#"{"type":"content_block_delta","index":7,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#;
        assert!(parse_anthropic_event(&mut state, data).unwrap().is_none());
    }

    #[test]
    fn stream_ignores_unmodelled_delta_types() {
        let mut state = StreamState::default();
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"citations_delta","citation":{}}}"#;
        assert!(parse_anthropic_event(&mut state, data).unwrap().is_none());
    }

    #[test]
    fn stream_surfaces_error_event() {
        // An `overloaded_error` mid-stream is the single most common Anthropic
        // streaming failure, and it is retryable. Reporting it as a generic
        // streaming error would make a retry layer give up on it.
        let mut state = StreamState::default();
        let data = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        let err = parse_anthropic_event(&mut state, data).unwrap_err();
        assert!(matches!(err, CosmosError::Server { message: ref m, .. } if m == "Overloaded"));
        assert!(err.is_retryable());
        assert_eq!(err.provider(), PROVIDER_NAME);
    }

    #[test]
    fn stream_error_event_for_a_bad_request_is_not_retryable() {
        let mut state = StreamState::default();
        let data =
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad input"}}"#;
        let err = parse_anthropic_event(&mut state, data).unwrap_err();
        assert!(matches!(err, CosmosError::InvalidRequest { .. }));
        assert!(!err.is_retryable());
    }

    #[test]
    fn stream_rejects_malformed_json() {
        let mut state = StreamState::default();
        let err = parse_anthropic_event(&mut state, "{oops").unwrap_err();
        assert!(matches!(err, CosmosError::Streaming { .. }));
    }
}

#[cfg(test)]
mod block_tests {
    use super::*;
    use crate::content::CacheHint;
    use crate::reasoning::{Reasoning, ReasoningEffort};
    use crate::types::StreamAccumulator;

    fn wire(msgs: &[Message]) -> Vec<Value> {
        let refs: Vec<&Message> = msgs.iter().collect();
        AnthropicProvider::messages_to_wire(&refs).unwrap()
    }

    #[test]
    fn every_block_type_lowers_to_its_wire_shape() {
        let assistant = Message::assistant_blocks(vec![
            ContentBlock::Thinking {
                text: "hmm".into(),
                signature: Some("sig".into()),
            },
            ContentBlock::RedactedThinking { data: "xyz".into() },
            ContentBlock::text("Looking."),
            ContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: json!({"path": "a"}),
            },
        ]);
        let user = Message::user_blocks(vec![
            ContentBlock::text("see"),
            ContentBlock::image_base64("image/png", [1u8, 2, 3]),
            ContentBlock::image_url("https://x/y.png"),
        ]);
        let result = Message::tool_result_blocks(
            "t1",
            vec![
                ContentBlock::text("shot"),
                ContentBlock::image_base64("image/png", [9u8]),
            ],
            true,
        );
        let w = wire(&[user, assistant, result]);

        assert_eq!(w[0]["content"][1]["source"]["type"], "base64");
        assert_eq!(w[0]["content"][1]["source"]["data"], "AQID");
        assert_eq!(
            w[0]["content"][2]["source"],
            json!({"type": "url", "url": "https://x/y.png"})
        );
        assert_eq!(
            w[1]["content"],
            json!([
                {"type": "thinking", "thinking": "hmm", "signature": "sig"},
                {"type": "redacted_thinking", "data": "xyz"},
                {"type": "text", "text": "Looking."},
                {"type": "tool_use", "id": "t1", "name": "read", "input": {"path": "a"}},
            ])
        );
        let tr = &w[2]["content"][0];
        assert_eq!(w[2]["role"], "user");
        assert_eq!(tr["type"], "tool_result");
        assert_eq!(tr["is_error"], true);
        assert_eq!(tr["content"][0]["text"], "shot");
        assert_eq!(tr["content"][1]["type"], "image");
    }

    #[test]
    fn unsigned_thinking_is_not_replayed() {
        let msg = Message::assistant_blocks(vec![
            ContentBlock::Thinking {
                text: "x".into(),
                signature: None,
            },
            ContentBlock::text("hi"),
        ]);
        assert_eq!(wire(&[msg])[0]["content"], json!("hi"));
    }

    #[test]
    fn response_blocks_survive_a_replay_in_order() {
        let body = json!({
            "id": "msg_1",
            "content": [
                { "type": "thinking", "thinking": "plan", "signature": "s1" },
                { "type": "text", "text": "Checking." },
                { "type": "tool_use", "id": "a", "name": "one", "input": {} },
                { "type": "tool_use", "id": "b", "name": "two", "input": {"x": 1} }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        });
        let resp = AnthropicProvider::map_response(body).unwrap();
        let replay = resp.to_assistant_message();
        assert_eq!(replay.content.len(), 4);
        assert!(replay.content[0].is_thinking());

        let w = wire(&[replay]);
        let types: Vec<&str> = w[0]["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["thinking", "text", "tool_use", "tool_use"]);
        assert_eq!(w[0]["content"][0]["signature"], "s1");
    }

    #[test]
    fn stream_with_thinking_signature_text_and_tool_use_yields_four_blocks() {
        let events = [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":10}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"let me "}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"think"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-1"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"enc"}}"#,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"Reading."}}"#,
            r#"{"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"t1","name":"read","input":{}}}"#,
            r#"{"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"p\":1}"}}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":7}}"#,
        ];
        let mut state = StreamState::default();
        let mut acc = StreamAccumulator::new();
        for e in events {
            if let Some(chunk) = parse_anthropic_event(&mut state, e).unwrap() {
                acc.push(&chunk);
            }
        }
        let resp = acc.into_response();
        assert_eq!(
            resp.choices[0].message.content,
            vec![
                ContentBlock::Thinking {
                    text: "let me think".into(),
                    signature: Some("sig-1".into())
                },
                ContentBlock::RedactedThinking { data: "enc".into() },
                ContentBlock::text("Reading."),
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "read".into(),
                    input: json!({"p": 1})
                },
            ]
        );
        assert_eq!(resp.usage.unwrap().total_tokens, 17);
    }

    #[test]
    fn streamed_and_whole_responses_build_the_same_message() {
        let whole = AnthropicProvider::map_response(json!({
            "content": [
                { "type": "thinking", "thinking": "ab", "signature": "s" },
                { "type": "text", "text": "hi" }
            ],
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        }))
        .unwrap();

        let mut state = StreamState::default();
        let mut acc = StreamAccumulator::new();
        for e in [
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"ab"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"s"}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"hi"}}"#,
        ] {
            if let Some(c) = parse_anthropic_event(&mut state, e).unwrap() {
                acc.push(&c);
            }
        }
        assert_eq!(
            acc.into_response().choices[0].message,
            whole.choices[0].message
        );
    }

    #[test]
    fn effort_sets_a_budget_and_raises_max_tokens() {
        let req = CompletionRequest::new("claude-sonnet-4-6", vec![Message::user("hi")])
            .with_temperature(0.2)
            .with_max_tokens(2000)
            .with_reasoning(Reasoning::effort(ReasoningEffort::High));
        let body = AnthropicProvider::build_body(&req, false).unwrap();
        assert_eq!(
            body["thinking"],
            json!({"type": "enabled", "budget_tokens": 24576})
        );
        assert!(body["max_tokens"].as_u64().unwrap() > 24576);
        // Anthropic rejects these while thinking.
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn explicit_budget_is_floored_at_the_api_minimum() {
        let req = CompletionRequest::new("m", vec![Message::user("hi")])
            .with_reasoning(Reasoning::budget(10));
        let body = AnthropicProvider::build_body(&req, false).unwrap();
        assert_eq!(body["thinking"]["budget_tokens"], 1024);
        assert_eq!(body["max_tokens"], 2048);
    }

    #[test]
    fn reasoning_off_sends_nothing() {
        let req = CompletionRequest::new("m", vec![Message::user("hi")])
            .with_temperature(0.2)
            .with_reasoning(Reasoning::Off);
        let body = AnthropicProvider::build_body(&req, false).unwrap();
        assert!(body.get("thinking").is_none());
        assert!(body.get("temperature").is_some());
    }

    #[test]
    fn default_breakpoints_land_on_tools_system_and_two_messages() {
        let mut req = CompletionRequest::new(
            "m",
            vec![
                Message::system("rules"),
                Message::user("one"),
                Message::assistant("two"),
                Message::user("three"),
            ],
        )
        .with_tools(vec![json!({"name": "a"}), json!({"name": "b"})]);
        crate::apply_default_cache_breakpoints(&mut req);

        let body = AnthropicProvider::build_body(&req, false).unwrap();
        assert_eq!(
            body["tools"][1]["cache_control"],
            json!({"type": "ephemeral"})
        );
        assert!(body["tools"][0].get("cache_control").is_none());
        assert_eq!(
            body["system"][0]["cache_control"],
            json!({"type": "ephemeral"})
        );
        assert!(body["messages"][0]["content"].is_string()); // "one": uncached
        assert_eq!(
            body["messages"][1]["content"][0]["cache_control"],
            json!({"type": "ephemeral"})
        );
        assert_eq!(
            body["messages"][2]["content"][0]["cache_control"],
            json!({"type": "ephemeral"})
        );

        let total = body.to_string().matches("cache_control").count();
        assert_eq!(total, 4);
    }

    #[test]
    fn more_than_four_breakpoints_are_trimmed_oldest_first() {
        let mut msgs = vec![Message::system("s").with_cache(CacheHint::Ephemeral)];
        for i in 0..6 {
            msgs.push(Message::user(format!("m{i}")).with_cache(CacheHint::Ephemeral));
        }
        let req = CompletionRequest::new("m", msgs)
            .with_tools(vec![json!({"name": "a"})])
            .with_cache_tools(CacheHint::Ephemeral);
        let body = AnthropicProvider::build_body(&req, false).unwrap();
        assert_eq!(body.to_string().matches("cache_control").count(), 4);
        // Tools and system survive; so do the newest messages.
        assert!(body["tools"][0].get("cache_control").is_some());
        assert!(body["system"][0].get("cache_control").is_some());
        assert!(body["messages"][5]["content"][0]
            .get("cache_control")
            .is_some());
        assert!(body["messages"][0]["content"].is_string());
    }

    #[test]
    fn a_long_hint_sets_the_ttl_and_thinking_is_skipped() {
        let msg = Message::assistant_blocks(vec![
            ContentBlock::text("a"),
            ContentBlock::Thinking {
                text: "t".into(),
                signature: Some("s".into()),
            },
        ])
        .with_cache(CacheHint::EphemeralLong);
        let w = wire(&[msg]);
        assert_eq!(
            w[0]["content"][0]["cache_control"],
            json!({"type": "ephemeral", "ttl": "1h"})
        );
        assert!(w[0]["content"][1].get("cache_control").is_none());
    }

    #[test]
    fn usage_counts_cache_reads_and_writes_as_prompt_tokens() {
        let resp = AnthropicProvider::map_response(json!({
            "content": [{ "type": "text", "text": "x" }],
            "usage": {
                "input_tokens": 5,
                "cache_read_input_tokens": 100,
                "cache_creation_input_tokens": 20,
                "output_tokens": 3
            }
        }))
        .unwrap();
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 125);
        assert_eq!(usage.cache_read_tokens, Some(100));
        assert_eq!(usage.cache_write_tokens, Some(20));
        assert_eq!(usage.total_tokens, 128);
    }

    #[test]
    fn streamed_usage_is_normalized_from_message_start() {
        let mut state = StreamState::default();
        parse_anthropic_event(
            &mut state,
            r#"{"type":"message_start","message":{"usage":{"input_tokens":4,"cache_read_input_tokens":6,"cache_creation_input_tokens":0}}}"#,
        )
        .unwrap();
        let chunk = parse_anthropic_event(
            &mut state,
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}"#,
        )
        .unwrap()
        .unwrap();
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.cache_read_tokens, Some(6));
        assert_eq!(usage.total_tokens, 12);
    }
}
