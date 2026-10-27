//! A multi-turn tool-calling loop that drives a model through a toolset.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use cosmos_llm::{
    apply_default_cache_breakpoints, CancellationToken, Client, CompletionRequest,
    CompletionResponse, CompletionStream, ContentBlock, CosmosError, Message, Role,
    StreamAccumulator, StreamChunk, ThinkingDelta, ToolCall, ToolCallDelta,
};
use futures_util::future::BoxFuture;
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::approval::{ApprovalRequest, ApprovalScope, Approver, Decision};
use crate::context::{ProgressSink, ToolContext};
use crate::events::{AgentEvent, ToolOutcome, TurnStop};
use crate::output_limit::{OutputLimit, SpillStore};
use crate::spec::ToolSpec;
use crate::thread::{merge_inputs, validate_history, InputQueue, StatusCell, ThreadStatus};
use crate::{Executor, Registry, Session, ToolDefinition};

/// Anything that can answer a completion request.
///
/// [`Client`] implements this, and so can a test double or a caller's own
/// wrapper. The loop takes this rather than a concrete client so a run can be
/// tested without a network, and so a caller who has already built their own
/// caching or logging layer can drive the loop through it.
///
/// Only [`completion`](Completer::completion) is required. The loop reads the
/// model through [`stream`](Completer::stream), whose default calls
/// `completion` and replays the response as chunks, so an existing
/// implementation keeps working and just produces one big delta per block.
pub trait Completer: Send + Sync {
    /// Sends one request and returns the provider's response.
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionResponse, CosmosError>>;

    /// Sends one request and returns the response as a stream of chunks.
    ///
    /// The default calls [`completion`](Completer::completion) and emits each
    /// block of the response as one chunk (see [`response_to_stream`]).
    fn stream<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionStream, CosmosError>> {
        Box::pin(async move { Ok(response_to_stream(self.completion(req).await?)) })
    }
}

impl Completer for Client {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionResponse, CosmosError>> {
        Box::pin(async move { Client::completion(self, req.clone()).await })
    }

    /// Streams when the provider can, and otherwise falls back to the default.
    fn stream<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionStream, CosmosError>> {
        Box::pin(async move {
            if self.can_stream() {
                Client::stream_completion(self, req.clone()).await
            } else {
                Ok(response_to_stream(
                    Client::completion(self, req.clone()).await?,
                ))
            }
        })
    }
}

/// Replays a finished response as a chunk stream: one chunk per content block
/// in order (thinking, text, tool calls), then a chunk with the finish reason
/// and usage.
///
/// What the default [`Completer::stream`] uses, so a text-only reply from a
/// non-streaming completer is one `TextDelta`.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm::{Choice, CompletionResponse, Message};
/// use cosmos_llm_tool::agent::response_to_stream;
/// use futures_util::StreamExt;
///
/// # tokio_test::block_on(async {
/// let response = CompletionResponse {
///     id: None,
///     model: None,
///     choices: vec![Choice {
///         index: 0,
///         message: Message::assistant("hi"),
///         finish_reason: Some("stop".into()),
///         tool_calls: vec![],
///     }],
///     usage: None,
///     route: None,
/// };
/// let chunks: Vec<_> = response_to_stream(response).collect().await;
/// assert_eq!(chunks[0].as_ref().unwrap().delta, "hi");
/// assert_eq!(chunks.len(), 2);
/// # })
/// ```
pub fn response_to_stream(response: CompletionResponse) -> CompletionStream {
    let mut chunks = Vec::new();
    let (mut thinking_index, mut tool_index) = (0u32, 0u32);

    for block in response.to_assistant_message().content {
        match block {
            ContentBlock::Text { text } if !text.is_empty() => chunks.push(StreamChunk::text(text)),
            ContentBlock::Thinking { text, signature } => {
                chunks.push(StreamChunk {
                    thinking: Some(ThinkingDelta::Text {
                        index: thinking_index,
                        text,
                    }),
                    ..Default::default()
                });
                if let Some(signature) = signature {
                    chunks.push(StreamChunk {
                        thinking: Some(ThinkingDelta::Signature {
                            index: thinking_index,
                            signature,
                        }),
                        ..Default::default()
                    });
                }
                thinking_index += 1;
            }
            ContentBlock::RedactedThinking { data } => {
                chunks.push(StreamChunk {
                    thinking: Some(ThinkingDelta::Redacted {
                        index: thinking_index,
                        data,
                    }),
                    ..Default::default()
                });
                thinking_index += 1;
            }
            ContentBlock::ToolUse { id, name, input } => {
                chunks.push(StreamChunk {
                    tool_calls: vec![ToolCallDelta {
                        index: tool_index,
                        id: Some(id),
                        name: Some(name),
                        arguments: Some(input.to_string()),
                    }],
                    ..Default::default()
                });
                tool_index += 1;
            }
            _ => {}
        }
    }

    chunks.push(StreamChunk {
        finish_reason: response
            .choices
            .first()
            .and_then(|c| c.finish_reason.clone()),
        usage: response.usage.clone(),
        ..Default::default()
    });
    Box::pin(stream::iter(chunks.into_iter().map(Ok)))
}

/// Why a run ended.
///
/// Reported as itself rather than as a generic stop, because "the model
/// decided it was finished" and "it hit the step cap" mean very different
/// things when reading a run afterwards, and a bare step count cannot tell
/// them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StopReason {
    /// The model stopped calling tools, or a tool called [`Session::finish`].
    Finished,
    /// `max_steps` was reached with the model still working.
    Steps,
    /// The budget callback stopped the run mid-flight.
    Budget,
    /// The user interrupted through the cancellation token. Not an error: the
    /// history is valid and the thread can continue.
    Interrupted,
    /// An approval returned [`Decision::Abort`].
    Aborted,
}

/// Usage accumulated across a run.
///
/// Providers vary in what they report and some report nothing at all, so these
/// are best-effort totals: a provider that returns no usage block leaves them
/// at zero rather than failing the run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TokenUsage {
    /// Tokens consumed by prompts across every turn.
    pub prompt_tokens: u64,
    /// Tokens generated across every turn.
    pub completion_tokens: u64,
    /// Prompt plus completion, as reported.
    pub total_tokens: u64,
    /// Prompt tokens served from the provider's cache (already in
    /// `prompt_tokens`).
    pub cache_read_tokens: u64,
    /// Prompt tokens written to the provider's cache.
    pub cache_write_tokens: u64,
    /// Reasoning tokens (already in `completion_tokens`).
    pub reasoning_tokens: u64,
    /// Model turns taken so far.
    pub steps: u32,
    /// Dollars charged across every turn that reported a price.
    ///
    /// `None` until some turn reports one — see [`cosmos_llm::Usage::cost`],
    /// which only OpenRouter populates. A run mixing providers sums the turns
    /// that reported and silently omits the ones that did not, so a `Some`
    /// here is a floor on what the run cost, not a total. Budget on
    /// [`TokenUsage::total_tokens`] instead when the provider reports no price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

