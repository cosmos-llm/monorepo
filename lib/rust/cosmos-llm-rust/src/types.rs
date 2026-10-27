use serde::{Deserialize, Serialize};

use crate::content::{CacheHint, ContentBlock, Role};
use crate::reasoning::{Reasoning, ThinkingDelta};
use crate::routing::OpenRouterRouting;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// A single message in a conversation.
///
/// A message is a [`Role`] and an ordered list of [`ContentBlock`]s: text,
/// images, thinking, tool calls, and tool results. The constructors cover the
/// common shapes ([`Message::user`], [`Message::tool_result`],
/// [`Message::assistant_with_tools`]); [`Message::user_blocks`] and
/// [`Message::assistant_blocks`] take blocks directly. Read content back with
/// [`Message::text`], [`Message::tool_calls`] and [`Message::tool_result_id`].
///
/// Providers disagree about how to encode tool traffic on the wire — OpenAI
/// uses a `"tool"` role with a `tool_call_id`, Anthropic a `"user"` message
/// holding a `tool_result` block — so each provider lowers the blocks itself.
///
/// # Serialization
///
/// Serializes as `{"role": "user", "content": [{"type": "text", ...}]}`.
/// Deserialization also accepts the earlier shape: a string `content` with
/// `tool_call_id` and `tool_calls` side fields.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "MessageRepr")]
#[non_exhaustive]
pub struct Message {
    /// Role of the message author.
    pub role: Role,
    /// Content blocks, in order.
    pub content: Vec<ContentBlock>,
    /// Optional tool name, which some providers attach to a function result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A prompt-cache breakpoint after this message's last block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheHint>,
}

/// Accepts both the current message shape and the earlier one.
#[derive(Deserialize)]
struct MessageRepr {
    role: Role,
    #[serde(default)]
    content: Value,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    cache: Option<CacheHint>,
    // Side fields of the earlier shape.
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

impl TryFrom<MessageRepr> for Message {
    type Error = String;

    fn try_from(repr: MessageRepr) -> Result<Self, String> {
        let mut content: Vec<ContentBlock> = match repr.content {
            Value::Null => Vec::new(),
            Value::String(s) if s.is_empty() => Vec::new(),
            Value::String(s) => vec![ContentBlock::text(s)],
            other => serde_json::from_value(other).map_err(|e| e.to_string())?,
        };
        if let Some(id) = repr.tool_call_id {
            content = vec![ContentBlock::ToolResult {
                tool_use_id: id,
                content,
                is_error: false,
            }];
        }
        content.extend(repr.tool_calls.into_iter().map(ContentBlock::tool_use));
        Ok(Self {
            role: repr.role,
            content,
            name: repr.name,
            cache: repr.cache,
        })
    }
}

impl Message {
    /// Creates a text message.
    ///
    /// An empty `text` produces no blocks, so an assistant turn that only
    /// calls tools carries no empty text block.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{Message, Role};
    /// let msg = Message::new(Role::User, "Hello!");
    /// assert_eq!(msg.role, Role::User);
    /// assert_eq!(msg.text(), "Hello!");
    /// ```
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            role,
            content: if text.is_empty() {
                Vec::new()
            } else {
                vec![ContentBlock::text(text)]
            },
            name: None,
            cache: None,
        }
    }

    /// Creates a message from blocks.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{ContentBlock, Message, Role};
    /// let msg = Message::from_blocks(Role::User, vec![ContentBlock::text("hi")]);
    /// assert_eq!(msg.text(), "hi");
    /// ```
    pub fn from_blocks(role: Role, content: Vec<ContentBlock>) -> Self {
        Self {
            role,
            content,
            name: None,
            cache: None,
        }
    }

    /// Creates a `system` role message.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{Message, Role};
    /// let msg = Message::system("You are a helpful assistant.");
    /// assert_eq!(msg.role, Role::System);
    /// ```
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, content)
    }

    /// Creates a `user` role message.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{Message, Role};
    /// let msg = Message::user("What is 2+2?");
    /// assert_eq!(msg.role, Role::User);
    /// ```
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, content)
    }

    /// Creates a `user` message from blocks, e.g. text plus an image.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{ContentBlock, Message};
    /// let msg = Message::user_blocks(vec![
    ///     ContentBlock::text("What is in this picture?"),
    ///     ContentBlock::image_url("https://example.com/cat.png"),
    /// ]);
    /// assert_eq!(msg.content.len(), 2);
    /// ```
    pub fn user_blocks(content: Vec<ContentBlock>) -> Self {
        Self::from_blocks(Role::User, content)
    }

    /// Creates an `assistant` role message.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{Message, Role};
    /// let msg = Message::assistant("4");
    /// assert_eq!(msg.role, Role::Assistant);
    /// ```
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Role::Assistant, content)
    }

    /// Creates an `assistant` message from blocks, e.g. a response's thinking,
    /// text and tool calls kept in order.
    pub fn assistant_blocks(content: Vec<ContentBlock>) -> Self {
        Self::from_blocks(Role::Assistant, content)
    }

    /// Creates a message carrying a tool's result back to the model.
    ///
    /// `call_id` must be the `id` of the [`ToolCall`] this answers. Each
    /// provider maps this to its own wire shape: OpenAI to a `"tool"` role
    /// message with `tool_call_id`, Anthropic to a `"user"` message holding a
    /// `tool_result` block with `tool_use_id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{Message, Role};
    ///
    /// let msg = Message::tool_result("call_1", "72°F and sunny");
    /// assert_eq!(msg.role, Role::Tool);
    /// assert_eq!(msg.tool_result_id(), Some("call_1"));
    /// assert_eq!(msg.text(), "72°F and sunny");
    /// ```
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::tool_result_blocks(call_id, Self::new(Role::Tool, content).content, false)
    }

    /// Creates a tool result that reports a failure.
    ///
    /// Anthropic sets `is_error` on the block; OpenAI has no such flag, so the
    /// text alone carries the failure.
    pub fn tool_error(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::tool_result_blocks(call_id, Self::new(Role::Tool, content).content, true)
    }

    /// Creates a tool result from blocks, so text and images can travel
    /// together (a `read` of a PNG, a screenshot tool).
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{ContentBlock, Message};
    /// let msg = Message::tool_result_blocks(
    ///     "call_1",
    ///     vec![ContentBlock::text("screenshot"), ContentBlock::image_base64("image/png", [1, 2])],
    ///     false,
    /// );
    /// assert_eq!(msg.tool_result_id(), Some("call_1"));
    /// ```
    pub fn tool_result_blocks(
        call_id: impl Into<String>,
        content: Vec<ContentBlock>,
        is_error: bool,
    ) -> Self {
        Self::from_blocks(
            Role::Tool,
            vec![ContentBlock::ToolResult {
                tool_use_id: call_id.into(),
                content,
                is_error,
            }],
        )
    }

    /// Creates a message carrying a tool's result serialized from JSON.
    ///
    /// Providers accept tool results as strings, so structured results have to
    /// be stringified somewhere. Doing it here keeps the formatting consistent
    /// instead of leaving each call site to pick a `to_string()` of its own.
    /// A JSON string is passed through unquoted, since double-encoding it only
    /// wastes tokens and confuses the model.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Message;
    /// use serde_json::json;
    ///
    /// let msg = Message::tool_result_json("call_1", &json!({"temp_f": 72}));
    /// assert_eq!(msg.text(), r#"{"temp_f":72}"#);
    ///
    /// // A bare string is not re-quoted.
    /// let plain = Message::tool_result_json("call_2", &json!("done"));
    /// assert_eq!(plain.text(), "done");
    /// ```
    pub fn tool_result_json(call_id: impl Into<String>, content: &Value) -> Self {
        let text = match content {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        Self::tool_result(call_id, text)
    }

    /// Creates an assistant message that requested tool calls.
    ///
    /// Replaying an assistant turn requires the calls it made, not only its
    /// text: OpenAI rejects a `"tool"` message whose `tool_call_id` refers to a
    /// call absent from the preceding assistant message.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{Message, Role, ToolCall};
    /// use serde_json::json;
    ///
    /// let msg = Message::assistant_with_tools(
    ///     "Let me check.",
    ///     vec![ToolCall {
    ///         id: "call_1".into(),
    ///         name: "get_weather".into(),
    ///         input: json!({"city": "Boston"}),
    ///     }],
    /// );
    /// assert_eq!(msg.role, Role::Assistant);
    /// assert_eq!(msg.tool_calls().count(), 1);
    /// ```
    pub fn assistant_with_tools(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        let mut msg = Self::new(Role::Assistant, content);
        msg.content
            .extend(tool_calls.into_iter().map(ContentBlock::tool_use));
        msg
    }

    /// Attaches a tool name to this message, returning `self`.
    ///
    /// Some providers accept a `name` alongside a function result. It is
    /// optional everywhere this crate supports.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Message;
    ///
    /// let msg = Message::tool_result("call_1", "ok").with_name("get_weather");
    /// assert_eq!(msg.name.as_deref(), Some("get_weather"));
    /// ```
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Marks a prompt-cache breakpoint after this message, returning `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CacheHint, Message};
    /// let msg = Message::system("long prompt").with_cache(CacheHint::Ephemeral);
    /// assert_eq!(msg.cache, Some(CacheHint::Ephemeral));
    /// ```
    pub fn with_cache(mut self, hint: CacheHint) -> Self {
        self.cache = Some(hint);
        self
    }

    /// The concatenated text of this message.
    ///
    /// Joins the `Text` blocks. For a tool result it is the text inside the
    /// result. Thinking, images and tool calls contribute nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{ContentBlock, Message};
    /// let msg = Message::assistant_blocks(vec![
    ///     ContentBlock::text("a"),
    ///     ContentBlock::Thinking { text: "hmm".into(), signature: None },
    ///     ContentBlock::text("b"),
    /// ]);
    /// assert_eq!(msg.text(), "ab");
    /// ```
    pub fn text(&self) -> String {
        let mut out = String::new();
        push_text(&mut out, &self.content);
        out
    }

    /// The tool calls this message requested, in order.
    pub fn tool_calls(&self) -> impl Iterator<Item = ToolCall> + '_ {
        self.content.iter().filter_map(|b| match b {
            ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            _ => None,
        })
    }

    /// The id of the call this message answers, if it carries a tool result.
    pub fn tool_result_id(&self) -> Option<&str> {
        self.content.iter().find_map(|b| match b {
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
            _ => None,
        })
    }

    /// Returns `true` if this message carries a tool result.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Message;
    ///
    /// assert!(Message::tool_result("call_1", "ok").is_tool_result());
    /// assert!(!Message::user("hi").is_tool_result());
    /// ```
    pub fn is_tool_result(&self) -> bool {
        self.role == Role::Tool || self.tool_result_id().is_some()
    }
}

