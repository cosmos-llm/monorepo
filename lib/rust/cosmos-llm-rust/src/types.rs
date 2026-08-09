use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single message in a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Role of the message author: `"system"`, `"user"`, or `"assistant"`.
    pub role: String,
    /// Text content of the message.
    pub content: String,
}

impl Message {
    /// Creates a new message.
    ///
    /// # Arguments
    ///
    /// * `role` — one of `"system"`, `"user"`, or `"assistant"`.
    /// * `content` — message text.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Message;
    /// let msg = Message::new("user", "Hello!");
    /// assert_eq!(msg.role, "user");
    /// ```
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    /// Creates a `system` role message.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Message;
    /// let msg = Message::system("You are a helpful assistant.");
    /// assert_eq!(msg.role, "system");
    /// ```
    pub fn system(content: impl Into<String>) -> Self {
        Self::new("system", content)
    }

    /// Creates a `user` role message.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Message;
    /// let msg = Message::user("What is 2+2?");
    /// assert_eq!(msg.role, "user");
    /// ```
    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }

    /// Creates an `assistant` role message.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Message;
    /// let msg = Message::assistant("4");
    /// assert_eq!(msg.role, "assistant");
    /// ```
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", content)
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
#[derive(Debug, Clone, Serialize)]
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
}

/// Token usage reported by the provider.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Usage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u32,
    /// Tokens generated in the completion.
    pub completion_tokens: u32,
    /// Total tokens (prompt + completion).
    pub total_tokens: u32,
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
    /// Tool calls requested by the model, if any.
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
    /// };
    /// assert_eq!(resp.content(), Some("Hello!"));
    /// ```
    pub fn content(&self) -> Option<&str> {
        self.choices.first().map(|c| c.message.content.as_str())
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
    /// };
    /// assert_eq!(resp.text(), "");
    /// ```
    pub fn text(&self) -> &str {
        self.content().unwrap_or("")
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
    /// };
    /// assert!(resp.tool_calls().is_empty());
    /// ```
    pub fn tool_calls(&self) -> &[ToolCall] {
        self.choices
            .first()
            .map(|c| c.tool_calls.as_slice())
            .unwrap_or(&[])
    }
}

/// A partial tool call delivered during a streaming response.
///
/// Providers stream tool arguments as JSON text fragments, so `arguments` here
/// is an unparsed partial string rather than a [`Value`]. Fragments belonging
/// to the same call share an `index`; feed them to a
/// [`StreamAccumulator`] to reassemble complete [`ToolCall`]s.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolCallDelta {
    /// Position of this tool call within the message, used to correlate
    /// fragments across chunks.
    pub index: u32,
    /// Provider-assigned call identifier. Sent once, on the first fragment.
    pub id: Option<String>,
    /// Tool name. Sent once, on the first fragment.
    pub name: Option<String>,
    /// Partial JSON text to append to this call's arguments.
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
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StreamChunk {
    /// Incremental text fragment for this chunk. Empty for chunks that carry
    /// only tool-call or metadata updates.
    pub delta: String,
    /// Set when the stream is complete.
    pub finish_reason: Option<String>,
    /// Partial tool calls advanced by this chunk.
    pub tool_calls: Vec<ToolCallDelta>,
    /// Token usage, when the provider reports it. Typically present only on
    /// the final chunk.
    pub usage: Option<Usage>,
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
        self.text.push_str(&chunk.delta);

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
        CompletionResponse {
            id: None,
            model: None,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant(self.text),
                finish_reason: self.finish_reason,
                tool_calls,
            }],
            usage: self.usage,
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
        };
        assert_eq!(resp.content(), Some("hello"));
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
            usage: Some(Usage {
                prompt_tokens: 3,
                completion_tokens: 1,
                total_tokens: 4,
            }),
            finish_reason: Some("stop".into()),
            ..Default::default()
        });
        assert_eq!(acc.usage().unwrap().total_tokens, 4);
        assert_eq!(acc.into_response().usage.unwrap().prompt_tokens, 3);
    }
}
