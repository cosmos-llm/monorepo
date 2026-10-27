# 080 — Approval before tool execution

**Status:** implemented — `Approver` trait (boxed-future, data-only `ApprovalRequest`/`Decision`/`ApprovalScope`), `AgentLoop::with_approver`, per-call `ToolHints` and `preview` hooks on `ToolDefinition` (`read_only`/`destructive`/`hints`/`preview`), session grants for `AllowAlways`, and edit/write/multi_edit in `cosmos-llm-coding-tools` now offer a diff preview. Deferred: the interactive oneshot approver and policy engine (150, 210).

**Crate:** `cosmos-llm-tool` (hook) and `cosmos-llm-agent-core` (policy, 150) · **Depends on:** 060 · **Blocks:** 120 in practice

## Problem

`dispatch_all` (`agent.rs`) runs every requested call straight away. Running
`rm -rf` or overwriting a file has to be able to stop and ask the user first,
and the answer comes back asynchronously from a UI.

## Design

The loop gets a single asynchronous gate. It decides nothing itself.

```rust
#[async_trait]
pub trait Approver: Send + Sync {
    async fn check(&self, req: &ApprovalRequest) -> Decision;
}

pub struct ApprovalRequest {
    pub call_id: String,
    pub tool: String,
    pub input: Value,
    /// Tool-supplied summary for the prompt: the command, the path, the diff.
    pub preview: Option<Value>,
    pub hints: ToolHints,
}

pub enum Decision {
    Allow,
    /// Allow this and future calls matching `scope` for the rest of the session.
    AllowAlways(ApprovalScope),
    /// Reject; `reason` goes back to the model as the tool result.
    Deny { reason: Option<String> },
    /// Reject and end the turn (user pressed Esc on the prompt).
    Abort,
}
```

`AgentLoop::with_approver(Arc<dyn Approver>)`. With no approver every call is
allowed, which is today's behavior.

### Tool hints

`ToolDefinition` gains metadata the policy can read without knowing each
tool:

```rust
pub struct ToolHints {
    pub read_only: bool,     // read, glob, grep, list
    pub destructive: bool,   // write, edit, shell
    pub paths: Vec<PathBuf>, // extracted by the tool from its input
}

impl ToolDefinition {
    pub fn hints(self, f: impl Fn(&Value) -> ToolHints + Send + Sync + 'static) -> Self;
    pub fn preview(self, f: impl Fn(&Value) -> Option<Value> + Send + Sync + 'static) -> Self;
}
```

`preview` exists so the edit tool can produce the diff before it writes, and
the user approves the actual change rather than the raw arguments.

### Flow per call

```
ToolCallReady → hints/preview → approver.check()
   Allow / AllowAlways → ToolStarted → execute → ToolFinished
   Deny                → ToolFinished(is_error, "User denied: <reason>")
   Abort               → deny this and all remaining calls in the turn, end the turn (090)
```

Calls in one turn are checked one at a time, in order, so the user never sees
two prompts at once. Approved calls then execute concurrently as now.

### The interactive approver

Lives in core (210). It turns a check into an `AgentEvent::ApprovalRequested
{ call_id, .. }`, parks a `oneshot::Sender<Decision>` keyed by call id, and
completes when the frontend sends `Op::Approve { call_id, decision }`. Before
asking, it consults the policy (150). The policy usually answers without
involving the user at all.

## Tests

- Deny returns the denial text as an error result and the run continues.
- Abort with three calls: none executes, all three get results (keeps the
  history valid, see 070), the turn ends with `TurnStop::Aborted`.
- `AllowAlways(ToolName("shell"))` skips the prompt for the second shell call.
- No approver configured: behavior identical to current tests.

## References

- `crush/internal/permission/permission.go`: request/grant with
  session-persistent grants.
- `codex/codex-rs/protocol/src/approvals.rs`: approval request/decision types.
- `goose/crates/goose-agent`: tool confirmation router.