fn push_text(out: &mut String, blocks: &[ContentBlock]) {
    for block in blocks {
        match block {
            ContentBlock::Text { text } => out.push_str(text),
            ContentBlock::ToolResult { content, .. } => push_text(out, content),
            _ => {}
        }
    }
}

/// Parameters for a completion or chat request.
///
/// # Examples
///
/// ```
/// use cosmos_llm::{CompletionRequest, Message};
///
/// let req = CompletionRequest::new("gpt-4o", vec![Message::user("Hi")])
///     .with_temperature(0.7)
///     .with_max_tokens(256);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// Model identifier, e.g. `"gpt-4o"` or `"claude-3-5-sonnet-20241022"`.
    pub model: String,
    /// Conversation history to send to the provider.
    pub messages: Vec<Message>,
    /// Sampling temperature in `[0.0, 2.0]`. Higher values increase randomness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Maximum number of tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Nucleus sampling cutoff in `[0.0, 1.0]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Sequences at which the model will stop generating further tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// Tool schemas the model may call, in provider-specific format (e.g. as
    /// produced by `ToolDefinition::to_openai_schema`/`to_anthropic_schema`
    /// in the `cosmos-llm-tool` crate).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    /// Provider-specific tool choice directive (e.g. `"auto"`, `"none"`, or a
    /// forced-tool object).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    /// Extra top-level fields merged into the request body verbatim.
    ///
    /// The fields above are the ones every provider understands. This is the
    /// escape hatch for the ones only *some* provider understands —
    /// OpenRouter's `provider` routing block, its `transforms`, a preview
    /// parameter that shipped ahead of a release here. Entries are written
    /// into the JSON body as-is, and a key that collides with a field the
    /// provider builds itself wins, which makes this an override as well as an
    /// addition.
    ///
    /// Providers that build an OpenAI-shaped body honour this. Anthropic does
    /// too. A provider that ignores an unknown field will silently drop
    /// whatever is put here, so this is a sharp tool: prefer the typed
    /// builders — [`CompletionRequest::with_openrouter_routing`] and friends —
    /// where one exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// use serde_json::json;
    ///
    /// let req = CompletionRequest::new("openai/gpt-4o", vec![Message::user("hi")])
    ///     .with_extra("transforms", json!(["middle-out"]));
    /// assert_eq!(req.extra.get("transforms"), Some(&json!(["middle-out"])));
    /// ```
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub extra: std::collections::BTreeMap<String, Value>,
    /// How much the model should reason. See [`CompletionRequest::with_reasoning`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
    /// A prompt-cache breakpoint after the tool list. See
    /// [`CompletionRequest::with_cache_tools`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_tools: Option<CacheHint>,
    /// Cancels the request when fired. See [`CompletionRequest::with_cancel`].
    ///
    /// A live handle, not data, so it is never serialized.
    #[serde(skip)]
    pub cancel: Option<CancellationToken>,
}

