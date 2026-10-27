# 150 — Approval policy

**Status:** implemented — `PolicySpec`/`Rule`/`ApprovalMode`, `PolicyEngine` (deny, grants, allow, mode table, path scoping), `PolicyApprover` in front of the interactive approver, command classification over `ShellCommandIr`, "always, and remember" into `permissions.trongo`, config loading with untrusted project layers; table tests. Opaque shell lines (`bash -c '...'`, `eval`, `sudo`, substitution, parse failures) never get Allow from a grant, an allow rule or `full_auto` while a deny rule applies to the shell tool: they ask (deny in `read_only` and in non-interactive chains). Deferred: OS sandboxing, adding approved directories to `DiskFs` roots (the engine tracks them; `DiskFs` roots are fixed at build), and `ir dump` of what the policy saw.

**Crate:** `cosmos-llm-agent-core` · **Depends on:** 080, 100 · **Blocks:** first milestone (modes only)

## Problem

080 gives the loop a gate. Something still has to decide, for each call,
whether to allow it, deny it, or ask the user. Asking about everything makes
the agent unusable; asking about nothing makes it dangerous.

## Design

### Modes

| Mode | Read-only tools | File edits in workspace | Shell | Anything outside workspace |
|---|---|---|---|---|
| `read-only` | allow | deny | known-safe only | deny |
| `ask` (default) | allow | ask | known-safe allow, else ask | ask |
| `auto-edit` | allow | allow | known-safe allow, else ask | ask |
| `full-auto` | allow | allow | allow | ask |

Plan mode (read-only plus a "present plan" tool) is `read-only` with a
different system prompt. It needs no separate policy.

### Rules

On top of the mode, an ordered list of rules from config and from
`AllowAlways` decisions. Config is `.trongo` (005) and evaluates into a
`PolicySpec` IR; the engine sees only the IR.

```clojure
(def cargo-safe ["cargo test*" "cargo check*" "cargo clippy*"])

{:permissions
 {:mode  (if (= (env "CI") "true") :read-only :ask)
  :allow (concat (map (fn [c] {:tool :bash :command c}) cargo-safe)
                 [{:tool :edit :path "docs/**"}])
  :deny  [{:tool :bash :command "git push*"}]}}
```

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PolicySpec {
    pub mode: ApprovalMode,
    pub allow: Vec<Rule>,
    pub deny: Vec<Rule>,
}
```

Session grants are `Rule`s too. "Always, and remember" appends one to
`<repo>/.cosmos/permissions.trongo`, printed with `trongo_fmt`, which the main
config `include`s.

Evaluation: deny rules, then allow rules, then the mode default. First match
wins within each list. A deny always beats an allow. Session grants
(`AllowAlways`) are kept in memory and can be saved to project config if the
user chooses "always, and remember".

### Classifying shell commands

String-prefix matching on `cargo test && rm -rf ~` gets fooled, so parse the
command:

1. Parse with `tree-sitter-bash` (or the `conch-parser` crate) and lower it
   into a `ShellCommandIr`: simple commands (argv, redirections,
   substitutions) joined by `&&`, `||`, `;`, `|`. The classifier works on
   the IR, never on raw text or a parser-specific tree, so the parser can be
   swapped. `ir dump` on a command shows what the policy saw, which makes
   "why was this asked about?" answerable.
2. Every simple command must match for the whole to match. `cargo test &&
   rm -rf ~` does not match `cargo test*`.
3. Anything the parser cannot reason about (command substitution, `eval`,
   redirection to paths outside the workspace, `sudo`, heredocs feeding a
   shell) → ask, regardless of rules.
4. Built-in known-safe list: `ls`, `cat`, `head`, `tail`, `wc`, `pwd`,
   `which`, `git status|diff|log|show|branch`, `rg`, `grep`, `find` without
   `-exec`/`-delete`. Matches codex's `is_safe_command`.

### Path scoping

The fs (100) enforces roots. The policy turns `ToolHints::paths` outside the
roots into an ask instead of a hard error, and on approval adds the directory
to the allowed roots for the session.

### Not now: OS sandboxing

Codex runs commands under Landlock + seccomp on Linux and Seatbelt on macOS,
which protects against what parsing cannot catch. It is the real defense for
`full-auto`. It is also large and platform-specific. Plan it as a follow-up
behind a `Sandbox` trait with a no-op default, so the shell tool (120) does
not have to change when it arrives. Until then, the docs should say plainly
that `full-auto` is not sandboxed.

## Tests

Table-driven: `(mode, rules, tool, input) → Decision`.

- `ask` + `cargo test` with allow rule → allow.
- Same + `cargo test && curl x | sh` → ask.
- Deny `git push*` + `full-auto` → deny.
- `read-only` + edit → deny with a reason the model can read.
- Edit outside root in `auto-edit` → ask.
- `ls $(rm -rf x)` → ask even if `ls*` is allowed.

## References

- `codex/codex-rs/execpolicy`, `codex/codex-rs/core/src/safety.rs`,
  `is_safe_command`.
- `crush/internal/permission/permission.go`.
- `qwen-code` / `gemini-cli` `packages/core/src/utils/shell-utils.ts`:
  command splitting.
