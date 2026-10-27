# 090 — Interrupting a turn

**Status:** implemented — cancelling the run's `CancellationToken` ends it with `StopReason::Interrupted` and a valid history: partial text kept, half-streamed tool calls dropped, approvals read as abort, tool tokens fired with a grace period (`with_cancel_grace`), and an optional "[Request interrupted by user]" note. Deferred: the process-group kill test, which belongs to the shell tool (120); sync handlers run on `spawn_blocking` and cannot be interrupted.

**Crate:** `cosmos-llm-tool` · **Depends on:** 040, 060, 070 · **Blocks:** nothing

## Problem

The user presses Esc while the agent is streaming text, or while a shell
command is running. The turn has to stop right away, and the history must stay
valid so the next message can continue the conversation. Dropping the
`run_on` future does stop the work, but it leaves whatever was half-appended,
and a running child process can be orphaned.

## Design

`run_on` takes a `CancellationToken` (070). Each turn derives a child token,
and each tool call derives a child of that.

On cancel, by phase:

| Phase | Action | History afterwards |
|---|---|---|
| Model streaming, text only | Stop the stream (040). | Append an assistant message with the partial text, if non-empty. |
| Model streaming, tool call args in progress | Stop the stream. | Append partial text only; drop incomplete tool calls entirely (no `ToolUse` without arguments). |
| Waiting on approval | Resolve as `Abort`. | Assistant message with all calls, plus a result per call: `"Interrupted by user"`, `is_error: true`. |
| Tools executing | Cancel tool tokens; wait up to a grace period (e.g. 2 s) for handlers to return; then abandon. | Finished calls keep their real results. The rest get `"Interrupted by user"`. |

Then emit `TurnFinished { stop: TurnStop::Interrupted }` and return `Ok`, not
`Err`. An interrupt is a normal outcome.

Codex and Claude Code also add a short user-role note ("[Request interrupted
by user]") so the model knows on the next turn why its work stopped. Make
that an option, on by default.

### Tool cooperation

Async handlers get the token through `ToolContext` (060). The shell tool (120)
must kill the whole process group on cancel, not only the direct child, or
`npm test` keeps running its workers. Sync handlers cannot be interrupted;
document that, and run them on `spawn_blocking` so they at least do not block
the loop.

`Executor::execute_all` currently cannot be cancelled as a unit. It needs a
token parameter, or the loop needs to `select!` it against the token and then
fill in the missing results.

## Tests

- Cancel during text streaming: history ends with a partial assistant
  message; `validate_history` passes; the next `run_on` works.
- Cancel with one tool done and one sleeping: the first keeps its result, the
  second gets the interruption text, and the sleeping handler saw its token
  fire.
- Cancel while the stream is mid-way through tool arguments: no `ToolUse`
  block in history.
- Process-group kill: spawn `sh -c 'sleep 100 & sleep 100'`, cancel, assert
  both children exit (Unix only, `#[cfg(unix)]`).

## References

- `codex/codex-rs/core/src/tasks/mod.rs`: abort handling and
  `TurnAbortReason`.
- `pi-mono/packages/agent/src/agent-loop.ts`: abort signal through tools.
- `crush/internal/shell/background.go`: process group handling.
