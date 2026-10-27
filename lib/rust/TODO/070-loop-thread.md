# 070 — Threads: conversations that outlive a run

**Status:** implemented — `Thread` (history, `InputQueue` steering, status, usage, `truncate_to`) and `AgentLoop::run_on`/`run_with` live in `cosmos-llm-tool` rather than `agent-core`, `validate_history` runs in debug builds before each request, and queued input is delivered between tool results and the next model call (`AgentEvent::InputDelivered`). Deferred: persistence and compaction drivers (170, 200) in `agent-core`.

**Crate:** `cosmos-llm-tool` (type) and `cosmos-llm-agent-core` (driver) · **Depends on:** 060 · **Blocks:** 170, 200, 210

## Problem

`AgentLoop::run` starts with an empty `Vec<Message>` (`agent.rs:296`), adds
the system prompt and one user message, and returns the history in
`RunOutcome::messages` when it ends. A chat needs:

- the next user message appended to the same history,
- input typed while the agent is working to be queued, and delivered at the
  next safe point instead of starting a second run,
- the history to be rewritten in place by compaction (170) and by "edit an
  earlier message and resend",
- the system prompt and tool set to be swappable between turns (a mode
  change, a newly enabled tool).

## Design

Split the loop into a stateless turn engine and a stateful thread.

```rust
/// Everything one model↔tools exchange needs; no history of its own.
pub struct AgentLoop { registry, model, system, max_steps, schema, session, budget, policy /* 080 */ }

impl AgentLoop {
    /// Runs until the model stops calling tools, a cap is hit, or `cancel` fires.
    /// Appends to `history` in place, so a cancelled run keeps its progress.
    pub async fn run_on(
        &self,
        client: &dyn Completer,
        history: &mut Vec<Message>,
        events: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<RunOutcome, CosmosError>;

    /// Existing API, kept as a wrapper around `run_on`.
    pub async fn run(&self, client: &dyn Completer, user: impl Into<String>) -> Result<RunOutcome, CosmosError>;
}
```

```rust
pub struct Thread {
    pub id: ThreadId,
    history: Vec<Message>,
    pending: VecDeque<UserInput>,
    usage: TokenUsage,
    status: ThreadStatus, // Idle | Running | AwaitingApproval
}
```

`Thread` lives in `cosmos-llm-agent-core`. Its task owns the thread and
reacts to ops (210):

- `UserInput` while idle → append, start `run_on`.
- `UserInput` while running → push to `pending`. Codex and pi-mono deliver
  queued input between tool results and the next model call: after the tool
  results are appended, drain `pending` into a user message so the model sees
  it on its next step. This needs a hook in `run_on` (a `steer` channel the
  loop polls between steps).
- `Interrupt` → see 090.

### History invariants

Whatever rewrites history must keep it valid for every provider:

1. Every assistant `ToolUse` is followed by a `ToolResult` with the same id
   before the next assistant message.
2. No two consecutive user messages after provider mapping. Anthropic merges
   them, but OpenAI accepts it, so the mapping layer is the right place to
   handle it, not the thread.
3. Thinking blocks stay attached to their turn (020).

Add `fn validate_history(&[Message]) -> Result<(), HistoryError>` and call it
in debug builds before every request. It catches compaction and interrupt
bugs early, where they are cheap to diagnose.

### Edit and resend

`Thread::truncate_to(message_index)` drops everything after a user message so
the frontend can offer "edit and resend". File changes made after that point
are not undone here; that is 130's job.

## Tests

- Two `UserInput`s on an idle thread produce one history containing both
  exchanges.
- Input queued during a tool step appears as a user message before the next
  model call (assert on the mock's received requests).
- `validate_history` rejects a dangling `ToolUse`, accepts parallel results.
- `run` (old API) still passes its existing tests.

## References

- `codex/codex-rs/core/src/codex.rs` and `context_manager/`: history and
  pending input.
- `pi-mono/packages/agent/src/agent.ts`: `steer` / `followUp` queues.
- `opencode/packages/opencode/src/session/`: session/message model.
