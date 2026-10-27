# 230 — Custom commands and hooks

**Status:** implemented — `CommandSpec`/`CommandSet` from `.trongo` and markdown (user + project layers, `CommandSourceReg` via cosmos-llm-registry), `$ARGUMENTS`/`$1..$9`/`@path`/policy-gated `` !`cmd` ``, `Op::RunCommand`, `list_commands`; `HookSpec`/`HookRunner` for `pre_tool_use`, `post_tool_use` (wrapped around tool handlers, no loop change), `user_prompt_submit`, `stop` (capped), `session_start`, Claude Code stdin/stdout contract plus `:payload-format :trongo`, project hooks trusted by hash of the evaluated definitions. Deferred: argument-dependent `.trongo` templates (the prompt is a string expanded at run time), enforcing `allowed_tools`, reading `.claude/` and `.opencode/` directories, a frontend op to grant hook trust, and a hook's `updated_input` is not re-validated against the tool schema.

**Crate:** `cosmos-llm-agent-core` · **Depends on:** 210, 080 · **Blocks:** nothing

## Problem

Users want to package repeated prompts ("review this diff the way we do it")
and to run their own checks around tool calls (format after every edit, block
writes to `migrations/`). Both are the main extension points in Claude Code,
crush and codex after MCP.

## Commands

### Definition

Two front ends, one IR (`CommandSpec`, 005).

**`.trongo` (preferred).** The body is a TrongoScript template, so a command
can branch on its arguments without a shell:

```
~/.config/cosmos/commands/review.trongo   → /review        (user)
<repo>/.cosmos/commands/review.trongo     → /review        (project, wins)
<repo>/.cosmos/commands/db/migrate.trongo → /db:migrate
```

```clojure
{:description   "Review the staged diff"
 :args          "[focus]"
 :model         "claude-sonnet-5-5"
 :allowed-tools [:read :grep :bash]
 :prompt        (read-file "review.prompt.erb")}
```

**Markdown with frontmatter (compatibility).** So Claude Code and opencode
command files can be dropped in unchanged:

```markdown
---
description: Review the staged diff
args: "[focus]"
---
Review the output of `git diff --cached`. Focus on $ARGUMENTS.
```

Both lower into the same `CommandSpec`, and nothing downstream knows which
file it came from.

Expansion: `$ARGUMENTS`, `$1..$9`. `` !`cmd` `` runs a shell command and
inlines its output (subject to policy, 150). `@path` inlines a file. The
result becomes a normal `UserInput`.

Core exposes `list_commands()` for the frontend's completion popup and
handles `Op::RunCommand`. Built-in commands (`/compact`, `/model`, `/undo`,
`/clear`) are ops the frontend maps directly. They are not markdown files.

Register the loader through `cosmos-llm-registry` so other sources (e.g. a
team server) can be added without a fork.

## Hooks

### Events

| Hook | When | Can |
|---|---|---|
| `PreToolUse` | after approval, before execution | block with a reason, rewrite input |
| `PostToolUse` | after execution | append to the result the model sees |
| `UserPromptSubmit` | before input reaches the model | block, add context |
| `Stop` | turn ends | ask the agent to continue (e.g. tests still failing) |
| `SessionStart` | thread created | add context |

### Configuration

In the main config, lowered into `HookSpec` (005):

```clojure
{:hooks
 {:post-tool-use [{:matcher    "edit|multi_edit|write"
                   :command    "cargo fmt -- $COSMOS_FILE_PATHS"
                   :timeout-ms 10000}]}}
```

The payload and response are IRs too (`HookPayload`, `HookResponse`). The hook
gets a JSON payload on stdin by default, or trongo data literals with
`:payload-format :trongo` (`{ event, tool, input, result,
session_id, cwd }`). It answers with an exit code (0 ok, 2 block, other =
non-blocking error shown to the user) and optional JSON on stdout (`{
decision, reason, additional_context, updated_input }`). This is Claude
Code's contract, and copying it lets existing hook scripts work unchanged.

### Placement

`PreToolUse`/`PostToolUse` wrap tool execution inside the loop (060), so the
hook runner is another optional component on `AgentLoop` next to the approver
(080). The rest live in the core's thread driver.

Hooks from project config run arbitrary commands. They must be shown to the
user and trusted once per repo before they run. Store trust by config file
hash.

## Tests

- Command with `$ARGUMENTS` and `@file` expands correctly; project overrides
  user.
- `PreToolUse` exit 2 → tool not run, reason in the result.
- `PostToolUse` stdout `additional_context` → appended to the tool result.
- `Stop` hook returning `block` → loop runs another turn, capped so a broken
  hook cannot loop forever.
- Untrusted project hook → not run, `Warning` event emitted.

## References

- `codex/codex-rs/hooks`, `codex/codex-rs/skills`.
- `crush/internal/hooks`, `crush/internal/commands`, `crush/internal/skills`.
- `opencode/packages/opencode/src/command/`.
