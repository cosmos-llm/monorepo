# 120 — Shell tool

**Status:** implemented — `bash`, `job_output` and `job_kill` in `cosmos-llm-coding-tools::shell` (fresh `bash -c` per call in its own process group, merged streamed output, tracked cwd, group kill on timeout/interrupt, head+tail limit with spill via 180, background jobs, `mark_all_unverified` after each call), plus `parse_shell` lowering to the `ShellCommandIr` the policy (150) consumes and the `ShellRun` display IR; deferred: PTY mode, `ShellState` env overrides, parsing of compound commands (`if`/`for`/`case`/`[[`; they lower to `Opaque`), `ir dump`/`ir check` CLI, and a streaming-through-`AgentLoop` test.

**Crate:** `cosmos-llm-coding-tools` · **Depends on:** 060 (progress), 080, 090, 180 · **Blocks:** first milestone

## Problem

Builds, tests, git, package managers: a coding agent spends most of its time
running commands. Nothing exists for this yet.

## Design

### `bash` tool

| Param | Type | |
|---|---|---|
| `command` | string | required |
| `timeout_ms` | integer | default 120 000, max 600 000 |
| `description` | string | short label for the UI and approval prompt |
| `run_in_background` | bool | default false |

### Execution model

**One long-lived shell vs. a fresh process per call.** A persistent shell
(Claude Code's original approach) keeps `cd`, `export` and venv activation
between calls, but one hung command poisons it and output framing gets
fragile. Fresh processes are robust but forget state.

Recommendation, following crush and codex: **a fresh `bash -c` per call, with
tracked state.** Keep a `ShellState { cwd, env_overrides }` per session. After
each command, run `pwd` in the same invocation (append `; __rc=$?; printf
'\n%s\n' "$SENTINEL"; pwd; exit $__rc`), and update `cwd` if the new
directory is still inside the allowed roots. Otherwise reset it and tell the
model. Env changes are not carried over; document that in the tool
description.

Spawn details:

- `tokio::process::Command`, `bash -c` (fall back to `sh`), stdin closed,
  `TERM=dumb`, `NO_COLOR=1`, `PAGER=cat`, `GIT_PAGER=cat`, so commands do not
  wait on a pager or emit colors.
- New process group (`setsid` / `process_group(0)`). Timeout and cancel (090)
  send SIGTERM to the group, wait 2 s, then SIGKILL.
- Stdout and stderr merged in arrival order, as a terminal would show them.
- Strip ANSI escapes from what the model sees (`strip-ansi-escapes`).
- No PTY by default. Some tools behave differently without a TTY, but a PTY
  brings in raw-mode and resize handling. Add `portable-pty` later, as an
  option, if a real need turns up.

### Output

- Stream lines as `ToolOutputDelta` (060) so the UI shows live output.
- To the model: exit code, wall time, and output limited by 180 (head + tail,
  with the full output spilled to a file it can `read`).
- Non-zero exit is **not** a tool error. The model needs the output either
  way. Set `is_error` only for spawn failures and timeouts.

### Background jobs

`run_in_background: true` returns at once with a job id. Two companion tools:

- `job_output { job_id, since? }`: new output since the last read, and
  whether the job is still running.
- `job_kill { job_id }`.

Jobs belong to the thread and are killed when it ends. Cap concurrent jobs
(e.g. 8). This is how the agent runs a dev server and then tests against it.
See crush's `job_output.go` / `job_kill.go`.

### Approval

`hints.destructive = true` on every call; the policy (150) classifies the
actual command. `preview` returns `{ command, cwd, description }`.

## Tests

- `echo hi` → output "hi", exit 0.
- `exit 3` → exit 3, not an error.
- `cd sub && pwd`, then a separate `pwd` → second call runs in `sub`.
- `cd /` → state stays at root; the result says so.
- Timeout: `sleep 10` with 100 ms timeout → killed, result says timed out,
  and no `sleep` process remains.
- Background: start `sleep 0.2; echo done`, `job_output` until finished,
  output contains "done".
- 1 MB of output → truncated per 180, spill file exists with all of it.

## References

- `crush/internal/agent/tools/bash.go`, `bash.md.tpl`, `job_*.go`;
  `crush/internal/shell/`.
- `codex/codex-rs/shell-command`, `codex/codex-rs/exec-server`.
- `goose/crates/goose-mcp` developer `shell` tool.
- `mini-swe-agent`: fresh subprocess per command, nothing else.
