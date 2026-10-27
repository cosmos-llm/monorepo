# 060 — Streaming the agent loop

**Status:** implemented — `AgentLoop` streams the model through `Completer::stream` and sends owned `AgentEvent`s over a bounded `tokio::sync::mpsc` channel (`event_channel`, `RunHandles`, `with_events`); `ToolOutcome` carries content, error flag and UI `display`; `ToolSpec` is split from `ToolDefinition` (`spec()`/`from_spec`); `context_handler` hands tools a `ToolContext` with cancel token and `ProgressSink`; `output_limit` is wired in with `with_output_limit`/`with_spill`; JSON and trongo goldens for each IR. Deferred: the `display` IR types for shell and edit results (120, coding-tools).

**Crate:** `cosmos-llm-tool` · **Depends on:** 010 (020 for thinking events) · **Blocks:** 070, 080, 090, 210

## Problem

`AgentLoop::run` (`agent.rs:291`) calls `Completer::completion` and waits for
the whole response. The only output while it runs is `LoopEvent`
(`agent.rs:115`), which has two variants: `Text` fires once, with the final
text, and `Tool` fires after the tool has already finished. So a frontend sees
nothing while the model writes and cannot show a tool as "running".

`LoopEvent<'a>` also borrows from the loop, and `on_event` is a synchronous
`Fn`. A UI on another task cannot receive it without copying everything inside
the callback.

## Design

### Completer gains streaming

```rust
pub trait Completer: Send + Sync {
    fn completion<'a>(&'a self, req: &'a CompletionRequest) -> BoxFuture<'a, Result<CompletionResponse, CosmosError>>;

    /// Default: call `completion` and emit the response as one chunk.
    fn stream<'a>(&'a self, req: &'a CompletionRequest) -> BoxFuture<'a, Result<CompletionStream, CosmosError>> { ... }
}
```

The default keeps every existing `Completer` (test doubles, caching wrappers)
working. `impl Completer for Client` forwards to `stream_completion` when
`can_stream()` is true.

### Events are owned and sent on a channel

```rust
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub enum AgentEvent {
    TurnStarted { turn: u32 },
    TextDelta { text: String },
    ThinkingDelta { text: String },
    /// The model has started a tool call; arguments are still streaming.
    ToolCallStarted { call_id: String, name: String },
    /// Raw partial JSON, for UIs that preview e.g. the file being written.
    ToolCallArgsDelta { call_id: String, delta: String },
    /// Arguments complete; about to ask approval (080) or execute.
    ToolCallReady { call_id: String, name: String, input: Value },
    ToolStarted { call_id: String },
    /// Optional progress from a long tool (shell output lines, 120).
    ToolOutputDelta { call_id: String, chunk: String },
    ToolFinished { call_id: String, result: ToolOutcome },
    Usage { turn: TokenUsage, total: TokenUsage },
    TurnFinished { turn: u32, stop: TurnStop },
    Error { message: String, retryable: bool },
}
```

`run` takes an `mpsc::Sender<AgentEvent>` (or the loop owns one, set with
`with_events(tx)`). Use a bounded channel. If the receiver falls behind, the
loop waits, which is the correct backpressure for a UI. Keep `on_event` as a
thin adapter for existing callers until the next breaking release.

`ToolOutcome` replaces the bare `String` result internally:

```rust
pub struct ToolOutcome {
    pub content: Vec<ContentBlock>, // what the model sees (010)
    pub is_error: bool,
    pub display: Option<Value>,     // richer data for the UI: diff, exit code, ...
}
```

`AgentEvent` and `ToolOutcome` are IRs (005): owned, serializable, no
handles. The core, the session log and every frontend consume them without
depending on loop internals. `display` stays a `Value` here, but each tool
crate documents the IR it puts there (`FileDiff` for edits, `ShellRun` for
bash), so frontends deserialize a known type instead of probing JSON.

This is also where `ToolDefinition` splits into a `ToolSpec` IR (name,
description, parameters, hints) and the handler that executes it (005).

`display` is the important part. The edit tool (110) returns a short
confirmation to the model and a structured diff to the UI. Today both have to
fit in the one string.

### Tool progress

Async handlers receive a `ToolContext`:

```rust
pub struct ToolContext {
    pub call_id: String,
    pub cancel: CancellationToken,   // 040/090
    pub progress: ProgressSink,      // sends ToolOutputDelta
    pub session: Option<Arc<Session>>,
}
```

This is a breaking change to `async_handler`'s closure type. Add a second
constructor (`context_handler`) and leave the existing one in place.

### Turn shape

Per turn: start stream → forward deltas as events while feeding a
`StreamAccumulator` → on finish, build the assistant message → if tool calls,
emit `ToolCallReady` for each, dispatch, emit results → append to history.
That is the current loop with the accumulator in the middle.

## Tests

All against the mock provider with scripted chunks:

- Text-only turn emits `TurnStarted`, ≥1 `TextDelta`, `Usage`,
  `TurnFinished` in that order, and the deltas concatenate to the final text.
- Two parallel tool calls with interleaved argument deltas: each call's
  `ToolCallStarted` precedes its `ToolCallReady`, and the `input` values
  match.
- A `Completer` that only implements `completion` still produces the same
  event sequence (one big `TextDelta`).
- A full bounded channel with a slow consumer does not lose events.

## References

- `codex/codex-rs/protocol/src/protocol.rs`: `EventMsg` variants.
- `pi-mono/packages/agent/src/agent-loop.ts`: event set for a minimal loop.
- `goose/crates/goose-agent`: `AgentEvent` stream from `reply()`.
