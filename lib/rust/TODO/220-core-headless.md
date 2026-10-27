# 220 — Headless `exec` mode

**Status:** implemented — `run_exec`/`ExecResult`/`OutputFormat` and `cli::exec_main()` (feature `cli`, plus `examples/exec.rs`); text, json, jsonl and trongo output, exit codes 0/1/2/130, approvals denied with a reason, `--max-turns` via `max_steps`, `--max-cost` via usage cost. `trongo` is per-event literals (005), `json` the final object. Deferred: a bin target (the host binary mounts `exec_main`), `--output-schema`-style structured results, and a subprocess SIGINT test (interrupt is tested through the cancellation token).

**Crate:** `cosmos-llm-agent-core` (driver), shipped in the binary · **Depends on:** 210 · **Blocks:** nothing

## Problem

The same agent should run in CI, in scripts, and under the research harness
with no terminal UI: one prompt in, result out, exit code. If 210 is right,
this is a small driver, and it doubles as the easiest way to test the core end
to end against a real provider.

## Design

```
cosmos-code exec [OPTIONS] [PROMPT]       # PROMPT or stdin
  --model <id>
  --mode <read-only|auto-edit|full-auto>  # no `ask`: nobody to ask
  --output <text|json|jsonl|trongo>
  --max-turns <n>
  --max-cost <usd>                        # uses Session budgets / usage cost
  --resume <id> | --continue
  --cwd <dir>
```

- `text`: stream assistant text to stdout, tool activity as one-line
  summaries to stderr.
- `jsonl`: every `Event` (210) as a line on stdout. This is the scripting
  and harness interface.
- `json`: one object at the end: `{ result, session_id, turns, usage, cost,
  stop_reason }`.
- `trongo`: the same final object as trongo data literals, for feeding
  straight into a `.trongo` pipeline step with `read-file`.

The final object is an `ExecResult` IR (005), so all three structured outputs
are encodings of one type.

Approvals: with no frontend, anything the policy would `ask` about is denied
with a reason the model can see ("not permitted in non-interactive mode"). A
headless run must never block waiting for input.

Exit codes: `0` finished, `1` error, `2` stopped by `--max-turns` or
`--max-cost`, `130` interrupted (SIGINT is turned into `Op::Interrupt`).

The existing CLIs (`cosmos-llm-predict run`, etc.) put their command lines
behind a `cli` feature calling `cli::main()`. Follow the same pattern:
`cosmos-llm-agent-core` exposes `cli::exec_main()` and the binary is a few
lines.

## Tests

- Mock provider, `--output json`: shape matches the documented schema.
- A tool requiring approval under `read-only` → denied, run continues, exit
  0.
- `--max-turns 1` with a model that keeps calling tools → exit 2.
- SIGINT during a mock slow tool → exit 130 and the session file is valid.

## References

- `codex/codex-rs/exec/src/event_processor_with_jsonl_output.rs`.
- `crush/internal/cmd` (`crush run`).
- `pi-mono/packages/coding-agent/src/modes/print-mode.ts`.
