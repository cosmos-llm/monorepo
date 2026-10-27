# Changelog

## Unreleased

### Agent loop: events, threads, approvals, interrupts

Added:

- `AgentEvent` (`type`-tagged, `snake_case`, serde, `#[non_exhaustive]`),
  `ToolOutcome` (content blocks, `is_error`, UI-only `display`, `truncated`,
  `spill_path`) and `TurnStop`. `event_channel`, `RunHandles` and
  `AgentLoop::with_events` / `run_on` / `run_with` send events on a bounded
  `tokio::sync::mpsc` channel; a full channel makes the loop wait.
- `Completer::stream` (default replays `completion` as chunks, so existing
  completers keep working); `impl Completer for Client` streams when the
  provider can. `response_to_stream` is the shared helper.
- `ToolSpec` / `ToolHints`: the data-only half of a tool. `ToolDefinition::spec`
  and `from_spec`; `ToolDefinition::read_only` / `destructive` / `hints` /
  `preview` for approval metadata. The loop renders provider schemas from
  `ToolSpec`.
- `ToolContext` / `ProgressSink` and `ToolDefinition::context_handler`: a
  handler gets its call id, a cancellation token, a progress sink and a
  `set_display` hook. `Executor::execute_with` runs a tool with a context and
  puts sync handlers on `spawn_blocking`.
- `Thread`, `InputQueue`, `UserInput`, `ThreadStatus`, `validate_history` and
  `HistoryError`: a conversation that outlives a run, with input queued during
  a run delivered at the next safe point.
- `Approver`, `ApprovalRequest`, `Decision` (`allow`, `allow_always`, `deny`,
  `abort`), `ApprovalScope`, `AllowAll`, `FnApprover`, and
  `AgentLoop::with_approver`.
- Interrupts: cancelling the run token ends it with `StopReason::Interrupted`
  and a valid history. `with_interrupt_note`, `with_cancel_grace`.
- `TokenUsage` gains cache read/write and reasoning token counts, `add`, and
  serde. The loop calls `apply_default_cache_breakpoints` on every request.
- JSON and `.trongo` goldens for `ToolSpec`, `ToolOutcome`, `AgentEvent`,
  `ApprovalRequest`, `Decision`, `UserInput` and `ThreadStatus`.

Changed (behaviour):

- Tool results now go through `Executor::limit_result` with the loop's default
  `OutputLimit` (30,000 bytes, 2,000 lines) instead of being stringified
  whole; set `with_output_limit` to change it. A failing tool's result block
  now sets `is_error`.
- The model's final answer is appended to `RunOutcome::messages` (it was left
  out before), so the history is complete for a `Thread`.
- `AgentLoop::run_with` adds the system prompt to each request rather than
  storing it in the history, unless the history already starts with one;
  `run` still seeds it into the history as before.
- `StopReason` is `#[non_exhaustive]` and gains `Interrupted` and `Aborted`;
  downstream exhaustive matches need a wildcard arm.
- `Handler` gains a `Context` variant. `ParameterDef` derives `PartialEq`.
- `tokio` features `sync` and `macros`, and `tokio-util`, are now part of
  `async`.

Fixed:

- Removed an empty `if` in the `glob` preset's brace expansion (clippy).

### Added

- `output_limit` module: `OutputLimit` / `Truncate` (head, or head and tail)
  cap what a tool result adds to the model's context, cutting on line and
  UTF-8 boundaries with a note saying what was omitted. `limit_output`
  applies it; `SpillStore` keeps the full text in a per-session directory.
  Terminal trimmers: `strip_ansi`, `collapse_carriage_returns`, `dedupe_lines`,
  `truncate_long_lines`, `clean_terminal_output`.
- `ToolDefinition::output_limit` for a per-tool override, and
  `Executor::limit_result` to stringify and limit one result. The agent loop
  still needs to call it.
- `trongo` feature (default on): `trongo::from_trongo` / `to_trongo`, with
  JSON and `.trongo` golden files for `OutputLimit` under `tests/golden/`.