impl TokenUsage {
    /// Returns the accumulated cost, treating "not reported" as zero.
    ///
    /// Convenient for a budget check that would otherwise unwrap. Only use it
    /// where an unpriced run should read as free — against a provider that
    /// reports nothing, this reads zero forever and the ceiling never trips.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "agent")]
    /// # {
    /// use cosmos_llm_tool::TokenUsage;
    ///
    /// assert_eq!(TokenUsage::default().cost_or_zero(), 0.0);
    /// # }
    /// ```
    pub fn cost_or_zero(&self) -> f64 {
        self.cost.unwrap_or(0.0)
    }

    /// Adds another usage into this one.
    ///
    /// Cost sums only when `other` reported one, so a run where no turn ever
    /// reports a price ends with `None` rather than `Some(0.0)` — the
    /// difference between "this run was free" and "nobody told us".
    pub fn add(&mut self, other: &TokenUsage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.total_tokens += other.total_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.steps += other.steps;
        if let Some(cost) = other.cost {
            self.cost = Some(self.cost.unwrap_or(0.0) + cost);
        }
    }
}

/// What a finished run produced.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// The model's last non-empty text output.
    pub text: String,
    /// How many model turns were taken.
    pub steps_taken: u32,
    /// Why the run ended.
    pub reason: StopReason,
    /// Accumulated token usage.
    pub usage: TokenUsage,
    /// The full message history, for inspection or replay.
    pub messages: Vec<Message>,
}

/// An event emitted as a run proceeds.
///
/// Borrowed and synchronous, kept for existing callers of
/// [`AgentLoop::on_event`]. New code should receive [`AgentEvent`]s on a
/// channel instead.
#[derive(Debug, Clone)]
pub enum LoopEvent<'a> {
    /// The model produced final text and stopped calling tools.
    Text(&'a str),
    /// A tool was dispatched.
    Tool {
        /// The tool's name.
        name: &'a str,
        /// The arguments the model supplied.
        input: &'a Value,
        /// What the tool returned, as the model will see it.
        result: &'a str,
    },
}

/// Creates the bounded channel a run's events travel on.
///
/// A full channel makes the loop wait, which is the backpressure a UI wants.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{event_channel, AgentEvent};
///
/// let (tx, mut rx) = event_channel(64);
/// tx.try_send(AgentEvent::TurnStarted { turn: 1 }).unwrap();
/// assert_eq!(rx.try_recv().unwrap(), AgentEvent::TurnStarted { turn: 1 });
/// ```
pub fn event_channel(capacity: usize) -> (mpsc::Sender<AgentEvent>, mpsc::Receiver<AgentEvent>) {
    mpsc::channel(capacity.max(1))
}

/// The live handles one run is lent: where events go, how to stop it, and
/// where steering input arrives.
///
/// These are resources, not data, which is why they are separate from the
/// [`AgentLoop`] configuration and from the history.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm::CancellationToken;
/// use cosmos_llm_tool::{event_channel, RunHandles};
///
/// let (tx, _rx) = event_channel(64);
/// let token = CancellationToken::new();
/// let handles = RunHandles::new().events(tx).cancel(token.clone());
/// # let _ = handles;
/// // `token.cancel()` from another task interrupts the run.
/// ```
#[derive(Debug, Clone, Default)]
pub struct RunHandles {
    /// Where [`AgentEvent`]s are sent. `None` discards them.
    pub events: Option<mpsc::Sender<AgentEvent>>,
    /// Fires to interrupt the run. A fresh token is never cancelled.
    pub cancel: CancellationToken,
    /// Input delivered at the next safe point instead of starting a new run.
    pub steer: Option<InputQueue>,
    /// Updated as the run moves between phases.
    pub status: Option<StatusCell>,
}

impl RunHandles {
    /// Handles that discard events, never cancel, and take no steering.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the event channel.
    #[must_use]
    pub fn events(mut self, tx: mpsc::Sender<AgentEvent>) -> Self {
        self.events = Some(tx);
        self
    }

    /// Sets the cancellation token.
    #[must_use]
    pub fn cancel(mut self, token: CancellationToken) -> Self {
        self.cancel = token;
        self
    }

    /// Sets the steering queue.
    #[must_use]
    pub fn steer(mut self, queue: InputQueue) -> Self {
        self.steer = Some(queue);
        self
    }

    /// Sets the status cell.
    #[must_use]
    pub fn status(mut self, status: StatusCell) -> Self {
        self.status = Some(status);
        self
    }
}

/// The note appended after an interrupt so the model knows why its work
/// stopped.
const INTERRUPT_NOTE: &str = "[Request interrupted by user]";

/// Result of the approval gate for one call.
enum Gate {
    Run,
    Denied(ToolOutcome),
    Abort,
    Interrupted,
}

/// How a batch of tool calls ended.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Aborted,
    Interrupted,
}

/// How the model stream ended.
enum StreamEnd {
    Done(StreamAccumulator),
    Cancelled(StreamAccumulator),
}