impl CompletionRequest {
    /// Creates a new [`CompletionRequest`].
    ///
    /// # Arguments
    ///
    /// * `model` — model identifier.
    /// * `messages` — conversation history.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("Hello")]);
    /// assert_eq!(req.model, "gpt-4o");
    /// ```
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
            top_p: None,
            stop: None,
            tools: None,
            tool_choice: None,
            extra: std::collections::BTreeMap::new(),
            reasoning: None,
            cache_tools: None,
            cancel: None,
        }
    }

    /// Sets the sampling temperature.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
    ///     .with_temperature(0.5);
    /// assert_eq!(req.temperature, Some(0.5));
    /// ```
    pub fn with_temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    /// Sets the maximum number of tokens to generate.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
    ///     .with_max_tokens(100);
    /// assert_eq!(req.max_tokens, Some(100));
    /// ```
    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }

    /// Sets the top-p nucleus sampling value.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
    ///     .with_top_p(0.9);
    /// assert_eq!(req.top_p, Some(0.9));
    /// ```
    pub fn with_top_p(mut self, p: f32) -> Self {
        self.top_p = Some(p);
        self
    }

    /// Adds stop sequences.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
    ///     .with_stop(vec!["END".into()]);
    /// assert!(req.stop.is_some());
    /// ```
    pub fn with_stop(mut self, stop: Vec<String>) -> Self {
        self.stop = Some(stop);
        self
    }

    /// Sets the tool schemas the model may call.
    ///
    /// Each entry is a provider-specific schema `Value`, e.g. as produced by
    /// `ToolDefinition::to_openai_schema`/`to_anthropic_schema` in the
    /// `cosmos-llm-tool` crate.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// use serde_json::json;
    ///
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
    ///     .with_tools(vec![json!({"type": "function", "function": {"name": "echo"}})]);
    /// assert_eq!(req.tools.unwrap().len(), 1);
    /// ```
    pub fn with_tools(mut self, tools: Vec<Value>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Sets the provider-specific tool choice directive.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// use serde_json::json;
    ///
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
    ///     .with_tool_choice(json!("auto"));
    /// assert_eq!(req.tool_choice, Some(json!("auto")));
    /// ```
    pub fn with_tool_choice(mut self, choice: Value) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Asks the model to reason before answering.
    ///
    /// Anthropic gets `thinking: {type: "enabled", budget_tokens}`; effort maps
    /// to a budget (minimal 1k, low 2k, medium 8k, high 24k). `max_tokens` is
    /// raised to stay above the budget, and `temperature` and `top_p` are
    /// dropped, since Anthropic rejects them with thinking on. OpenAI gets
    /// `reasoning_effort` and returns no reasoning text, only
    /// `Usage::reasoning_tokens`. OpenRouter gets `reasoning`, and returns text.
    /// A field in [`CompletionRequest::extra`] still overrides all of this.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message, Reasoning, ReasoningEffort};
    /// let req = CompletionRequest::new("claude-sonnet-4-6", vec![Message::user("hi")])
    ///     .with_reasoning(Reasoning::effort(ReasoningEffort::High));
    /// assert!(req.reasoning.unwrap().is_on());
    /// ```
    pub fn with_reasoning(mut self, reasoning: Reasoning) -> Self {
        self.reasoning = Some(reasoning);
        self
    }

    /// Marks a prompt-cache breakpoint after the tool list. Anthropic only.
    ///
    /// See [`apply_default_cache_breakpoints`](crate::apply_default_cache_breakpoints) for the usual placement.
    pub fn with_cache_tools(mut self, hint: CacheHint) -> Self {
        self.cache_tools = Some(hint);
        self
    }

    /// Makes the request cancellable.
    ///
    /// [`Client`](crate::Client) races the call against the token: when it
    /// fires, the in-flight HTTP request is dropped, no retry attempt starts,
    /// and the call returns [`CosmosError::Cancelled`](crate::CosmosError). A
    /// stream yields that error once and then ends. The token rides on the
    /// request so the [`Provider`](crate::providers::Provider) trait does not
    /// change; calling a provider directly does not honour it.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CancellationToken, CompletionRequest, Message};
    /// let token = CancellationToken::new();
    /// let req = CompletionRequest::new("m", vec![Message::user("hi")])
    ///     .with_cancel(token.clone());
    /// token.cancel();
    /// assert!(req.cancel.unwrap().is_cancelled());
    /// ```
    pub fn with_cancel(mut self, token: CancellationToken) -> Self {
        self.cancel = Some(token);
        self
    }

    /// Sets one extra top-level body field. See [`CompletionRequest::extra`].
    ///
    /// Calling this twice with the same key replaces the earlier value.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// use serde_json::json;
    ///
    /// let req = CompletionRequest::new("openai/gpt-4o", vec![Message::user("hi")])
    ///     .with_extra("user", json!("account-42"));
    /// assert_eq!(req.extra["user"], json!("account-42"));
    /// ```
    pub fn with_extra(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extra.insert(key.into(), value);
        self
    }

    /// Attaches an OpenRouter provider-routing preference to this request.
    ///
    /// Serializes [`OpenRouterRouting`] into the `provider` body field, which
    /// is how OpenRouter is told to pin, order, exclude, or sort the upstream
    /// providers eligible to serve the call. Only the OpenRouter provider
    /// reads it; sending a request carrying one to OpenAI or Anthropic is
    /// harmless but does nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message, OpenRouterRouting};
    ///
    /// let req = CompletionRequest::new(
    ///     "anthropic/claude-3.5-sonnet",
    ///     vec![Message::user("hi")],
    /// )
    /// .with_openrouter_routing(OpenRouterRouting::new().only(["anthropic"]));
    ///
    /// assert_eq!(req.extra["provider"]["only"][0], "anthropic");
    /// ```
    pub fn with_openrouter_routing(self, routing: OpenRouterRouting) -> Self {
        self.with_extra("provider", routing.to_value())
    }

    /// Returns the OpenRouter routing preference set on this request, if any.
    ///
    /// Reads back what [`CompletionRequest::with_openrouter_routing`] wrote.
    /// Returns `None` when no routing was set, and when the `provider` key
    /// holds something that is not a routing object (e.g. a raw value pushed
    /// through [`CompletionRequest::with_extra`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message, OpenRouterRouting};
    ///
    /// let plain = CompletionRequest::new("openai/gpt-4o", vec![Message::user("hi")]);
    /// assert!(plain.openrouter_routing().is_none());
    ///
    /// let pinned = plain.with_openrouter_routing(OpenRouterRouting::new().only(["azure"]));
    /// assert_eq!(pinned.openrouter_routing().unwrap().only, vec!["azure"]);
    /// ```
    pub fn openrouter_routing(&self) -> Option<OpenRouterRouting> {
        OpenRouterRouting::from_value(self.extra.get("provider")?)
    }
}

/// Token usage reported by the provider.
///
/// Every field is `#[serde(default)]`: providers disagree about which counts
/// they return, and a missing count should read as zero rather than fail the
/// whole response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Usage {
    /// Every input token the model saw, **including** tokens served from or
    /// written to the prompt cache.
    ///
    /// Anthropic reports `input_tokens` excluding cache reads and writes;
    /// the Anthropic provider adds them back so this field means the same
    /// thing everywhere. It is the figure context accounting needs.
    #[serde(default)]
    pub prompt_tokens: u32,
    /// Tokens generated in the completion, including reasoning tokens.
    #[serde(default)]
    pub completion_tokens: u32,
    /// Total tokens (prompt + completion).
    #[serde(default)]
    pub total_tokens: u32,
    /// Prompt tokens served from the cache. A subset of `prompt_tokens`;
    /// `None` when the provider does not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u32>,
    /// Prompt tokens written to the cache. A subset of `prompt_tokens`;
    /// `None` when the provider does not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u32>,
    /// Tokens spent on reasoning. A subset of `completion_tokens`, **not** an
    /// addition to it; `None` when the provider does not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
    /// What the account was actually charged for this call, in US dollars.
    ///
    /// `None` from every provider that does not report it, which is most of
    /// them — a token count is not a price, and deriving one from a local
    /// table means carrying a table that drifts every time a provider
    /// repositions a model. OpenRouter returns a real charged figure when the
    /// request asks for it, so that is the one provider where this is
    /// populated. For a figure computed locally see
    /// [`ModelInfo::estimate_cost`](crate::ModelInfo::estimate_cost), which is
    /// a different thing and must not be mixed into this one.
    ///
    /// Treat `None` as "unknown", never as "free": a budget that sums costs
    /// across providers is only counting the ones that told it anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

