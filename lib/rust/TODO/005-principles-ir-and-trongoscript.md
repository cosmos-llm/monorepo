# 005 — Principles: IRs between modules, TrongoScript for config and data

These two rules apply to every file in this directory. Where a design below
conflicts with them, this file wins, and the other file should be fixed.

## 1. Modules talk through defined IRs

Every boundary between modules is a named intermediate representation: a
plain data type, defined once, owned by one crate. A module takes an IR in and
hands an IR out. It never reaches into another module's internals, and it
never passes a closure or trait object across a boundary where data would do.

The shape is a compiler pipeline. Front ends (config files, CLI flags,
markdown frontmatter, provider responses, user keystrokes) **lower into** an
IR. Back ends (provider wire JSON, the TUI renderer, the session log)
**lower out of** one. Passes in the middle (compaction, policy evaluation,
truncation) are functions IR → IR.

### Rules for an IR type

- **Data only.** `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`.
  No `Arc<dyn …>`, no handles, no borrowed lifetimes. If something needs a
  live resource, the IR carries an id and the receiver looks it up.
- **Owned by one crate**, the lowest one that needs it. Everyone else depends
  on that crate for the type.
- **`#[non_exhaustive]`** on public enums and structs, so adding a variant or
  field is not a breaking change.
- **Versioned when persisted or sent across processes.** The top-level
  envelope carries `v: u32`. Readers skip unknown variants with a warning
  instead of failing.
- **Serde shape is the contract.** Tagged enums (`#[serde(tag = "type")]`),
  `snake_case`, no `untagged` (it makes errors unreadable and order-dependent).
- **Golden files.** Each IR has a fixture in both JSON and TrongoScript under
  `tests/golden/`, and a round-trip test through both.
- **Inspectable from the shell.** In keeping with the existing crates' `cli`
  features, each crate that owns an IR can dump and load it (`… ir dump`, `…
  ir check FILE`), so a pipeline stage can be run or debugged on its own.
  `--format trongo` is the default for `dump`; `json` is available for `jq`.

### The IRs in this plan

| IR | Owner | Produced by | Consumed by | Doc |
|---|---|---|---|---|
| `CompletionRequest`, `Message`, `ContentBlock` | `cosmos-llm` | thread, compactor | providers (lowered to wire JSON) | 010 |
| `StreamChunk`, `ThinkingDelta` | `cosmos-llm` | providers | accumulator, loop | 020, 060 |
| `ModelInfo` | `cosmos-llm` | provider APIs, built-in table, config | budget, policy, tool selection | 050 |
| `ToolSpec` (schema half of `ToolDefinition`) | `cosmos-llm-tool` | tool crates, `.trongo` definitions | loop, providers | 060 |
| `AgentEvent`, `ToolOutcome` | `cosmos-llm-tool` | loop, tools | core, session log, frontends | 060 |
| `ApprovalRequest`, `Decision` | `cosmos-llm-tool` | loop | policy, frontends | 080 |
| `FileDiff`, `TurnChanges` | `cosmos-llm-coding-tools` | edit tools, tracker | UI, undo, session log | 110, 130 |
| `ShellCommandIr` (parsed command tree) | `cosmos-llm-coding-tools` | shell parser | policy | 150 |
| `PolicySpec` | `cosmos-llm-agent-core` | config | policy engine | 150 |
| `PromptPlan` (ordered sections + cache hints) | `cosmos-llm-context` | instruction loader, env probe | renderer → `CompletionRequest` | 190 |
| `CompactionPlan` | `cosmos-llm-agent-core` | budget check | compactor | 160, 170 |
| `Record` (session log line) | `cosmos-llm-agent-core` | thread driver | log writer, replay | 200 |
| `Op`, `Event` | `cosmos-llm-agent-core` | frontends / core | core / frontends | 210 |
| `AgentSpec`, `CommandSpec`, `HookSpec` | `cosmos-llm-agent-core` | `.trongo` / markdown files | core | 230, 240 |

`ToolDefinition` today mixes the IR (name, description, parameters) with the
handler closure. Split it: `ToolSpec` is the IR, and `ToolDefinition` is
`ToolSpec` + handler. Providers, policies and session logs only ever see
`ToolSpec`.

The same goes for prompts. Rather than build the system prompt as a string,
190 builds a `PromptPlan`, and rendering to text happens last. Compaction,
caching (030) and token estimates (160) all work on the plan, where the
section boundaries are still visible.

## 2. TrongoScript for configuration and intermediate data

[TrongoScript](https://github.com/durable-oss/trongoscript) (vendored for
reading at `external/trongoscript`) is the default format wherever this plan
needs configuration, definitions, or a human-readable form of intermediate
data.

### Why it fits the IR rule

TrongoScript values *are* `serde_json::Value`
(`trongoscript/src/value.rs`). So:

- **Config → IR:** evaluate a `.trongo` file to a `Value`, then
  `serde_json::from_value::<PolicySpec>(v)`. No adapter layer. The IR's serde
  shape is the config schema.
