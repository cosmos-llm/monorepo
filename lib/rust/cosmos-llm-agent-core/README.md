# cosmos-llm-agent-core

The headless core of a coding agent, between the tools and a frontend. A TUI,
an `exec` mode or an editor plugin drives it with `Op`s and draws `Event`s; it
never calls the loop, the thread or the tools directly.

```rust,no_run
use std::sync::Arc;
use cosmos_llm::Client;
use cosmos_llm_agent_core::{Core, CoreConfig, Event, LoadOptions};
use cosmos_llm_tool::Registry;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let cwd = std::env::current_dir()?;
let loaded = CoreConfig::load(&LoadOptions::new(&cwd))?;
let client = Arc::new(Client::new("anthropic", "key")?);
let (handle, mut events) = Core::builder(loaded.config)
    .completer(client)
    .tools(Registry::new())
    .warnings(loaded.warnings)
    .start()?;
handle.user_text("hello").await?;
while let Some(ev) = events.recv().await {
    if matches!(ev, Event::RunFinished { .. }) { break; }
}
# Ok(()) }
```

## Modules

| Module | What |
|---|---|
| `protocol` | `Op` (frontend to core), `Event` (core to frontend), `Envelope` (`v`) |
| `core` | `Core`, `CoreBuilder`, `CoreHandle`: one task owns the thread, turns run as child tasks |
| `approval` | `InteractiveApprover`: parks the loop on a oneshot per `call_id` until `Op::Approve` |
| `policy` | `PolicySpec`, `Rule`, `PolicyEngine`, `PolicyApprover`, shell classification |
| `record`, `session` | the JSONL session log: `Record`, `SessionStore`, `SessionWriter`, `SessionLog::load` |
| `budget` | `ContextBudget`, `Pressure`, `ContextTracker` (provider-anchored token estimate) |
| `compaction` | `CompactionPlan`, the `Compactor` seam, `SummarizingCompactor` (default) and `DropCompactor`, registered by name as `CompactorPlugin`; `compaction` config section |
| `exec`, `cli` | headless driver `run_exec` / `ExecResult`; `cli::exec_main()` behind the `cli` feature |
| `config`, `eval` | `CoreConfig`, loading `.trongo` / TOML, the sandbox and host builtins |
| `trongo` | `from_trongo` / `to_trongo` for the IR types |

## Approver chain

`PolicyApprover` runs first and answers allow or deny itself. Only calls it
cannot decide reach `InteractiveApprover`, which is the only thing that ever
produces `Event::ApprovalRequested`. `Core` assembles the chain; `AllowAlways`
answers are kept by the policy engine (so deny rules still win) and, with
`Op::Approve { remember: true }`, appended to `<repo>/.cosmos/permissions.trongo`.

`full_auto` is **not sandboxed**. The policy decides what to ask about; nothing
confines a command that is allowed to run.

## Config

`CoreConfig::load` merges `~/.config/cosmos/config.{trongo,toml,json}` and
`<repo>/.cosmos/config.{trongo,toml,json}`. `.trongo` files evaluate in a
sandbox (one read-only directory, a step budget, a timeout) with pure
host builtins: `env`, `render`, `git`, `model-info`, `platform`. The project
layer comes from a cloned repository and is untrusted by default: it can add
deny rules and tighten the mode, not allow anything. See `LoadOptions::trust_project`.

## Session logs

Append-only JSONL under `$XDG_DATA_HOME/cosmos/sessions/<project-hash>/`. Files
are `0600`, directories `0700`, and tool output can contain secrets
(`env`, `cat .env`); redaction is out of scope. Pasted images are stored as
blobs, not inline.

## Compaction

The default compactor summarizes: it cuts at a point that never separates a
`ToolUse` from its `ToolResult`, keeps the tail (about `tail_fraction` of the
window, at least `keep_turns` user turns) verbatim, asks the model for a
structured summary of the rest (tool output cut hard, chunked if it is too
long, an earlier summary folded in), and re-attaches the user's recent
messages word for word. Config: `compaction.{enabled, strategy, model,
keep_turns, tail_fraction, recent_user_tokens, tool_result_chars,
chunk_tokens, summary_max_tokens}`. `strategy` is a registry name
(`summarize`, `drop`); register your own `CompactorPlugin`, or pass a
compactor to `CoreBuilder::compactor`.

## Headless exec

`cargo run --example exec --features cli -- 'prompt' --output jsonl`. Output
`text` | `json` | `jsonl` | `trongo`; exit codes 0 finished, 1 error, 2 turn or
cost limit, 130 interrupted. Anything the policy would ask about is denied
with a reason the model sees. See `exec` module docs.

## Commands, hooks, subagents

`CoreBuilder::extensions(&LoadOptions, Option<&HookTrust>)` loads all three from
`~/.config/cosmos/` and `<repo>/.cosmos/` (project wins):

| What | Where | Front ends |
|---|---|---|
| Commands (`/review`, `/db:migrate`) | `commands/**.{trongo,md}` | `.trongo` map, or markdown with frontmatter |
| Agents (`task`'s `agent`) | `agents/**.{trongo,md}` | `.trongo` map, list, or `{:agents [...]}`; markdown |
| Hooks | `hooks` key of `config.{trongo,toml,json}`, or `hooks.trongo` | `{:post-tool-use [{:matcher "edit" :command "cargo fmt"}]}` |

Hooks speak Claude Code's contract (JSON on stdin, exit 2 blocks, JSON on
stdout), and project hooks run only after the user has trusted their hash
(`HookTrust`). The `task` tool starts a child agent with a subset of the parent's
tools through the parent's approver chain; see the `subagent` module docs.

## Not yet

`ir dump` / `ir check` commands, restoring the file tracker
from `file_changes` records, a SQLite index for session search, and OS
sandboxing for `full_auto`.

Golden files for every IR, in JSON and TrongoScript, are in `tests/golden/`.
Regenerate with `UPDATE_GOLDEN=1 cargo test --test golden` and read the diff.