impl Usage {
    /// Creates a usage record from token counts, with no cost attached.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Usage;
    ///
    /// let usage = Usage::from_tokens(10, 5);
    /// assert_eq!(usage.total_tokens, 15);
    /// assert!(usage.cost.is_none());
    /// ```
    pub fn from_tokens(prompt_tokens: u32, completion_tokens: u32) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens.saturating_add(completion_tokens),
            ..Self::default()
        }
    }

    /// Attaches a charged cost in US dollars, returning `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Usage;
    ///
    /// let usage = Usage::from_tokens(10, 5).with_cost(0.00042);
    /// assert_eq!(usage.cost, Some(0.00042));
    /// ```
    #[must_use]
    pub fn with_cost(mut self, cost: f64) -> Self {
        self.cost = Some(cost);
        self
    }

    /// Records cache reads and writes (both subsets of `prompt_tokens`).
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Usage;
    /// let usage = Usage::from_tokens(100, 5).with_cache(80, 10);
    /// assert_eq!(usage.cache_read_tokens, Some(80));
    /// ```
    #[must_use]
    pub fn with_cache(mut self, read: u32, write: u32) -> Self {
        self.cache_read_tokens = Some(read);
        self.cache_write_tokens = Some(write);
        self
    }

    /// Records reasoning tokens (a subset of `completion_tokens`).
    #[must_use]
    pub fn with_reasoning_tokens(mut self, tokens: u32) -> Self {
        self.reasoning_tokens = Some(tokens);
        self
    }
}

/// A single tool call requested by the model, normalized across providers.
///
/// `input` is always a parsed JSON value regardless of whether the source
/// provider encoded arguments as a JSON string (OpenAI) or a native object
/// (Anthropic).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned identifier for this call, used to correlate a
    /// tool result back to the request.
    pub id: String,
    /// Name of the tool being called.
    pub name: String,
    /// Parsed arguments to pass to the tool.
    pub input: Value,
}

/// A single choice returned by the provider.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Choice {
    /// Zero-based index of this choice.
    pub index: u32,
    /// The generated message.
    pub message: Message,
    /// Reason the generation stopped (e.g. `"stop"`, `"length"`).
    pub finish_reason: Option<String>,
    /// Tool calls requested by the model, if any. Mirrors the `ToolUse`
    /// blocks of `message`, for callers that only want the calls.
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

/// The response from a completion request.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CompletionResponse {
    /// Provider-assigned response identifier.
    pub id: Option<String>,
    /// Model that produced the response.
    pub model: Option<String>,
    /// One or more completion choices.
    pub choices: Vec<Choice>,
    /// Token usage statistics, if provided.
    pub usage: Option<Usage>,
    /// The upstream provider that actually served the request.
    ///
    /// Only OpenRouter reports this, and only when asked — see
    /// [`OpenRouterProvider::with_route_reporting`](crate::providers::openrouter::OpenRouterProvider::with_route_reporting).
    /// `None` everywhere else, and `None` on a cache hit even when reporting
    /// is on, so treat it as "not stated" rather than "not routed".
    ///
    /// This is what makes a routing preference verifiable: without it, an
    /// `order` that silently fell through to a fallback looks exactly like one
    /// that was honoured.
    #[serde(default)]
    pub route: Option<RouteInfo>,
}

/// Which upstream provider served an OpenRouter request.
///
/// Decoded from the `openrouter_metadata` block OpenRouter attaches when
/// reporting is enabled. The wire shape is explicitly additive, so unknown
/// fields are ignored rather than treated as an error.
///
/// # Examples
///
/// ```
/// use cosmos_llm::RouteInfo;
///
/// let info = RouteInfo {
///     provider: Some("Anthropic".into()),
///     model: Some("anthropic/claude-3.5-sonnet".into()),
///     attempts: 1,
/// };
/// assert_eq!(info.provider.as_deref(), Some("Anthropic"));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RouteInfo {
    /// Name of the upstream that served the request, e.g. `"Anthropic"`.
    #[serde(default)]
    pub provider: Option<String>,
    /// The model the serving provider ran, which differs from the requested
    /// one when a fallback in `models` was used.
    #[serde(default)]
    pub model: Option<String>,
    /// How many providers were tried before one succeeded.
    ///
    /// Greater than one means the preferred provider failed and routing moved
    /// on, which is the signal that a preference was not honoured.
    #[serde(default)]
    pub attempts: u32,
}

impl RouteInfo {
    /// Extracts route information from an `openrouter_metadata` block.
    ///
    /// Prefers the endpoint marked `selected`, falling back to the last
    /// recorded attempt, since a cache hit or a partial block can carry one
    /// without the other. Returns `None` when neither is present.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::RouteInfo;
    /// use serde_json::json;
    ///
    /// let meta = json!({
    ///     "attempt": 1,
    ///     "endpoints": {
    ///         "available": [
    ///             {"provider": "OpenAI", "model": "openai/gpt-4o", "selected": true}
    ///         ]
    ///     }
    /// });
    /// let info = RouteInfo::from_metadata(&meta).unwrap();
    /// assert_eq!(info.provider.as_deref(), Some("OpenAI"));
    /// ```
    pub fn from_metadata(meta: &Value) -> Option<Self> {
        let attempts = meta["attempt"].as_u64().unwrap_or(0) as u32;

        let selected = meta["endpoints"]["available"]
            .as_array()
            .and_then(|list| list.iter().find(|e| e["selected"].as_bool() == Some(true)))
            .or_else(|| meta["attempts"].as_array().and_then(|list| list.last()))?;

        Some(Self {
            provider: selected["provider"].as_str().map(str::to_owned),
            model: selected["model"].as_str().map(str::to_owned),
            attempts,
        })
    }
}

impl CompletionResponse {
    /// Returns the text content of the first choice, if present.
    ///
    /// This is a convenience method for the common case of requesting a
    /// single completion.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionResponse, Choice, Message};
    ///
    /// let resp = CompletionResponse {
    ///     id: None,
    ///     model: None,
    ///     choices: vec![Choice {
    ///         index: 0,
    ///         message: Message::assistant("Hello!"),
    ///         finish_reason: Some("stop".into()),
    ///         tool_calls: vec![],
    ///     }],
    ///     usage: None,
    ///     route: None,
    /// };
    /// assert_eq!(resp.content().as_deref(), Some("Hello!"));
    /// ```
    pub fn content(&self) -> Option<String> {
        self.choices.first().map(|c| c.message.text())
    }

