# 180 — Tool output limits

**Status:** implemented — `output_limit` module in `cosmos-llm-tool` (`OutputLimit`, `Truncate`, `limit_output`, `SpillStore`, terminal trimmers), `ToolDefinition::output_limit` and `Executor::limit_result`, with trongo goldens; `agent.rs` now calls `limit_result` (060); deferred: wiring the spill directory to the session store (200), and the per-tool trimming inside `bash`/`grep`/`read` (coding-tools).

**Crate:** `cosmos-llm-tool` (mechanism) · **Depends on:** nothing · **Blocks:** 120, 140

## Problem

One `cat` of a lockfile or a `cargo build` with warnings can add 100k tokens
to the history. It stays there for every later turn and gets paid for each
time. Today `stringify(&value)` in `agent.rs` passes tool results through at
whatever size they are.

## Design

A limit applied by the loop to every tool result, so no individual tool can
forget it, plus helpers tools use for smarter cuts.

```rust
pub struct OutputLimit {
    pub max_bytes: usize,  // default 30_000 (~7.5k tokens)
    pub max_lines: usize,  // default 2_000
    pub strategy: Truncate,
}

pub enum Truncate {
    Head,
    /// Keep the start and the end; errors and summaries are usually at the end.
    HeadTail { head_ratio: f32 },
}
```

- Per-tool override on `ToolDefinition::output_limit(...)`. `read` paginates
  itself and wants a higher limit; `bash` wants `HeadTail`.
- Truncation cuts on line and UTF-8 boundaries, never mid-character.
- The truncation note says what was cut and how to get it: `[... 14,203
  lines omitted. Full output: .cosmos/tool-output/7f3a.txt — use read with
  offset/limit ...]`.

### Spill files

Full output is written to a per-session directory (inside the session store,
200, not the workspace, so it never shows up in `git status`). The `read`
tool must be allowed to read that directory regardless of workspace roots.
Clean it up when the session is deleted.

### Tool-specific trimming

Before the generic limit:

- `bash`: strip ANSI, collapse `\r` progress-bar redraws to the final state,
  dedupe runs of identical lines ("[previous line repeated 340 times]").
- `grep`: cut long lines (minified files).
- `read`: already paginated; ensure the default `limit` is sane (2000 lines)
  and lines are capped at ~2000 chars.

The UI still gets everything through `ToolOutputDelta` / `display` (060). The
limit only applies to what goes into the model's context.

## Tests

- 10 MB of output → result under `max_bytes`, ends with a note, spill file
  holds the original bytes.
- Multi-byte UTF-8 at the cut point → valid UTF-8.
- `HeadTail` keeps the last line (where `error:` usually is).
- Progress-bar collapse: `"10%\r20%\r100%\n"` → `"100%\n"`.

## References

- `crush/internal/shell/truncate.go`: line/byte caps and spill files.
- `codex/codex-rs/core/src/` truncation of exec output (`truncate.rs` /
  `exec_output`).
- `opencode/packages/opencode/src/tool/truncation.ts`.
