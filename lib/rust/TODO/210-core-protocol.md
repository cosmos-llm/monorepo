# 210 — Core protocol: ops in, events out

**Status:** implemented — `Op`/`Event`/`Envelope` in `protocol`, `Core`/`CoreBuilder`/`CoreHandle` with an interactive approver keyed by `call_id`, turns as child tasks, session log and `ContextUsage` wiring, undo and turn diffs over `CodingTools`; goldens in JSON and trongo. Additions to the sketch: `RunFinished`, `HistoryLoaded`, `Undone`, `ModelChanged`, `ModeChanged`, `Approve.remember`. Deferred: `RunCommand` (230), a scripted `.trongo` frontend, and stdio JSONL framing.

**Crate:** `cosmos-llm-agent-core` · **Depends on:** 060, 070, 080 · **Blocks:** the TUI, 220

## Problem

A frontend should not call into the loop, the thread and the tools directly.
If it does, the TUI ends up owning agent logic, and the next frontend
(headless, an editor extension, a web UI) has to reimplement it. Codex, goose
and opencode all put a message boundary between the core and its frontends.

## Design

```rust
pub struct Core { /* spawns the thread task */ }

impl Core {
    pub fn start(config: CoreConfig) -> (CoreHandle, mpsc::Receiver<Event>);
}

#[derive(Clone)]
pub struct CoreHandle { tx: mpsc::Sender<Op> }
```

### Ops (frontend → core)

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Op {
    UserInput { blocks: Vec<ContentBlock> },     // text + pasted images
    Interrupt,
    Approve { call_id: String, decision: Decision },
    SetModel { model: String },
    SetMode { mode: ApprovalMode },             // 150
    SetReasoning { reasoning: Reasoning },      // 020
    Compact { instructions: Option<String> },   // 170
    Undo { turn: Option<u32> },                 // 130
    TruncateTo { message: MsgId },              // 070
    RunCommand { name: String, args: String },  // 230
    Shutdown,
}
```

### Events (core → frontend)

`AgentEvent` from 060, wrapped with what only the core knows:

```rust
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    SessionConfigured { session_id, model, cwd, mode, context_window },
    Agent(AgentEvent),                       // deltas, tool lifecycle, usage
    ApprovalRequested { call_id, tool, preview, hints },
    ContextUsage { used, window },           // 160
    CompactionStarted, CompactionFinished { before, after },
    TurnDiff { files: Vec<FileDiff> },       // 130
    Warning { message }, Error { message, fatal },
    ShutdownComplete,
}
```

`Op` and `Event` are the core's IR (005): the only types a frontend sees.
Both are `Serialize`, so the same protocol works over stdio JSONL for
out-of-process frontends (editor plugin, `--json` exec mode) with no extra
design work. Version it: `SessionConfigured` carries `protocol: u32`.

Recorded event streams double as test fixtures. Store them as trongo data
literals (one form per event) under `tests/golden/`. They are easier to read
and hand-edit than JSONL, and a scripted frontend for tests can be a `.trongo`
file listing the ops to send.

### Concurrency model

One task owns the `Thread` and processes ops in order. Turns run as a child
task so `Interrupt` and `Approve` stay responsive during a turn. The UI never
holds a lock on core state. It rebuilds its view from events, which also makes
replaying a saved session (200) the same code path as a live one.

### What does not cross the boundary

Rendering (markdown, syntax highlighting, diff colors), keybindings, the
composer and its history: all frontend. The core sends structured data
(`FileDiff`, `display` values), never ANSI.

## Tests

The protocol is the integration test surface. Drive `Core` with a mock
completer and assert on event sequences:

- user input → text deltas → `TurnFinished`.
- tool call in `ask` mode → `ApprovalRequested` → `Approve(Allow)` → tool
  events → turn ends.
- `Interrupt` during a slow tool → `TurnFinished { Interrupted }` within the
  grace period.
- `Shutdown` with a running background job → job killed,
  `ShutdownComplete`.
- Serde round trip for every `Op` and `Event` variant (one golden JSON file).

## References

- `codex/codex-rs/protocol/src/protocol.rs`: `Op`, `EventMsg`, `Submission`.
- `codex/codex-rs/app-server-protocol`: the same types over JSON-RPC.
- `goose/crates/goose-sdk-types`.
- `pi-mono/packages/coding-agent/src/modes/rpc/`.