/// Drives a model through a toolset until it stops calling tools.
///
/// Deliberately small. The interesting decisions live elsewhere — the tools
/// enforce the invariants, a [`Session`] holds the caps, an [`Approver`] gates
/// each call, this only turns the crank and counts what it costs.
///
/// The loop is a stateless turn engine: configuration lives here, the history
/// is lent to [`run_with`](AgentLoop::run_with), and a
/// [`Thread`](crate::Thread) owns it between runs.
///
/// # Provider neutrality
///
/// The loop speaks [`cosmos_llm::ToolCall`], which the client crate has
/// already normalized across providers: OpenAI encodes tool arguments as a
/// JSON string and Anthropic as a native object, and both arrive here as a
/// parsed [`Value`]. Replaying a turn goes through
/// [`CompletionResponse::to_assistant_message`] and [`Message::tool_result`],
/// which each provider maps to its own wire shape. Nothing in this file is
/// dialect-specific.
///
/// # A failing tool does not fail the run
///
/// A tool that errors has its error turned into the string the model sees as
/// that call's result. A raising tool would otherwise kill a run that is
/// mostly succeeding, throwing away every finding the agent had gathered — and
/// "your argument was invalid" is something a model can act on.
///
/// # Interrupts
///
/// Cancelling the run's token stops it cleanly and returns `Ok` with
/// [`StopReason::Interrupted`]; the history stays valid for every provider.
///
/// | Phase | Effect on the history |
/// |---|---|
/// | Model streaming | Partial text is kept as an assistant message; half-streamed tool calls are dropped. |
/// | Waiting on approval | Read as [`Decision::Abort`]; every call gets an "Interrupted by user" error result. |
/// | Tools executing | Tool tokens fire; handlers get a grace period (default 2 s), finished calls keep their results, the rest get "Interrupted by user". |
///
/// A short user note ("[Request interrupted by user]") is appended afterwards
/// so the model knows why its work stopped; turn it off with
/// [`with_interrupt_note`](AgentLoop::with_interrupt_note). Synchronous handlers
/// cannot be interrupted: they run on `spawn_blocking`, so they do not block the
/// loop, but one that is abandoned keeps running in the background.
///
/// # Examples
///
/// ```rust
/// # use cosmos_llm_tool::{AgentLoop, Registry, ToolDefinition, ParameterType};
/// # use serde_json::json;
/// let mut registry = Registry::new();
/// registry.register(
///     ToolDefinition::new("echo")
///         .param("value", ParameterType::String, true, "text to echo")
///         .handler(|p| Ok(json!(p["value"].as_str().unwrap_or_default()))),
/// );
///
/// let agent = AgentLoop::new(registry).with_max_steps(5);
/// assert_eq!(agent.max_steps(), 5);
/// ```
pub struct AgentLoop {
    registry: Registry,
    model: Option<String>,
    system: Option<String>,
    max_steps: u32,
    schema: SchemaDialect,
    session: Option<Arc<Session>>,
    #[allow(clippy::type_complexity)]
    budget: Option<Box<dyn Fn(&TokenUsage) -> bool + Send + Sync>>,
    #[allow(clippy::type_complexity)]
    on_event: Option<Box<dyn Fn(LoopEvent<'_>) + Send + Sync>>,
    events: Option<mpsc::Sender<AgentEvent>>,
    approver: Option<Arc<dyn Approver>>,
    grants: Mutex<Vec<ApprovalScope>>,
    output_limit: OutputLimit,
    spill: Option<SpillStore>,
    interrupt_note: bool,
    cancel_grace: Duration,
}

/// Which dialect the tool schemas are rendered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaDialect {
    /// OpenAI's `{"type": "function", "function": {...}}` shape.
    OpenAi,
    /// Anthropic's `{"name", "description", "input_schema"}` shape.
    Anthropic,
}

impl std::fmt::Debug for AgentLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentLoop")
            .field("tools", &self.registry.names())
            .field("model", &self.model)
            .field("max_steps", &self.max_steps)
            .field("schema", &self.schema)
            .field("has_session", &self.session.is_some())
            .field("has_budget", &self.budget.is_some())
            .field("has_approver", &self.approver.is_some())
            .finish()
    }
}

impl AgentLoop {
    /// Creates a loop over the given tools.
    pub fn new(registry: Registry) -> Self {
        Self {
            registry,
            model: None,
            system: None,
            max_steps: 10,
            schema: SchemaDialect::OpenAi,
            session: None,
            budget: None,
            on_event: None,
            events: None,
            approver: None,
            grants: Mutex::new(Vec::new()),
            output_limit: OutputLimit::new(),
            spill: None,
            interrupt_note: true,
            cancel_grace: Duration::from_secs(2),
        }
    }

    /// Sets the model id sent with each request.
    ///
    /// Optional: a [`Client`] configured with a default model fills this in.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sets the system prompt.
    ///
    /// [`run`](AgentLoop::run) puts it at the head of the history it builds.
    /// [`run_with`](AgentLoop::run_with) adds it to each request instead, unless
    /// the history already starts with a system message, so a thread's history
    /// stays free of it and a different loop can swap it between turns.
    #[must_use]
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Sets the hard cap on model turns.
    #[must_use]
    pub fn with_max_steps(mut self, max_steps: u32) -> Self {
        self.max_steps = max_steps;
        self
    }

    /// Sets which dialect tool schemas are rendered in.
    #[must_use]
    pub fn with_schema(mut self, schema: SchemaDialect) -> Self {
        self.schema = schema;
        self
    }

    /// Shares a session with the tools, so a `done` tool can end the run.
    ///
    /// The loop checks [`Session::finished`] after each turn and stops as soon
    /// as it is set, which is why calling `finish` costs no extra request.
    #[must_use]
    pub fn with_session(mut self, session: Arc<Session>) -> Self {
        self.session = Some(session);
        self
    }

    /// Sets a budget check, run after each turn with the totals so far.
    ///
    /// Returning `false` stops the run with [`StopReason::Budget`].
    #[must_use]
    pub fn with_budget<F>(mut self, budget: F) -> Self
    where
        F: Fn(&TokenUsage) -> bool + Send + Sync + 'static,
    {
        self.budget = Some(Box::new(budget));
        self
    }