    /// Returns the assistant text of the first choice, or an empty string if
    /// there are no choices or no text content.
    ///
    /// Unlike [`CompletionResponse::content`], this never returns `None` —
    /// it mirrors the Ruby client's provider-neutral `text` accessor, which
    /// callers can use unconditionally in a tool-calling loop.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::CompletionResponse;
    ///
    /// let resp = CompletionResponse {
    ///     id: None,
    ///     model: None,
    ///     choices: vec![],
    ///     usage: None,
    ///     route: None,
    /// };
    /// assert_eq!(resp.text(), "");
    /// ```
    pub fn text(&self) -> String {
        self.content().unwrap_or_default()
    }

    /// Returns `true` if the first choice requested any tool calls.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionResponse, Choice, Message, ToolCall};
    /// use serde_json::json;
    ///
    /// let resp = CompletionResponse {
    ///     id: None,
    ///     model: None,
    ///     choices: vec![Choice {
    ///         index: 0,
    ///         message: Message::assistant(""),
    ///         finish_reason: Some("tool_calls".into()),
    ///         tool_calls: vec![ToolCall { id: "1".into(), name: "echo".into(), input: json!({}) }],
    ///     }],
    ///     usage: None,
    ///     route: None,
    /// };
    /// assert!(resp.tool_use());
    /// ```
    pub fn tool_use(&self) -> bool {
        !self.tool_calls().is_empty()
    }

    /// Returns the tool calls requested by the first choice, or an empty
    /// slice if there are no choices or no tool calls.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::CompletionResponse;
    ///
    /// let resp = CompletionResponse {
    ///     id: None,
    ///     model: None,
    ///     choices: vec![],
    ///     usage: None,
    ///     route: None,
    /// };
    /// assert!(resp.tool_calls().is_empty());
    /// ```
    pub fn tool_calls(&self) -> &[ToolCall] {
        self.choices
            .first()
            .map(|c| c.tool_calls.as_slice())
            .unwrap_or(&[])
    }

    /// Builds the assistant message to replay this response in the next
    /// request.
    ///
    /// A tool-calling loop appends this, then one [`Message::tool_result`] per
    /// call. Both halves are required: providers correlate a result to its call
    /// by id, and the id is only meaningful if the assistant turn that produced
    /// it is present.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{Choice, CompletionResponse, Message, ToolCall};
    /// use serde_json::json;
    ///
    /// let resp = CompletionResponse {
    ///     id: None,
    ///     model: None,
    ///     choices: vec![Choice {
    ///         index: 0,
    ///         message: Message::assistant("Checking."),
    ///         finish_reason: Some("tool_calls".into()),
    ///         tool_calls: vec![ToolCall {
    ///             id: "call_1".into(),
    ///             name: "get_weather".into(),
    ///             input: json!({"city": "Boston"}),
    ///         }],
    ///     }],
    ///     usage: None,
    ///     route: None,
    /// };
    ///
    /// let msg = resp.to_assistant_message();
    /// assert_eq!(msg.text(), "Checking.");
    /// assert_eq!(msg.tool_calls().next().unwrap().id, "call_1");
    /// ```
    pub fn to_assistant_message(&self) -> Message {
        let Some(choice) = self.choices.first() else {
            return Message::assistant("");
        };
        // Providers put every block, in order, on the message. A response
        // built by hand may list calls only in `Choice::tool_calls`; add the
        // ones the message lacks so the replayed turn always names them.
        let mut msg = choice.message.clone();
        msg.role = Role::Assistant;
        for call in &choice.tool_calls {
            if !msg.tool_calls().any(|c| c.id == call.id) {
                msg.content.push(ContentBlock::tool_use(call.clone()));
            }
        }
        msg
    }
}

/// A partial tool call delivered during a streaming response.
///
/// Providers stream tool arguments as JSON text fragments, so `arguments` here
/// is an unparsed partial string rather than a [`Value`]. Fragments belonging
/// to the same call share an `index`; feed them to a
/// [`StreamAccumulator`] to reassemble complete [`ToolCall`]s.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ToolCallDelta {
    /// Position of this tool call within the message, used to correlate
    /// fragments across chunks.
    pub index: u32,
    /// Provider-assigned call identifier. Sent once, on the first fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Tool name. Sent once, on the first fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Partial JSON text to append to this call's arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// A delta chunk delivered during a streaming response.
///
/// Most chunks carry only a text `delta`. The final chunk of a stream carries
/// a `finish_reason`, and — when the provider reports it — `usage`.
///
/// # Examples
///
/// ```
/// use cosmos_llm::StreamChunk;
///
/// let chunk = StreamChunk::text("Hello");
/// assert_eq!(chunk.delta, "Hello");
/// assert!(chunk.finish_reason.is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StreamChunk {
    /// Incremental text fragment for this chunk. Empty for chunks that carry
    /// only tool-call or metadata updates.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub delta: String,
    /// Set when the stream is complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// Partial tool calls advanced by this chunk.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallDelta>,
    /// Token usage, when the provider reports it. Typically present only on
    /// the final chunk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// A fragment of the model's reasoning. Show it as it streams, collapsed;
    /// the accumulator also keeps it as a `Thinking` block for replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingDelta>,
}

impl StreamChunk {
    /// Creates a chunk carrying only a text fragment.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::StreamChunk;
    /// assert_eq!(StreamChunk::text("hi").delta, "hi");
    /// ```
    pub fn text(delta: impl Into<String>) -> Self {
        Self {
            delta: delta.into(),
            ..Self::default()
        }
    }

    /// Returns `true` if this chunk carries no content of any kind.
    ///
    /// Providers emit bookkeeping events (Anthropic's `ping`, OpenAI's role
    /// preamble) that map to empty chunks. Callers rendering output can skip
    /// these; the accumulator ignores them either way.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::StreamChunk;
    ///
    /// assert!(StreamChunk::default().is_empty());
    /// assert!(!StreamChunk::text("x").is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.delta.is_empty()
            && self.finish_reason.is_none()
            && self.tool_calls.is_empty()
            && self.usage.is_none()
            && self.thinking.is_none()
    }
}

/// Reassembles streamed [`StreamChunk`]s into a [`CompletionResponse`].
///
/// Streaming hands back text a fragment at a time and tool arguments as
/// partial JSON. Feeding every chunk to an accumulator yields the same
/// response shape a non-streaming call would have produced, so a tool-calling
/// loop can be written once and used with either.
///
/// # Examples
///
/// ```
/// use cosmos_llm::{StreamAccumulator, StreamChunk};
///
/// let mut acc = StreamAccumulator::new();
/// acc.push(&StreamChunk::text("Hello, "));
/// acc.push(&StreamChunk::text("world!"));
///
/// assert_eq!(acc.text(), "Hello, world!");
/// assert_eq!(acc.into_response().text(), "Hello, world!");
/// ```
#[derive(Debug, Clone, Default)]
pub struct StreamAccumulator {
    text: String,
    finish_reason: Option<String>,
    usage: Option<Usage>,
    /// Partial tool calls keyed by stream index, in first-seen order.
    tool_calls: Vec<(u32, PartialToolCall)>,
    /// Block order: each block's first appearance, so thinking, text and tool
    /// calls come out in the order the model produced them.
    slots: Vec<Slot>,
}

/// A position in the assembled message.
#[derive(Debug, Clone)]
enum Slot {
    Text(String),
    Thinking {
        index: u32,
        text: String,
        signature: Option<String>,
    },
    Redacted {
        index: u32,
        data: String,
    },
    Tool(u32),
}