- **IR → text:** `serde_json::to_value(&ir)` →
  `trongoscript::trongo_fmt` prints it as data literals (`{:type :edit :path
  "src/lib.rs"}`). That output is inert: evaluating it yields the same value.
  This is the readable dump format for every `ir dump` command and golden
  file.
- **Logic where config needs it:** `(if (= env "ci") …)`, `include`,
  functions that produce families of agent definitions, prompt templates in
  the template mode. All of it evaluates to plain data before any module sees
  it, so no module has to know TrongoScript exists.

### Where it is used

| Use | Format | Doc |
|---|---|---|
| Main config (`~/.config/cosmos/config.trongo`, `<repo>/.cosmos/config.trongo`) | `.trongo`, evaluated → `CoreConfig` IR | throughout |
| Model overrides | `.trongo` → `ModelInfo` | 050 |
| Permission rules | `.trongo` → `PolicySpec` | 150 |
| Agent and command definitions | `.trongo` (markdown body allowed via `read-file`) | 230, 240 |
| Hooks config | `.trongo` → `HookSpec` | 230 |
| System prompt templates | TrongoScript template mode | 190 |
| IR dumps, golden files, fixtures | trongo data literals | all |
| `exec --output trongo` | trongo data literals per event | 220 |

### Where it is not

- **Hot paths and machine-only channels.** Provider wire format is whatever
  the provider demands. The in-process event channel passes Rust values, not
  text. The session log (200) stays JSONL because it is append-only and
  crash-tolerant line by line. A `trongo convert` or `ir dump` gives the
  readable view.
- **Compatibility formats.** `AGENTS.md`/`CLAUDE.md` are markdown. Hook stdin
  payloads stay JSON so Claude Code hook scripts work unchanged; a hook can
  opt in to trongo with `:payload-format :trongo`.
- **TOML/JSON still load.** `trongo embed` already handles TOML/YAML/JSON
  trees with `!trongo` snippets. Accept `config.toml` too, and lower it into
  the same IR. A user with a five-line config should not have to learn a Lisp.

### Sandbox posture

TrongoScript has no ambient network, subprocess or write capability, and reads
only through a host-built `Sandbox`. That is what makes evaluating
`<repo>/.cosmos/*.trongo` from a cloned repository safe. Keep it so:

- Mount only `<repo>/.cosmos/` and `~/.config/cosmos/`, read-only. Never `/`
  or the whole repo.
- Host builtins exposed to config are pure lookups. None may run a command,
  write a file, or open a socket. Anything that needs a capability goes in a
  hook (230), which is trust-gated.

### Host builtins the core registers

The stdlib has `include`, `read-file`, `map`, `concat`, `str` and so on, but
nothing host-specific. The examples in 150, 190 and 240 assume these, which
the core adds with `EvalConfig::with_host_builtin`:

| Builtin | Returns | Notes |
|---|---|---|
| `(env "NAME")` | string or nil | Allowlist of variables (`CI`, `HOME`, `USER`, `TERM`, `COSMOS_*`); secrets like `*_API_KEY` are never visible. |
| `(render "file.erb" data)` | string | Runs the trongoscript template renderer on a sandboxed file with `data` in scope. |
| `(git :branch)`, `(git :root)` | string or nil | Read from data gathered before evaluation; no subprocess during eval. |
| `(model-info "id")` | map or nil | From the 050 table. |
| `(platform)` | map | `{:os … :arch … :shell …}`. |

`git` and `platform` read a snapshot taken before evaluation starts. That keeps
evaluation pure and repeatable, and keeps it in line with the no-subprocess
rule.
- Evaluation has a time and size limit (`SandboxLimits`), so a hostile
  `.trongo` cannot hang startup.

### Dependency

`trongoscript` is on crates.io (`trongoscript = "0.1"`), so any crate here can
depend on it by version. A local `[patch.crates-io]` pointing at
`external/trongoscript` is fine while co-developing, but is never committed to
`main` (same rule as the harness's `PLAN/010.030.000-dependencies.md`).

Even so, keep the layering:

- **IR types never mention trongoscript.** They are plain serde types. A
  consumer of `cosmos-llm` who only wants the client does not pay for a DSL,
  and an IR stays loadable from JSON with no evaluator involved.
- **Library crates** (`cosmos-llm`, `cosmos-llm-tool`,
  `cosmos-llm-coding-tools`, `cosmos-llm-context`) take `trongoscript`
  behind a `trongo` feature, on by default. It provides
  `from_trongo`/`to_trongo` helpers for their IRs, `--format trongo` on their
  CLIs, and trongo golden files.
- **`cosmos-llm-agent-core`** depends on it unconditionally. It owns config
  evaluation, the sandbox, and the host builtins below.

## Checklist for every TODO file

- [ ] Each new boundary has a named IR type, listed in the table above.
- [ ] Each config surface lowers from `.trongo` (and TOML) into an IR.
- [ ] Each IR has JSON + trongo golden files.
- [ ] No IR carries a closure, a trait object, or a borrowed reference.