    /// Sets an event callback, for progress reporting.
    ///
    /// Kept for existing callers; see [`with_events`](AgentLoop::with_events).
    #[must_use]
    pub fn on_event<F>(mut self, on_event: F) -> Self
    where
        F: Fn(LoopEvent<'_>) + Send + Sync + 'static,
    {
        self.on_event = Some(Box::new(on_event));
        self
    }

    /// Sends [`AgentEvent`]s to `tx` from [`run`](AgentLoop::run), and from
    /// [`run_with`](AgentLoop::run_with) when its handles carry no channel.
    #[must_use]
    pub fn with_events(mut self, tx: mpsc::Sender<AgentEvent>) -> Self {
        self.events = Some(tx);
        self
    }

    /// Asks `approver` before every tool call. With none, every call runs.
    #[must_use]
    pub fn with_approver(mut self, approver: Arc<dyn Approver>) -> Self {
        self.approver = Some(approver);
        self
    }

    /// Sets the default cap on what a tool result adds to the context.
    ///
    /// A tool's own [`output_limit`](ToolDefinition::output_limit) wins. The
    /// default, [`OutputLimit::new`], is 30,000 bytes and 2,000 lines.
    #[must_use]
    pub fn with_output_limit(mut self, limit: OutputLimit) -> Self {
        self.output_limit = limit;
        self
    }

    /// Keeps the full text of truncated results where the model can page
    /// through it.
    #[must_use]
    pub fn with_spill(mut self, spill: SpillStore) -> Self {
        self.spill = Some(spill);
        self
    }

    /// Whether an interrupt appends "[Request interrupted by user]" for the
    /// model. On by default.
    #[must_use]
    pub fn with_interrupt_note(mut self, on: bool) -> Self {
        self.interrupt_note = on;
        self
    }

    /// How long tool handlers get to return after an interrupt before they are
    /// abandoned. Default 2 seconds.
    #[must_use]
    pub fn with_cancel_grace(mut self, grace: Duration) -> Self {
        self.cancel_grace = grace;
        self
    }

    /// Returns the configured step cap.
    pub fn max_steps(&self) -> u32 {
        self.max_steps
    }

    /// The scopes granted so far by [`Decision::AllowAlways`].
    pub fn grants(&self) -> Vec<ApprovalScope> {
        self.grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Forgets every [`Decision::AllowAlways`] grant.
    pub fn clear_grants(&self) {
        self.grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// The data half of every registered tool, in registration order.
    ///
    /// This is all a provider, policy or session log needs to see.
    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        self.registry.all().iter().map(|t| t.spec()).collect()
    }

    /// Runs until the model stops calling tools, or a cap is hit.
    ///
    /// Starts a fresh history from the system prompt and `user`; use a
    /// [`Thread`](crate::Thread) to keep one across runs. Events go to the
    /// channel set with [`with_events`](AgentLoop::with_events), if any.
    ///
    /// # Errors
    ///
    /// Returns whatever error the client returns. A tool that fails does not
    /// produce an error here: its failure becomes the result text the model
    /// sees, so the run continues.
    pub async fn run(
        &self,
        client: &dyn Completer,
        user: impl Into<String>,
    ) -> Result<RunOutcome, CosmosError> {
        let mut history = Vec::new();
        if let Some(system) = &self.system {
            history.push(Message::system(system.clone()));
        }
        history.push(Message::user(user.into()));
        self.run_with(client, &mut history, &RunHandles::new())
            .await
    }

    /// Runs on a history the caller owns, sending events to `events` and
    /// stopping when `cancel` fires.
    ///
    /// Appends to `history` in place, so an interrupted or failed run keeps
    /// its progress. Shorthand for [`run_with`](AgentLoop::run_with).
    ///
    /// # Errors
    ///
    /// As [`run`](AgentLoop::run). An interrupt is not an error.
    pub async fn run_on(
        &self,
        client: &dyn Completer,
        history: &mut Vec<Message>,
        events: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<RunOutcome, CosmosError> {
        let handles = RunHandles::new()
            .events(events.clone())
            .cancel(cancel.clone());
        self.run_with(client, history, &handles).await
    }

    /// The general entry point: [`run_on`](AgentLoop::run_on) plus steering
    /// input and a status cell.
    ///
    /// # Errors
    ///
    /// As [`run`](AgentLoop::run). An interrupt is not an error.
    pub async fn run_with(
        &self,
        client: &dyn Completer,
        history: &mut Vec<Message>,
        handles: &RunHandles,
    ) -> Result<RunOutcome, CosmosError> {
        let mut effective = handles.clone();
        if effective.events.is_none() {
            effective.events = self.events.clone();
        }
        let handles = &effective;

        let schemas = self.tool_schemas();
        let mut usage = TokenUsage::default();
        let mut text = String::new();
        let mut steps_taken = 0;
        let mut reason = StopReason::Finished;
        set_status(handles, ThreadStatus::Running);

        while steps_taken < self.max_steps {
            steps_taken += 1;
            let turn = steps_taken;
            emit(handles, AgentEvent::TurnStarted { turn }).await;

            if handles.cancel.is_cancelled() {
                self.interrupt(history, handles, turn).await;
                reason = StopReason::Interrupted;
                break;
            }

            let mut messages = Vec::with_capacity(history.len() + 1);
            if let Some(system) = &self.system {
                if history.first().map(|m| m.role) != Some(Role::System) {
                    messages.push(Message::system(system.clone()));
                }
            }
            messages.extend(history.iter().cloned());
            if cfg!(debug_assertions) {
                if let Err(err) = validate_history(&messages) {
                    panic!("invalid history before request: {err}");
                }
            }

            let mut req = CompletionRequest::new(self.model.clone().unwrap_or_default(), messages)
                .with_cancel(handles.cancel.clone());
            if !schemas.is_empty() {
                req = req.with_tools(schemas.clone());
            }
            apply_default_cache_breakpoints(&mut req);

            let acc = match self.stream_turn(client, &req, handles).await {
                Ok(StreamEnd::Done(acc)) => acc,
                Ok(StreamEnd::Cancelled(acc)) => {
                    // Keep what the user already saw. A half-streamed tool
                    // call has no valid form, so only text survives.
                    if !acc.text().trim().is_empty() {
                        history.push(Message::assistant(acc.text()));
                    }
                    self.interrupt(history, handles, turn).await;
                    reason = StopReason::Interrupted;
                    break;
                }
                Err(err) => {
                    set_status(handles, ThreadStatus::Idle);
                    emit(
                        handles,
                        AgentEvent::Error {
                            message: err.to_string(),
                            retryable: err.is_retryable(),
                        },
                    )
                    .await;
                    return Err(err);
                }
            };

            let response = acc.into_response();
            let turn_usage = turn_usage(&response, turn);
            usage.add(&turn_usage);
            usage.steps = steps_taken;
            emit(
                handles,
                AgentEvent::Usage {
                    turn: turn_usage,
                    total: usage,
                },
            )
            .await;

            if !response.text().is_empty() {
                text = response.text();
            }

            let assistant = response.to_assistant_message();
            let calls = response.tool_calls().to_vec();
            // An empty assistant message is rejected by providers; a turn that
            // produced nothing leaves no trace.
            if !assistant.content.is_empty() {
                history.push(assistant);
            }

            if calls.is_empty() {
                self.emit_legacy(LoopEvent::Text(&response.text()));
                emit(
                    handles,
                    AgentEvent::TurnFinished {
                        turn,
                        stop: TurnStop::EndTurn,
                    },
                )
                .await;
                reason = StopReason::Finished;
                // Input typed during the last model call is a follow-up the
                // model should answer, not a reason to start a second run.
                if steps_taken < self.max_steps && self.deliver_steered(history, handles).await {
                    continue;
                }
                break;
            }

            for call in &calls {
                emit(
                    handles,
                    AgentEvent::ToolCallReady {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        input: call.input.clone(),
                    },
                )
                .await;
            }

            let (outcomes, flow) = self.run_tools(&calls, handles).await;
            for (call, outcome) in calls.iter().zip(&outcomes) {
                self.emit_legacy(LoopEvent::Tool {
                    name: &call.name,
                    input: &call.input,
                    result: &outcome.as_text(),
                });
                history.push(Message::tool_result_blocks(
                    call.id.clone(),
                    outcome.content.clone(),
                    outcome.is_error,
                ));
            }

            match flow {
                Flow::Interrupted => {
                    self.interrupt(history, handles, turn).await;
                    reason = StopReason::Interrupted;
                    break;
                }
                Flow::Aborted => {
                    emit(
                        handles,
                        AgentEvent::TurnFinished {
                            turn,
                            stop: TurnStop::Aborted,
                        },
                    )
                    .await;
                    reason = StopReason::Aborted;
                    break;
                }
                Flow::Continue => {}
            }

            // A tool that called `finish` ends the run here rather than at the
            // next model turn, so a `done` call does not cost an extra request.
            let (stop, end) = if self.session.as_ref().is_some_and(|s| s.finished()) {
                reason = StopReason::Finished;
                (TurnStop::Finished, true)
            } else if self.budget.as_ref().is_some_and(|b| !b(&usage)) {
                reason = StopReason::Budget;
                (TurnStop::Budget, true)
            } else if steps_taken >= self.max_steps {
                reason = StopReason::Steps;
                (TurnStop::Steps, true)
            } else {
                (TurnStop::ToolUse, false)
            };
            emit(handles, AgentEvent::TurnFinished { turn, stop }).await;
            if end {
                break;
            }

            self.deliver_steered(history, handles).await;
        }

        set_status(handles, ThreadStatus::Idle);
        Ok(RunOutcome {
            text,
            steps_taken,
            reason,
            usage,
            messages: history.clone(),
        })
    }

    /// Renders every registered tool in the configured dialect.
    ///
    /// Goes through [`ToolSpec`], so what a provider sees is exactly the data
    /// half of the tool.
    fn tool_schemas(&self) -> Vec<Value> {
        self.tool_specs()
            .iter()
            .map(|spec| match self.schema {
                SchemaDialect::OpenAi => spec.to_openai_schema(),
                SchemaDialect::Anthropic => spec.to_anthropic_schema(),
            })
            .collect()
    }

    /// Reads one model response, forwarding deltas as events.
    async fn stream_turn(
        &self,
        client: &dyn Completer,
        req: &CompletionRequest,
        handles: &RunHandles,
    ) -> Result<StreamEnd, CosmosError> {
        let mut acc = StreamAccumulator::new();
        let cancel = &handles.cancel;

        let opened = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Ok(StreamEnd::Cancelled(acc)),
            opened = client.stream(req) => opened,
        };
        let mut stream = match opened {
            Ok(stream) => stream,
            Err(CosmosError::Cancelled) => return Ok(StreamEnd::Cancelled(acc)),
            Err(err) => return Err(err),
        };

        // Stream index -> the call id announced in `ToolCallStarted`.
        let mut announced: Vec<(u32, String)> = Vec::new();
        loop {
            let item = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(StreamEnd::Cancelled(acc)),
                item = stream.next() => item,
            };
            let chunk = match item {
                None => return Ok(StreamEnd::Done(acc)),
                Some(Err(CosmosError::Cancelled)) => return Ok(StreamEnd::Cancelled(acc)),
                Some(Err(err)) => return Err(err),
                Some(Ok(chunk)) => chunk,
            };

            if let Some(ThinkingDelta::Text { text, .. }) = &chunk.thinking {
                emit(handles, AgentEvent::ThinkingDelta { text: text.clone() }).await;
            }
            if !chunk.delta.is_empty() {
                emit(
                    handles,
                    AgentEvent::TextDelta {
                        text: chunk.delta.clone(),
                    },
                )
                .await;
            }
            for delta in &chunk.tool_calls {
                let call_id = match announced.iter().find(|(i, _)| *i == delta.index) {
                    Some((_, id)) => id.clone(),
                    None => {
                        let id = delta
                            .id
                            .clone()
                            .unwrap_or_else(|| format!("call_{}", delta.index));
                        announced.push((delta.index, id.clone()));
                        emit(
                            handles,
                            AgentEvent::ToolCallStarted {
                                call_id: id.clone(),
                                name: delta.name.clone().unwrap_or_default(),
                            },
                        )
                        .await;
                        id
                    }
                };
                if let Some(args) = delta.arguments.as_ref().filter(|a| !a.is_empty()) {
                    emit(
                        handles,
                        AgentEvent::ToolCallArgsDelta {
                            call_id,
                            delta: args.clone(),
                        },
                    )
                    .await;
                }
            }
            acc.push(&chunk);
        }
    }

    /// Runs one turn's calls: approval one at a time in order, then the
    /// approved calls concurrently. Always returns one outcome per call.
    async fn run_tools(
        &self,
        calls: &[ToolCall],
        handles: &RunHandles,
    ) -> (Vec<ToolOutcome>, Flow) {
        let mut outcomes: Vec<Option<ToolOutcome>> = vec![None; calls.len()];
        let mut approved = Vec::new();
        let mut flow = Flow::Continue;

        for (i, call) in calls.iter().enumerate() {
            let skipped = match flow {
                Flow::Interrupted => Some(ToolOutcome::interrupted()),
                Flow::Aborted => Some(ToolOutcome::error(
                    "Not run: the user aborted this turn at an earlier tool call.",
                )),
                Flow::Continue => None,
            };
            if let Some(outcome) = skipped {
                record(handles, &mut outcomes, calls, i, outcome).await;
                continue;
            }

            let Some(tool) = self.registry.get(&call.name) else {
                let outcome = ToolOutcome::error(format!(
                    "No such tool: {}. Use only the tools you were given.",
                    call.name
                ));
                record(handles, &mut outcomes, calls, i, outcome).await;
                continue;
            };

            match self.gate(tool, call, handles).await {
                Gate::Run => approved.push(i),
                Gate::Denied(outcome) => record(handles, &mut outcomes, calls, i, outcome).await,
                Gate::Abort => {
                    flow = Flow::Aborted;
                    let outcome = ToolOutcome::error("Not run: the user aborted this turn.");
                    record(handles, &mut outcomes, calls, i, outcome).await;
                }
                Gate::Interrupted => {
                    flow = Flow::Interrupted;
                    record(handles, &mut outcomes, calls, i, ToolOutcome::interrupted()).await;
                }
            }
        }

        if !approved.is_empty() && flow == Flow::Continue {
            // Built with a loop rather than `.map(|&i| ..)`: the closure form
            // makes the run future fail to prove `Send` (higher-ranked
            // lifetime on the `&usize` argument), so it could not be spawned.
            let mut runs: Vec<futures_util::future::BoxFuture<'_, (usize, ToolOutcome)>> =
                Vec::with_capacity(approved.len());
            for &i in &approved {
                runs.push(Box::pin(self.run_call(i, &calls[i], handles)));
            }
            let mut running = stream::iter(runs).buffer_unordered(8);

            // Armed on interrupt: handlers get `cancel_grace` to return.
            let grace = tokio::time::sleep(Duration::from_secs(60 * 60 * 24 * 365));
            tokio::pin!(grace);
            let mut interrupted = false;
            loop {
                tokio::select! {
                    biased;
                    item = running.next() => match item {
                        Some((i, outcome)) => record(handles, &mut outcomes, calls, i, outcome).await,
                        None => break,
                    },
                    _ = handles.cancel.cancelled(), if !interrupted => {
                        interrupted = true;
                        flow = Flow::Interrupted;
                        grace.as_mut().reset(tokio::time::Instant::now() + self.cancel_grace);
                    }
                    _ = &mut grace, if interrupted => break,
                }
            }
            drop(running);
            if handles.cancel.is_cancelled() {
                flow = Flow::Interrupted;
            }
        }

        // Anything still without a result was abandoned after an interrupt.
        for i in 0..calls.len() {
            if outcomes[i].is_none() {
                record(handles, &mut outcomes, calls, i, ToolOutcome::interrupted()).await;
            }
        }

        let outcomes = outcomes
            .into_iter()
            .map(|o| o.unwrap_or_else(ToolOutcome::interrupted))
            .collect();
        (outcomes, flow)
    }

    /// Asks the approver about one call, unless a grant already covers it.
    async fn gate(&self, tool: &ToolDefinition, call: &ToolCall, handles: &RunHandles) -> Gate {
        let Some(approver) = &self.approver else {
            return Gate::Run;
        };

        let hints = tool.call_hints(&call.input);
        let granted = self
            .grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|scope| scope.covers(&call.name, &call.input, &hints));
        if granted {
            return Gate::Run;
        }

        let mut request =
            ApprovalRequest::new(call.id.clone(), call.name.clone(), call.input.clone());
        request.preview = tool.call_preview(&call.input);
        request.hints = hints;

        emit(
            handles,
            AgentEvent::ApprovalRequested {
                request: request.clone(),
            },
        )
        .await;
        set_status(handles, ThreadStatus::AwaitingApproval);
        let answer = tokio::select! {
            biased;
            _ = handles.cancel.cancelled() => None,
            decision = approver.check(&request) => Some(decision),
        };
        set_status(handles, ThreadStatus::Running);

        let Some(decision) = answer else {
            emit(
                handles,
                AgentEvent::ApprovalResolved {
                    call_id: call.id.clone(),
                    decision: Decision::Abort,
                },
            )
            .await;
            return Gate::Interrupted;
        };
        emit(
            handles,
            AgentEvent::ApprovalResolved {
                call_id: call.id.clone(),
                decision: decision.clone(),
            },
        )
        .await;

        match decision {
            Decision::Allow => Gate::Run,
            Decision::AllowAlways { scope } => {
                self.grants
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(scope);
                Gate::Run
            }
            Decision::Deny { reason } => Gate::Denied(ToolOutcome::error(match reason {
                Some(reason) => format!("User denied: {reason}"),
                None => "User denied this tool call.".to_owned(),
            })),
            Decision::Abort => Gate::Abort,
        }
    }

