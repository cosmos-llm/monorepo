# Coding-agent support — TODO

What the Rust crates need before a coding TUI can sit on top of them. The TUI
itself is out of scope here, and so is MCP. Each file below is one piece of
supporting work: what is missing today, a proposed design, how to test it, and
where the reference agents in `../../../external/` solved the same problem.

Files count by tens so work can be inserted without renumbering.

## Read first

[005](005-principles-ir-and-trongoscript.md) sets two rules that override the
details in every other file:

- **Modules talk through defined IRs.** Every boundary is a named, data-only,
  serde type owned by one crate. Front ends lower into it, back ends lower
  out of it.
- **TrongoScript for config and intermediate data.** `.trongo` files evaluate
  to `serde_json::Value`, which deserializes straight into the IR. IR dumps
  and golden files use trongo data literals.

## Reading order

The groups are in dependency order. Within a group, files are mostly
independent.

### Client (`cosmos-llm`)

| | |
|---|---|
| [010](010-client-content-blocks.md) | `Message` holds a `String`. Replace it with typed content blocks (text, image, thinking, tool use, tool result). Most other client work depends on this. |
| [020](020-client-reasoning.md) | Request reasoning, stream thinking deltas, replay signed thinking blocks. |
| [030](030-client-prompt-caching.md) | Cache breakpoints, and cache read/write tokens on `Usage`. |
| [040](040-client-cancellation.md) | Cancel an in-flight request or stream. |
| [050](050-client-model-metadata.md) | Context window, output cap, and capability flags per model. |

### Agent loop (`cosmos-llm-tool`)

| | |
|---|---|
| [060](060-loop-streaming-events.md) | Stream the model turn through the loop, and emit owned events over a channel. |
| [070](070-loop-thread.md) | A conversation that outlives one `run()`, with queued user input. |
| [080](080-loop-approvals.md) | Ask a policy, and through it the user, before a tool runs. |
| [090](090-loop-interrupts.md) | Stop a turn cleanly and leave a history every provider will accept. |

### Coding tools (new crate, `cosmos-llm-coding-tools`)

| | |
|---|---|
| [100](100-tools-filesystem-backend.md) | A filesystem trait so presets run on disk as well as on the VFS. |
| [110](110-tools-edit.md) | Exact-match edit, multi-edit, and write with a read-before-write guard. |
| [120](120-tools-shell.md) | Shell tool: persistent cwd, timeout, background jobs, truncated output. |
| [130](130-tools-file-tracking.md) | Track reads and writes for staleness checks, turn diffs, and undo. |
| [140](140-tools-search.md) | Ignore-aware glob/grep on disk, and fuzzy path search for `@` mentions. |

### Safety

| | |
|---|---|
| [150](150-safety-policy.md) | Approval modes, command classification, path scoping. OS sandboxing deferred. |

### Context (`cosmos-llm-context` and the new core crate)

| | |
|---|---|
| [160](160-context-token-accounting.md) | Know how full the window is before sending. |
| [170](170-context-compaction.md) | Summarize old turns when the window fills. |
| [180](180-context-tool-output-limits.md) | Cap tool output; spill the rest where the model can page through it. |
| [190](190-context-project-instructions.md) | Load `AGENTS.md`/`CLAUDE.md` and environment facts into the system prompt. |

### Core (new crate, `cosmos-llm-agent-core`)

| | |
|---|---|
| [200](200-core-session-persistence.md) | Append-only session log; resume, continue, fork. |
| [210](210-core-protocol.md) | Ops in, events out: the boundary a TUI, headless mode, or editor plugin drives. |
| [220](220-core-headless.md) | `exec` mode: one prompt, JSONL or plain output, exit code. |
| [230](230-core-commands-and-hooks.md) | Slash commands from markdown files; pre/post tool hooks. |
| [240](240-core-subagents.md) | A `task` tool that runs a child loop with a narrower toolset. |

## Target layering

```
cosmos-llm                 client: blocks, reasoning, caching, cancel, model info
   ↑
cosmos-llm-tool            tools, registry, executor, AgentLoop (streaming, approvals)
   ↑
cosmos-llm-coding-tools    disk fs, edit, shell, file tracker, search     (new)
   ↑
cosmos-llm-agent-core      thread, compaction, persistence, protocol,     (new)
                           policy, commands, hooks, subagents
   ↑
cosmos-code (bin)          ratatui frontend + `exec`                      (not covered here)
```

IR types are plain serde everywhere. The library crates add trongo
loading, dumping and golden files behind a default-on `trongo` feature.
`cosmos-llm-agent-core` owns config evaluation, the sandbox, and host
builtins (005).

`cosmos-llm-context` stays where it is and gains the instruction loader (190).
The VFS presets in `cosmos-llm-tool` keep working; 100 puts them behind a trait
rather than replacing them.

## First milestone

The smallest set that gives a usable agent with a frontend on top:

1. 010 content blocks, 040 cancellation
2. 060 streaming events, 070 thread, 080 approvals, 090 interrupts
3. 100 filesystem backend, 110 edit, 120 shell
4. 150 policy (approval modes only), 180 output limits, 190 project instructions
5. 210 protocol

Everything else (reasoning, caching, compaction, persistence, commands, hooks,
subagents) is the second tier. Caching and compaction come first in that tier,
because long sessions get expensive and then stop working without them.

## Reference agents

| Agent | Language | Most useful for |
|---|---|---|
| `codex/codex-rs` | Rust | protocol, compaction, rollout persistence, apply-patch, exec policy, sandboxing |
| `goose/crates` | Rust | provider abstraction, context management, summarization |
| `crush/internal` | Go | tool set (edit, multiedit, bash, jobs), permissions, file tracker, shell truncation |
| `pi-mono/packages` | TS | the smallest complete stack: `ai` → `agent` → `coding-agent` |
| `opencode/packages` | TS | session model, tool prompts |
| `aider` | Python | git-backed undo, edit formats |
| `mini-swe-agent` | Python | how little a loop actually needs |