/// A tool call being assembled from streamed fragments.
#[derive(Debug, Clone, Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl StreamAccumulator {
    /// Creates an empty accumulator.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::StreamAccumulator;
    /// assert_eq!(StreamAccumulator::new().text(), "");
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Folds one chunk into the accumulated state.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{StreamAccumulator, StreamChunk};
    ///
    /// let mut acc = StreamAccumulator::new();
    /// acc.push(&StreamChunk::text("partial"));
    /// assert_eq!(acc.text(), "partial");
    /// ```
    pub fn push(&mut self, chunk: &StreamChunk) {
        if let Some(thinking) = &chunk.thinking {
            self.push_thinking(thinking);
        }

        if !chunk.delta.is_empty() {
            self.text.push_str(&chunk.delta);
            match self.slots.last_mut() {
                Some(Slot::Text(text)) => text.push_str(&chunk.delta),
                _ => self.slots.push(Slot::Text(chunk.delta.clone())),
            }
        }

        if chunk.finish_reason.is_some() {
            self.finish_reason = chunk.finish_reason.clone();
        }
        if chunk.usage.is_some() {
            self.usage = chunk.usage.clone();
        }

        for delta in &chunk.tool_calls {
            let entry = match self.tool_calls.iter_mut().find(|(i, _)| *i == delta.index) {
                Some((_, call)) => call,
                None => {
                    self.slots.push(Slot::Tool(delta.index));
                    self.tool_calls
                        .push((delta.index, PartialToolCall::default()));
                    &mut self.tool_calls.last_mut().expect("just pushed").1
                }
            };
            if let Some(ref id) = delta.id {
                entry.id = id.clone();
            }
            if let Some(ref name) = delta.name {
                entry.name = name.clone();
            }
            if let Some(ref args) = delta.arguments {
                entry.arguments.push_str(args);
            }
        }
    }

    fn push_thinking(&mut self, delta: &ThinkingDelta) {
        let index = delta.index();
        let found = self.slots.iter().position(|slot| match (slot, delta) {
            (Slot::Thinking { index: i, .. }, ThinkingDelta::Text { .. })
            | (Slot::Thinking { index: i, .. }, ThinkingDelta::Signature { .. }) => *i == index,
            (Slot::Redacted { index: i, .. }, ThinkingDelta::Redacted { .. }) => *i == index,
            _ => false,
        });
        match delta {
            ThinkingDelta::Text { text, .. }
            | ThinkingDelta::Signature {
                signature: text, ..
            } => {
                let pos = found.unwrap_or_else(|| {
                    self.slots.push(Slot::Thinking {
                        index,
                        text: String::new(),
                        signature: None,
                    });
                    self.slots.len() - 1
                });
                if let Slot::Thinking {
                    text: acc,
                    signature,
                    ..
                } = &mut self.slots[pos]
                {
                    match delta {
                        ThinkingDelta::Signature { .. } => {
                            signature.get_or_insert_with(String::new).push_str(text)
                        }
                        _ => acc.push_str(text),
                    }
                }
            }
            ThinkingDelta::Redacted { data, .. } => match found {
                Some(pos) => {
                    if let Slot::Redacted { data: acc, .. } = &mut self.slots[pos] {
                        acc.push_str(data);
                    }
                }
                None => self.slots.push(Slot::Redacted {
                    index,
                    data: data.clone(),
                }),
            },
        }
    }

    /// Returns the content blocks assembled so far, in the order the model
    /// produced them: thinking, text and tool calls interleaved as streamed.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{ContentBlock, StreamAccumulator, StreamChunk};
    ///
    /// let mut acc = StreamAccumulator::new();
    /// acc.push(&StreamChunk::text("hi"));
    /// assert_eq!(acc.blocks(), vec![ContentBlock::text("hi")]);
    /// ```
    pub fn blocks(&self) -> Vec<ContentBlock> {
        let calls = self.tool_calls();
        self.slots
            .iter()
            .filter_map(|slot| match slot {
                Slot::Text(text) => Some(ContentBlock::text(text.clone())),
                Slot::Thinking {
                    text, signature, ..
                } => Some(ContentBlock::Thinking {
                    text: text.clone(),
                    signature: signature.clone(),
                }),
                Slot::Redacted { data, .. } => {
                    Some(ContentBlock::RedactedThinking { data: data.clone() })
                }
                Slot::Tool(index) => self
                    .tool_calls
                    .iter()
                    .position(|(i, _)| i == index)
                    .map(|pos| ContentBlock::tool_use(calls[pos].clone())),
            })
            .collect()
    }

    /// Returns the text accumulated so far.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{StreamAccumulator, StreamChunk};
    ///
    /// let mut acc = StreamAccumulator::new();
    /// acc.push(&StreamChunk::text("so far"));
    /// assert_eq!(acc.text(), "so far");
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the finish reason, once the stream has reported one.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::StreamAccumulator;
    /// assert_eq!(StreamAccumulator::new().finish_reason(), None);
    /// ```
    pub fn finish_reason(&self) -> Option<&str> {
        self.finish_reason.as_deref()
    }

    /// Returns the tool calls assembled so far.
    ///
    /// Arguments that are not yet valid JSON — because the stream is still in
    /// flight — are reported as [`Value::Null`]. Empty arguments become an
    /// empty object, matching a provider that streams a no-argument call.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{StreamAccumulator, StreamChunk, ToolCallDelta};
    ///
    /// let mut acc = StreamAccumulator::new();
    /// acc.push(&StreamChunk {
    ///     tool_calls: vec![ToolCallDelta {
    ///         index: 0,
    ///         id: Some("call_1".into()),
    ///         name: Some("get_weather".into()),
    ///         arguments: Some(r#"{"city":"Boston"}"#.into()),
    ///     }],
    ///     ..Default::default()
    /// });
    ///
    /// let calls = acc.tool_calls();
    /// assert_eq!(calls[0].name, "get_weather");
    /// assert_eq!(calls[0].input["city"], "Boston");
    /// ```
    pub fn tool_calls(&self) -> Vec<ToolCall> {
        self.tool_calls
            .iter()
            .map(|(_, call)| ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                input: if call.arguments.trim().is_empty() {
                    Value::Object(Default::default())
                } else {
                    serde_json::from_str(&call.arguments).unwrap_or(Value::Null)
                },
            })
            .collect()
    }

    /// Returns the token usage reported by the stream, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::StreamAccumulator;
    /// assert!(StreamAccumulator::new().usage().is_none());
    /// ```
    pub fn usage(&self) -> Option<&Usage> {
        self.usage.as_ref()
    }

    /// Builds a [`CompletionResponse`] equivalent to the non-streaming result.
    ///
    /// The response always has exactly one choice; `id` and `model` are
    /// `None`, since streaming chunks are not required to repeat them.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{StreamAccumulator, StreamChunk};
    ///
    /// let mut acc = StreamAccumulator::new();
    /// acc.push(&StreamChunk {
    ///     delta: "done".into(),
    ///     finish_reason: Some("stop".into()),
    ///     ..Default::default()
    /// });
    ///
    /// let resp = acc.into_response();
    /// assert_eq!(resp.text(), "done");
    /// assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    /// ```
    pub fn into_response(self) -> CompletionResponse {
        let tool_calls = self.tool_calls();
        let message = Message::assistant_blocks(self.blocks());
        CompletionResponse {
            id: None,
            model: None,
            choices: vec![Choice {
                index: 0,
                message,
                finish_reason: self.finish_reason,
                tool_calls,
            }],
            usage: self.usage,
            route: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_constructors() {
        let s = Message::system("sys");
        assert_eq!(s.role, "system");
        let u = Message::user("usr");
        assert_eq!(u.role, "user");
        let a = Message::assistant("asst");
        assert_eq!(a.role, "assistant");
    }

    #[test]
    fn tool_result_carries_call_id() {
        let msg = Message::tool_result("call_1", "72F");
        assert_eq!(msg.role, "tool");
        assert_eq!(msg.text(), "72F");
        assert_eq!(msg.tool_result_id(), Some("call_1"));
        assert_eq!(msg.tool_calls().count(), 0);
        assert!(msg.is_tool_result());
    }

    #[test]
    fn tool_result_json_stringifies_structured_content() {
        let msg = Message::tool_result_json("c", &serde_json::json!({"a": 1}));
        assert_eq!(msg.text(), r#"{"a":1}"#);
    }

    #[test]
    fn tool_result_json_passes_strings_through_unquoted() {
        let msg = Message::tool_result_json("c", &serde_json::json!("plain"));
        assert_eq!(msg.text(), "plain");
    }

    #[test]
    fn assistant_with_tools_keeps_calls() {
        let msg = Message::assistant_with_tools(
            "checking",
            vec![ToolCall {
                id: "call_1".into(),
                name: "t".into(),
                input: serde_json::json!({}),
            }],
        );
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.tool_calls().count(), 1);
        assert!(msg.tool_result_id().is_none());
        assert!(!msg.is_tool_result());
    }

    #[test]
    fn plain_message_serializes_without_tool_fields() {
        // Optional fields are skipped when unset, so a text message carries
        // nothing but its role and blocks.
        let json = serde_json::to_value(Message::user("hi")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"role": "user", "content": [{"type": "text", "text": "hi"}]})
        );
    }

    #[test]
    fn tool_message_round_trips_through_serde() {
        let msg = Message::tool_result("call_1", "ok").with_name("get_weather");
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["content"][0]["tool_use_id"], "call_1");
        assert_eq!(json["name"], "get_weather");
        assert_eq!(serde_json::from_value::<Message>(json).unwrap(), msg);
    }

    #[test]
    fn response_converts_to_replayable_assistant_message() {
        let resp = CompletionResponse {
            id: None,
            model: None,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant("checking"),
                finish_reason: Some("tool_calls".into()),
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "get_weather".into(),
                    input: serde_json::json!({"city": "Boston"}),
                }],
            }],
            usage: None,
            route: None,
        };

        let msg = resp.to_assistant_message();
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.text(), "checking");
        assert_eq!(msg.tool_calls().count(), 1);
        assert_eq!(msg.tool_calls().next().unwrap().id, "call_1");
    }

    #[test]
    fn completion_request_builder() {
        let req = CompletionRequest::new("m", vec![Message::user("hi")])
            .with_temperature(0.5)
            .with_max_tokens(100)
            .with_top_p(0.9)
            .with_stop(vec!["END".into()])
            .with_tools(vec![serde_json::json!({"name": "echo"})])
            .with_tool_choice(serde_json::json!("auto"));
        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.max_tokens, Some(100));
        assert_eq!(req.top_p, Some(0.9));
        assert!(req.stop.is_some());
        assert_eq!(req.tools.as_ref().unwrap().len(), 1);
        assert_eq!(req.tool_choice, Some(serde_json::json!("auto")));
    }

    #[test]
    fn completion_response_content() {
        let resp = CompletionResponse {
            id: None,
            model: None,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant("hello"),
                finish_reason: None,
                tool_calls: vec![],
            }],
            usage: None,
            route: None,
        };
        assert_eq!(resp.content().as_deref(), Some("hello"));
        assert_eq!(resp.text(), "hello");
        assert!(!resp.tool_use());
        assert!(resp.tool_calls().is_empty());
    }

    #[test]
    fn completion_response_tool_calls() {
        let resp = CompletionResponse {
            id: None,
            model: None,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant(""),
                finish_reason: Some("tool_calls".into()),
                tool_calls: vec![ToolCall {
                    id: "1".into(),
                    name: "echo".into(),
                    input: serde_json::json!({"msg": "hi"}),
                }],
            }],
            usage: None,
            route: None,
        };
        assert!(resp.tool_use());
        assert_eq!(resp.tool_calls().len(), 1);
        assert_eq!(resp.tool_calls()[0].name, "echo");
    }

    #[test]
    fn empty_choices_returns_none() {
        let resp = CompletionResponse {
            id: None,
            model: None,
            choices: vec![],
            usage: None,
            route: None,
        };
        assert_eq!(resp.content(), None);
    }

    #[test]
    fn stream_chunk_emptiness() {
        assert!(StreamChunk::default().is_empty());
        assert!(!StreamChunk::text("x").is_empty());
        assert!(!StreamChunk {
            finish_reason: Some("stop".into()),
            ..Default::default()
        }
        .is_empty());
    }

    #[test]
    fn accumulator_joins_text_fragments() {
        let mut acc = StreamAccumulator::new();
        for part in ["Hello", ", ", "world"] {
            acc.push(&StreamChunk::text(part));
        }
        acc.push(&StreamChunk {
            finish_reason: Some("stop".into()),
            ..Default::default()
        });

        assert_eq!(acc.text(), "Hello, world");
        assert_eq!(acc.finish_reason(), Some("stop"));

        let resp = acc.into_response();
        assert_eq!(resp.text(), "Hello, world");
        assert!(!resp.tool_use());
    }

    #[test]
    fn accumulator_reassembles_split_tool_arguments() {
        let mut acc = StreamAccumulator::new();
        acc.push(&StreamChunk {
            tool_calls: vec![ToolCallDelta {
                index: 0,
                id: Some("call_1".into()),
                name: Some("get_weather".into()),
                arguments: Some("{\"city\":".into()),
            }],
            ..Default::default()
        });
        acc.push(&StreamChunk {
            tool_calls: vec![ToolCallDelta {
                index: 0,
                arguments: Some("\"Boston\"}".into()),
                ..Default::default()
            }],
            ..Default::default()
        });

        let calls = acc.tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].input, serde_json::json!({"city": "Boston"}));
    }

    #[test]
    fn accumulator_tracks_parallel_tool_calls_by_index() {
        let mut acc = StreamAccumulator::new();
        acc.push(&StreamChunk {
            tool_calls: vec![
                ToolCallDelta {
                    index: 0,
                    id: Some("a".into()),
                    name: Some("first".into()),
                    arguments: Some("{}".into()),
                },
                ToolCallDelta {
                    index: 1,
                    id: Some("b".into()),
                    name: Some("second".into()),
                    arguments: Some("{\"x\":".into()),
                },
            ],
            ..Default::default()
        });
        acc.push(&StreamChunk {
            tool_calls: vec![ToolCallDelta {
                index: 1,
                arguments: Some("2}".into()),
                ..Default::default()
            }],
            ..Default::default()
        });

        let calls = acc.tool_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "first");
        assert_eq!(calls[1].input, serde_json::json!({"x": 2}));
    }

    #[test]
    fn accumulator_treats_empty_arguments_as_empty_object() {
        let mut acc = StreamAccumulator::new();
        acc.push(&StreamChunk {
            tool_calls: vec![ToolCallDelta {
                index: 0,
                id: Some("a".into()),
                name: Some("noargs".into()),
                arguments: None,
            }],
            ..Default::default()
        });
        assert_eq!(acc.tool_calls()[0].input, serde_json::json!({}));
    }

    #[test]
    fn accumulator_reports_incomplete_arguments_as_null() {
        let mut acc = StreamAccumulator::new();
        acc.push(&StreamChunk {
            tool_calls: vec![ToolCallDelta {
                index: 0,
                id: Some("a".into()),
                name: Some("partial".into()),
                arguments: Some("{\"unterminated\":".into()),
            }],
            ..Default::default()
        });
        assert_eq!(acc.tool_calls()[0].input, Value::Null);
    }

    #[test]
    fn accumulator_keeps_last_reported_usage() {
        let mut acc = StreamAccumulator::new();
        acc.push(&StreamChunk::text("hi"));
        acc.push(&StreamChunk {
            usage: Some(Usage::from_tokens(3, 1)),
            finish_reason: Some("stop".into()),
            ..Default::default()
        });
        assert_eq!(acc.usage().unwrap().total_tokens, 4);
        assert_eq!(acc.into_response().usage.unwrap().prompt_tokens, 3);
    }

    #[test]
    fn usage_from_tokens_sums_the_total_and_leaves_cost_unknown() {
        let usage = Usage::from_tokens(10, 5);
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
        assert_eq!(usage.cost, None);
    }

    #[test]
    fn usage_from_tokens_saturates_rather_than_overflowing() {
        let usage = Usage::from_tokens(u32::MAX, 10);
        assert_eq!(usage.total_tokens, u32::MAX);
    }

    #[test]
    fn usage_with_cost_attaches_a_price() {
        assert_eq!(Usage::from_tokens(1, 1).with_cost(0.5).cost, Some(0.5));
    }

    #[test]
    fn usage_deserializes_a_body_without_a_cost() {
        // Every provider but OpenRouter reports this shape. It must parse.
        let usage: Usage = serde_json::from_value(serde_json::json!({
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }))
        .unwrap();
        assert_eq!(usage.total_tokens, 15);
        assert_eq!(usage.cost, None);
    }

    #[test]
    fn usage_deserializes_a_body_with_a_cost() {
        let usage: Usage = serde_json::from_value(serde_json::json!({
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15,
            "cost": 0.0012
        }))
        .unwrap();
        assert_eq!(usage.cost, Some(0.0012));
    }

    #[test]
    fn usage_deserializes_a_partial_body() {
        // A provider that reports only a total should not fail the response.
        let usage: Usage = serde_json::from_value(serde_json::json!({"total_tokens": 7})).unwrap();
        assert_eq!(usage.total_tokens, 7);
        assert_eq!(usage.prompt_tokens, 0);
    }
}