    /// Executes one approved call, forwarding its progress as events.
    async fn run_call(
        &self,
        index: usize,
        call: &ToolCall,
        handles: &RunHandles,
    ) -> (usize, ToolOutcome) {
        let Some(tool) = self.registry.get(&call.name) else {
            return (
                index,
                ToolOutcome::error(format!("No such tool: {}", call.name)),
            );
        };
        emit(
            handles,
            AgentEvent::ToolStarted {
                call_id: call.id.clone(),
            },
        )
        .await;

        let (progress_tx, mut progress_rx) = mpsc::channel::<String>(64);
        let ctx = ToolContext::new(
            call.id.clone(),
            handles.cancel.child_token(),
            ProgressSink::new(progress_tx),
            self.session.clone(),
        );
        let display = ctx.clone();
        let exec = Executor::execute_with(tool, call.input.clone(), ctx);
        tokio::pin!(exec);

        let forward = |chunk: String| {
            emit(
                handles,
                AgentEvent::ToolOutputDelta {
                    call_id: call.id.clone(),
                    chunk,
                },
            )
        };

        let mut listening = true;
        let result = loop {
            tokio::select! {
                biased;
                chunk = progress_rx.recv(), if listening => match chunk {
                    Some(chunk) => forward(chunk).await,
                    None => listening = false,
                },
                result = &mut exec => break result,
            }
        };
        // Chunks sent just before the handler returned.
        while let Ok(chunk) = progress_rx.try_recv() {
            forward(chunk).await;
        }

        let outcome = match result {
            Ok(value) => {
                let limited =
                    Executor::limit_result(tool, &value, &self.output_limit, self.spill.as_ref());
                let mut outcome = ToolOutcome::text(limited.text);
                outcome.truncated = limited.truncated;
                outcome.spill_path = limited.spill_path.map(|p| p.to_string_lossy().into_owned());
                outcome.display = display.take_display();
                outcome
            }
            // A tool failure is information the model can act on, not a
            // reason to abandon the run.
            Err(err) => {
                let mut outcome = ToolOutcome::error(format!("Tool error: {err}"));
                outcome.display = display.take_display();
                outcome
            }
        };
        (index, outcome)
    }

