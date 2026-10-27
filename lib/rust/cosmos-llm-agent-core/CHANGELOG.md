# Changelog

## Unreleased

- Policy (150): an opaque shell line (`bash -c '...'`, `eval`, `sudo`,
  substitution, a parse failure) can no longer slip past a deny rule. When a
  deny rule applies to the shell tool, such a line is never allowed by a grant,
  an allow rule or `full_auto`: it asks, or is denied in `read_only` mode and by
  non-interactive approvers that resolve ask to deny. `PolicyEngine::derive`
  builds a stricter copy of an engine.
- Custom commands (230): `CommandSpec`, `CommandSet` (user and project
  `commands/` directories, `.trongo` or markdown, plus `CommandSourceReg`
  sources through `cosmos-llm-registry`), `$ARGUMENTS`/`$1`..`$9`, `@path`,
  `` !`cmd` `` (policy-gated), `Op::RunCommand`, `CoreHandle::list_commands`.
- Hooks (230): `HookSpec`, `HookRunner`, `HookPayload`/`HookResponse`,
  `pre_tool_use`/`post_tool_use` wrapped around tool handlers,
  `user_prompt_submit`, `stop` (capped at 3 continuations), `session_start`;
  project hooks trusted by content hash (`HookTrust`, `LoadedHooks`).
- Subagents (240): `AgentSpec`, `AgentSet`, the `task` tool,
  `Event::Subagent`, `SubagentOptions`. Children share the parent's approver
  chain, tracker and usage roll-up, and log separate transcripts.
- Compaction (170): `SummarizingCompactor` (default in `CoreBuilder` when a
  completer is set; `compaction.enabled = false` turns it off), `DropCompactor`,
  `CompactorPlugin` registry, `CompactionConfig`.
- Headless exec (220): `run_exec`, `ExecResult` IR, `OutputFormat`, and
  `cli::exec_main()` behind the `cli` feature.
- `Core`: `Op`/`Event` protocol over channels (also serde, versioned by
  `Envelope`), one task owning the `Thread`, turns as child tasks, an
  interactive approver keyed by `call_id`, per-run `RunFinished`.
- Session log (200): `Record` IR, JSONL writer thread with `fsync` at turn
  boundaries and an exclusive lock, replay into the model and display views,
  `continue` / `resume` / `fork`, retention, image blobs, `0600`/`0700`.
- Approval policy (150): `PolicySpec`, modes, deny/allow rules, session grants,
  path scoping, shell classification over `ShellCommandIr`, "always, and
  remember" into `permissions.trongo`.
- Token accounting (160): `ContextBudget`, `Pressure`, `ContextTracker`,
  `ContextUsage` events, overflow check before the call, retry after a
  provider context-length error; `Compactor` seam.
- `CoreConfig` loaded from `.trongo` or TOML with a sandboxed evaluator and the
  `env`, `render`, `git`, `model-info`, `platform` host builtins.