#[cfg(test)]
mod block_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_old_message_shape_still_deserializes() {
        let text: Message =
            serde_json::from_value(json!({"role": "user", "content": "hi"})).unwrap();
        assert_eq!(text, Message::user("hi"));

        let tool: Message = serde_json::from_value(json!({
            "role": "tool", "content": "72F", "tool_call_id": "c1", "name": "w"
        }))
        .unwrap();
        assert_eq!(tool, Message::tool_result("c1", "72F").with_name("w"));

        let assistant: Message = serde_json::from_value(json!({
            "role": "assistant",
            "content": "checking",
            "tool_calls": [{"id": "c1", "name": "w", "input": {"a": 1}}]
        }))
        .unwrap();
        assert_eq!(assistant.text(), "checking");
        assert_eq!(
            assistant.tool_calls().next().unwrap().input,
            json!({"a": 1})
        );
    }

    #[test]
    fn the_new_shape_round_trips() {
        let msg = Message::assistant_blocks(vec![
            ContentBlock::Thinking {
                text: "t".into(),
                signature: Some("s".into()),
            },
            ContentBlock::text("hi"),
            ContentBlock::image_base64("image/png", [1u8]),
        ])
        .with_cache(CacheHint::Ephemeral);
        let back: Message = serde_json::from_value(serde_json::to_value(&msg).unwrap()).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn an_unknown_role_is_an_error() {
        assert!(
            serde_json::from_value::<Message>(json!({"role": "robot", "content": "x"})).is_err()
        );
    }

    #[test]
    fn the_accumulator_keeps_blocks_in_arrival_order() {
        let mut acc = StreamAccumulator::new();
        acc.push(&StreamChunk {
            thinking: Some(ThinkingDelta::Text {
                index: 0,
                text: "a".into(),
            }),
            ..Default::default()
        });
        acc.push(&StreamChunk::text("hello "));
        acc.push(&StreamChunk {
            tool_calls: vec![ToolCallDelta {
                index: 0,
                id: Some("t".into()),
                name: Some("n".into()),
                arguments: Some("{}".into()),
            }],
            ..Default::default()
        });
        acc.push(&StreamChunk::text("again"));
        acc.push(&StreamChunk {
            thinking: Some(ThinkingDelta::Text {
                index: 0,
                text: "b".into(),
            }),
            ..Default::default()
        });

        let blocks = acc.blocks();
        assert_eq!(blocks.len(), 4);
        assert_eq!(
            blocks[0],
            ContentBlock::Thinking {
                text: "ab".into(),
                signature: None
            }
        );
        assert_eq!(blocks[1], ContentBlock::text("hello "));
        assert!(matches!(blocks[2], ContentBlock::ToolUse { .. }));
        assert_eq!(blocks[3], ContentBlock::text("again"));
        assert_eq!(acc.text(), "hello again");
    }

    #[test]
    fn a_hand_built_response_still_replays_its_tool_calls() {
        let resp = CompletionResponse {
            id: None,
            model: None,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant("x"),
                finish_reason: None,
                tool_calls: vec![ToolCall {
                    id: "a".into(),
                    name: "n".into(),
                    input: json!({}),
                }],
            }],
            usage: None,
            route: None,
        };
        let msg = resp.to_assistant_message();
        assert_eq!(msg.tool_calls().count(), 1);
        // And a message that already holds the call does not gain a second.
        let again = CompletionResponse {
            choices: vec![Choice {
                message: msg,
                ..resp.choices[0].clone()
            }],
            ..resp
        };
        assert_eq!(again.to_assistant_message().tool_calls().count(), 1);
    }

    #[test]
    fn thinking_deltas_make_a_chunk_non_empty() {
        let chunk = StreamChunk {
            thinking: Some(ThinkingDelta::Signature {
                index: 0,
                signature: "s".into(),
            }),
            ..Default::default()
        };
        assert!(!chunk.is_empty());
    }

    #[test]
    fn usage_serializes_without_unset_counts() {
        let json = serde_json::to_value(Usage::from_tokens(1, 2)).unwrap();
        assert_eq!(
            json,
            json!({"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3})
        );
    }
}