    /// Ends a run on an interrupt: the note for the model, and `TurnFinished`.
    async fn interrupt(&self, history: &mut Vec<Message>, handles: &RunHandles, turn: u32) {
        // Two user messages in a row add nothing: the unanswered one already
        // says what the model was asked.
        let last_is_user = history
            .last()
            .is_some_and(|m| m.role == Role::User && !m.is_tool_result());
        if self.interrupt_note && !last_is_user {
            history.push(Message::user(INTERRUPT_NOTE));
        }
        emit(
            handles,
            AgentEvent::TurnFinished {
                turn,
                stop: TurnStop::Interrupted,
            },
        )
        .await;
    }

    /// Moves queued input into the history as one user message. Returns
    /// whether there was any.
    async fn deliver_steered(&self, history: &mut Vec<Message>, handles: &RunHandles) -> bool {
        let Some(queue) = &handles.steer else {
            return false;
        };
        let pending = queue.drain();
        if pending.is_empty() {
            return false;
        }
        let message = merge_inputs(pending);
        emit(
            handles,
            AgentEvent::InputDelivered {
                text: message.text(),
            },
        )
        .await;
        history.push(message);
        true
    }

    /// Hands an event to the callback, if one is set.
    fn emit_legacy(&self, event: LoopEvent<'_>) {
        if let Some(callback) = &self.on_event {
            callback(event);
        }
    }
}

/// Sends an event, waiting if the channel is full. A dropped receiver is
/// ignored: the run continues unobserved.
async fn emit(handles: &RunHandles, event: AgentEvent) {
    if let Some(tx) = &handles.events {
        let _ = tx.send(event).await;
    }
}

fn set_status(handles: &RunHandles, status: ThreadStatus) {
    if let Some(cell) = &handles.status {
        cell.set(status);
    }
}

/// Stores a call's outcome and announces it.
async fn record(
    handles: &RunHandles,
    outcomes: &mut [Option<ToolOutcome>],
    calls: &[ToolCall],
    index: usize,
    outcome: ToolOutcome,
) {
    emit(
        handles,
        AgentEvent::ToolFinished {
            call_id: calls[index].id.clone(),
            result: outcome.clone(),
        },
    )
    .await;
    outcomes[index] = Some(outcome);
}

