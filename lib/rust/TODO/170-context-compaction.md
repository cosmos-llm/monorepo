# 170 — Compaction

**Status:** implemented — `SummarizingCompactor` (cut points that keep tool pairs whole, tail kept verbatim, recent user messages re-attached, chunked summaries that fold an earlier summary), `DropCompactor`, `CompactorPlugin` registry and `compaction` config, installed by default in `CoreBuilder`; tests with `MockProvider`. Deferred: the prune-old-tool-output-first step (`CompactionOutcome` can only replace a range with one message), retrying a summary call that itself overflows, and `AgentEvent`-level compaction progress.

**Crate:** `cosmos-llm-agent-core` · **Depends on:** 070, 160 · **Blocks:** long sessions

## Problem

A coding session fills a 200k window in an hour of tool use. Without
compaction it ends with a provider error. Dropping old messages without
summarizing loses the task, the decisions made so far, and which files
matter.

## Design

### Trigger

- Automatically, when 160 reports `Pressure::Compact` before a model call.
- Manually, from `Op::Compact { instructions: Option<String> }` (a `/compact
  focus on the auth bug` command).
- On `ContextLengthExceeded` from the provider, once.

### What survives

Following codex (`core/src/compact.rs`) and pi-mono:

1. System prompt and project instructions (190): regenerated, not summarized.
2. A **summary message** produced by the model from the history being
   dropped.
3. **Recent user messages** verbatim, newest first, up to a budget (codex
   uses 20k tokens). User intent is the thing summaries lose most often.
4. The **last N turns** verbatim (e.g. everything after a cut point chosen so
   the kept tail is ~20–30% of the window), cut only at a turn boundary so no
   `ToolUse` loses its `ToolResult` (070 invariants).

Everything else is replaced by the summary.

### Summary call

A separate completion, same model by default (configurable, since a cheaper
model is often fine):

- Input: the messages being dropped, with tool results truncated hard (they
  are the bulk, and the summary needs only what they showed).
- Prompt asks for a structured summary: goal, constraints from the user, key
  decisions, files touched and why, current state, next steps, open problems.
  Codex's prompt is in `codex-rs/prompts`; pi-mono's in
  `coding-agent/src/core/compaction/`.
- Output wrapped with a fixed prefix (codex: `SUMMARY_PREFIX`) so a later
  compaction recognizes and folds in an earlier summary instead of
  summarizing a summary of a summary.

The summary call can itself overflow on a very long history. Then summarize
in chunks and summarize the summaries.

### Events and persistence

`AgentEvent::CompactionStarted` / `CompactionFinished { before_tokens,
after_tokens }`. In the session log (200), compaction is a record that points
at the replaced range. The original messages stay in the log so the UI's
scrollback and `--resume` still show them. Only the model's view changes.

### Pluggable

```rust
#[async_trait]
pub trait Compactor: Send + Sync {
    async fn compact(&self, history: &[Message], ctx: &CompactCtx) -> Result<Vec<Message>, CompactError>;
}
```

Register it with `cosmos-llm-registry` like the other swappable pieces, so a
user can replace the strategy (e.g. drop-only, or tool-output pruning first)
without forking.

A cheaper first step that codex and opencode also use: before summarizing,
**prune old tool outputs**, replacing results older than K turns with "[output
pruned]". It is often enough on its own and costs no model call. Do it first,
and summarize only if still over budget.

## Tests

Mock completer that returns a canned summary:

- History over threshold → compacted history is under target, passes
  `validate_history`, contains the summary and the last user message
  verbatim.
- Second compaction folds the earlier summary (one summary message, not two).
- Pruning alone brings it under → no summary call made.
- Cut point never separates `ToolUse` from its `ToolResult`.

## References

- `codex/codex-rs/core/src/compact.rs`, `compact_retained_tool_outputs.rs`.
- `goose/crates/goose-context-management/src/summarize.rs`.
- `pi-mono/packages/coding-agent/src/core/compaction/`.
- `opencode/packages/opencode/src/session/compaction.ts`: prune + summarize.