/// One response's reported usage as a [`TokenUsage`] for that turn.
///
/// Cost stays `None` unless the turn reported one: the difference between
/// "this turn was free" and "nobody told us".
fn turn_usage(response: &CompletionResponse, step: u32) -> TokenUsage {
    let mut usage = TokenUsage {
        steps: step,
        ..TokenUsage::default()
    };
    if let Some(reported) = &response.usage {
        usage.prompt_tokens = u64::from(reported.prompt_tokens);
        usage.completion_tokens = u64::from(reported.completion_tokens);
        usage.total_tokens = u64::from(reported.total_tokens);
        usage.cache_read_tokens = u64::from(reported.cache_read_tokens.unwrap_or(0));
        usage.cache_write_tokens = u64::from(reported.cache_write_tokens.unwrap_or(0));
        usage.reasoning_tokens = u64::from(reported.reasoning_tokens.unwrap_or(0));
        usage.cost = reported.cost;
    }
    usage
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParameterType, ToolDefinition};
    use cosmos_llm::{Choice, Usage};
    use serde_json::json;
    use std::future::Future;
    use std::pin::Pin;

    /// A client that replays a scripted list of responses.
    struct ScriptedClient {
        responses: Mutex<Vec<CompletionResponse>>,
        requests: Mutex<Vec<CompletionRequest>>,
    }

    impl ScriptedClient {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                // Reversed so `pop` yields them in order.
                responses: Mutex::new(responses.into_iter().rev().collect()),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn request_count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }
    }

    impl Completer for ScriptedClient {
        fn completion<'a>(
            &'a self,
            req: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>>
        {
            self.requests.lock().unwrap().push(req.clone());
            let next = self.responses.lock().unwrap().pop();
            Box::pin(async move {
                next.ok_or_else(|| CosmosError::Api {
                    provider: "scripted".to_owned(),
                    message: "script exhausted".to_owned(),
                })
            })
        }
    }

    fn reply(text: &str) -> CompletionResponse {
        CompletionResponse {
            id: None,
            model: None,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant(text),
                finish_reason: Some("stop".to_owned()),
                tool_calls: vec![],
            }],
            usage: None,
            route: None,
        }
    }

    fn calls(text: &str, tool_calls: Vec<ToolCall>) -> CompletionResponse {
        CompletionResponse {
            id: None,
            model: None,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant(text),
                finish_reason: Some("tool_calls".to_owned()),
                tool_calls,
            }],
            usage: None,
            route: None,
        }
    }

    fn call(id: &str, name: &str, input: Value) -> ToolCall {
        ToolCall {
            id: id.to_owned(),
            name: name.to_owned(),
            input,
        }
    }

    fn echo_registry() -> Registry {
        let mut registry = Registry::new();
        registry.register(
            ToolDefinition::new("echo")
                .param("value", ParameterType::String, true, "text")
                .handler(|p| {
                    Ok(json!(format!(
                        "echoed:{}",
                        p["value"].as_str().unwrap_or("")
                    )))
                }),
        );
        registry
    }

    #[tokio::test]
    async fn returns_immediately_when_no_tool_use() {
        let client = ScriptedClient::new(vec![reply("final answer")]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.text, "final answer");
        assert_eq!(outcome.steps_taken, 1);
        assert_eq!(outcome.reason, StopReason::Finished);
        assert_eq!(client.request_count(), 1);
    }

    #[tokio::test]
    async fn dispatches_a_tool_call_and_feeds_the_result_back() {
        let client = ScriptedClient::new(vec![
            calls("", vec![call("call_1", "echo", json!({"value": "hello"}))]),
            reply("done"),
        ]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.text, "done");
        assert_eq!(outcome.steps_taken, 2);
        let result = outcome
            .messages
            .iter()
            .find(|m| m.is_tool_result())
            .expect("a tool result was appended");
        assert_eq!(result.text(), "echoed:hello");
        assert_eq!(result.tool_result_id(), Some("call_1"));
    }

    #[tokio::test]
    async fn assistant_turn_is_replayed_with_its_tool_calls() {
        let client = ScriptedClient::new(vec![
            calls(
                "thinking",
                vec![call("call_1", "echo", json!({"value": "x"}))],
            ),
            reply("done"),
        ]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let assistant = outcome
            .messages
            .iter()
            .find(|m| m.role == "assistant")
            .expect("the assistant turn was replayed");
        assert_eq!(assistant.text(), "thinking");
        let replayed: Vec<_> = assistant.tool_calls().collect();
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].id, "call_1");
    }

    #[tokio::test]
    async fn stops_at_max_steps_while_the_model_keeps_calling_tools() {
        let responses = (0..5)
            .map(|_| calls("mid", vec![call("c", "echo", json!({"value": "x"}))]))
            .collect();
        let client = ScriptedClient::new(responses);
        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_max_steps(3);

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.steps_taken, 3);
        assert_eq!(outcome.reason, StopReason::Steps);
        assert_eq!(client.request_count(), 3);
    }

    #[tokio::test]
    async fn unknown_tool_is_reported_to_the_model_not_raised() {
        let client = ScriptedClient::new(vec![
            calls("", vec![call("call_1", "nonexistent", json!({}))]),
            reply("recovered"),
        ]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let result = outcome
            .messages
            .iter()
            .find(|m| m.is_tool_result())
            .unwrap();
        assert!(result.text().contains("No such tool: nonexistent"));
        assert_eq!(outcome.text, "recovered");
    }

    #[tokio::test]
    async fn a_failing_tool_becomes_a_message_the_model_can_act_on() {
        let mut registry = Registry::new();
        registry.register(ToolDefinition::new("boom").handler(|_| Err("exploded".to_owned())));
        let client = ScriptedClient::new(vec![
            calls("", vec![call("call_1", "boom", json!({}))]),
            reply("recovered"),
        ]);
        let agent = AgentLoop::new(registry).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let result = outcome
            .messages
            .iter()
            .find(|m| m.is_tool_result())
            .unwrap();
        assert!(result.text().contains("Tool error"));
        assert!(result.text().contains("exploded"));
        assert_eq!(outcome.reason, StopReason::Finished);
    }

    #[tokio::test]
    async fn session_finish_ends_the_run_without_another_request() {
        let session = Arc::new(Session::new());
        let for_tool = Arc::clone(&session);

        let mut registry = Registry::new();
        registry.register(ToolDefinition::new("done").handler(move |_| {
            for_tool.finish(Some("all set"));
            Ok(json!("done"))
        }));

        let client = ScriptedClient::new(vec![
            calls("", vec![call("call_1", "done", json!({}))]),
            reply("never reached"),
        ]);
        let agent = AgentLoop::new(registry)
            .with_model("m")
            .with_session(Arc::clone(&session));

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.reason, StopReason::Finished);
        assert_eq!(outcome.steps_taken, 1);
        assert_eq!(client.request_count(), 1);
        assert_eq!(session.notes(), vec!["all set".to_string()]);
    }

    #[tokio::test]
    async fn budget_callback_stops_the_run() {
        let responses = (0..5)
            .map(|_| calls("mid", vec![call("c", "echo", json!({"value": "x"}))]))
            .collect();
        let client = ScriptedClient::new(responses);
        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_max_steps(10)
            .with_budget(|usage| usage.steps < 2);

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.reason, StopReason::Budget);
        assert_eq!(outcome.steps_taken, 2);
    }

    #[tokio::test]
    async fn budget_is_not_consulted_when_the_model_stops_on_its_own() {
        let client = ScriptedClient::new(vec![reply("done")]);
        let seen = Arc::new(Mutex::new(false));
        let flag = Arc::clone(&seen);

        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_budget(move |_| {
                *flag.lock().unwrap() = true;
                true
            });

        agent.run(&client, "hi").await.unwrap();

        assert!(!*seen.lock().unwrap());
    }

    #[tokio::test]
    async fn usage_accumulates_across_turns() {
        let mut first = calls("", vec![call("c", "echo", json!({"value": "x"}))]);
        first.usage = Some(Usage::from_tokens(10, 5));
        let mut second = reply("done");
        second.usage = Some(Usage::from_tokens(20, 4));

        let client = ScriptedClient::new(vec![first, second]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.usage.prompt_tokens, 30);
        assert_eq!(outcome.usage.completion_tokens, 9);
        assert_eq!(outcome.usage.total_tokens, 39);
        assert_eq!(outcome.usage.steps, 2);
    }

    #[tokio::test]
    async fn cost_accumulates_across_turns() {
        let mut first = calls("", vec![call("c", "echo", json!({"value": "x"}))]);
        first.usage = Some(Usage::from_tokens(10, 5).with_cost(0.001));
        let mut second = reply("done");
        second.usage = Some(Usage::from_tokens(20, 4).with_cost(0.002));

        let client = ScriptedClient::new(vec![first, second]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let cost = outcome.usage.cost.expect("both turns reported a price");
        assert!((cost - 0.003).abs() < 1e-9, "got {cost}");
    }

    #[tokio::test]
    async fn cost_stays_unknown_when_no_turn_reports_one() {
        let mut only = reply("done");
        only.usage = Some(Usage::from_tokens(10, 5));

        let client = ScriptedClient::new(vec![only]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        // Unknown, not free. A run that was billed but never told us must not
        // read as zero dollars.
        assert_eq!(outcome.usage.cost, None);
        assert_eq!(outcome.usage.cost_or_zero(), 0.0);
    }

    #[tokio::test]
    async fn a_priceless_turn_does_not_reset_the_running_cost() {
        let mut first = calls("", vec![call("c", "echo", json!({"value": "x"}))]);
        first.usage = Some(Usage::from_tokens(10, 5).with_cost(0.001));
        // Second turn reports counts but no price.
        let mut second = reply("done");
        second.usage = Some(Usage::from_tokens(20, 4));

        let client = ScriptedClient::new(vec![first, second]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let cost = outcome.usage.cost.expect("the first turn reported a price");
        assert!((cost - 0.001).abs() < 1e-9, "got {cost}");
        assert_eq!(outcome.usage.total_tokens, 39);
    }

    #[tokio::test]
    async fn budget_can_stop_a_run_on_a_dollar_ceiling() {
        let responses = (0..5)
            .map(|_| {
                let mut turn = calls("mid", vec![call("c", "echo", json!({"value": "x"}))]);
                turn.usage = Some(Usage::from_tokens(10, 5).with_cost(0.004));
                turn
            })
            .collect();
        let client = ScriptedClient::new(responses);
        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_max_steps(10)
            .with_budget(|usage| usage.cost_or_zero() < 0.01);

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.reason, StopReason::Budget);
        // Stops on the turn that crosses $0.01: 0.004, 0.008, 0.012.
        assert_eq!(outcome.steps_taken, 3);
    }

    #[tokio::test]
    async fn usage_tolerates_a_provider_that_reports_none() {
        let client = ScriptedClient::new(vec![reply("done")]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.usage.total_tokens, 0);
        assert_eq!(outcome.usage.steps, 1);
    }

    #[tokio::test]
    async fn several_calls_in_one_turn_keep_their_order() {
        let client = ScriptedClient::new(vec![
            calls(
                "",
                vec![
                    call("c1", "echo", json!({"value": "one"})),
                    call("c2", "echo", json!({"value": "two"})),
                    call("c3", "echo", json!({"value": "three"})),
                ],
            ),
            reply("done"),
        ]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let results: Vec<_> = outcome
            .messages
            .iter()
            .filter(|m| m.is_tool_result())
            .map(|m| m.text())
            .collect();
        assert_eq!(results, vec!["echoed:one", "echoed:two", "echoed:three"]);
    }

    #[tokio::test]
    async fn a_mix_of_known_and_unknown_tools_lines_up() {
        let client = ScriptedClient::new(vec![
            calls(
                "",
                vec![
                    call("c1", "echo", json!({"value": "one"})),
                    call("c2", "missing", json!({})),
                    call("c3", "echo", json!({"value": "three"})),
                ],
            ),
            reply("done"),
        ]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let results: Vec<_> = outcome
            .messages
            .iter()
            .filter(|m| m.is_tool_result())
            .map(|m| m.text())
            .collect();
        assert_eq!(results[0], "echoed:one");
        assert!(results[1].contains("No such tool: missing"));
        assert_eq!(results[2], "echoed:three");
    }

    #[tokio::test]
    async fn events_report_tool_calls_and_final_text() {
        let client = ScriptedClient::new(vec![
            calls("", vec![call("c1", "echo", json!({"value": "hello"}))]),
            reply("all done"),
        ]);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);

        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .on_event(move |event| {
                let entry = match event {
                    LoopEvent::Text(text) => format!("text:{text}"),
                    LoopEvent::Tool { name, result, .. } => format!("tool:{name}:{result}"),
                };
                sink.lock().unwrap().push(entry);
            });

        agent.run(&client, "hi").await.unwrap();

        let events = seen.lock().unwrap().clone();
        assert_eq!(events, vec!["tool:echo:echoed:hello", "text:all done"]);
    }

    #[tokio::test]
    async fn system_prompt_leads_the_message_history() {
        let client = ScriptedClient::new(vec![reply("ok")]);
        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_system("be brief");

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.messages[0].role, "system");
        assert_eq!(outcome.messages[0].text(), "be brief");
        assert_eq!(outcome.messages[1].role, "user");
    }

    #[tokio::test]
    async fn tool_schemas_are_sent_in_the_configured_dialect() {
        let client = ScriptedClient::new(vec![reply("ok")]);
        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_schema(SchemaDialect::Anthropic);

        agent.run(&client, "hi").await.unwrap();

        let sent = client.requests.lock().unwrap()[0].tools.clone().unwrap();
        assert!(sent[0].get("input_schema").is_some(), "anthropic dialect");
        assert!(sent[0].get("function").is_none());
    }

    #[tokio::test]
    async fn a_client_error_ends_the_run_as_an_error() {
        let client = ScriptedClient::new(vec![]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        assert!(agent.run(&client, "hi").await.is_err());
    }

    #[tokio::test]
    async fn a_json_string_result_is_not_requoted() {
        let client = ScriptedClient::new(vec![
            calls("", vec![call("c1", "echo", json!({"value": "x"}))]),
            reply("done"),
        ]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let result = outcome
            .messages
            .iter()
            .find(|m| m.is_tool_result())
            .unwrap();
        assert_eq!(result.text(), "echoed:x");
    }
}

// Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
