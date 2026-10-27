# Changelog

## Unreleased

- `DiskFs`: a `ToolFs` over a real working tree. Root-confined paths with
  symlink checks, optional extra roots, optional unrestricted reads, atomic
  writes.
- `FileTracker`: read stamps for staleness checks, per-turn change records,
  turn diffs and revert. IR types `FileDiff`, `Hunk`, `TurnChanges`,
  `RevertReport`.
- Tools: `read`, `write`, `edit`, `multi_edit` (exact match, whitespace
  fallback, CRLF preserved, read-before-write guard).
- Tools: `glob` (ignore-aware, newest first) and `grep` (ripgrep with an
  in-process fallback); `FileIndex` for fuzzy `@file` matching.
- Tools: `bash` (fresh `bash -c` per call in its own process group, merged
  output streamed as `ToolOutputDelta`, tracked working directory, timeout and
  interrupt kill the group, head+tail output limit), `job_output` and
  `job_kill` for background jobs (capped at 8, killed with the tool set).
  `CodingTools::all` now includes them; `file_tools()` is the old six.
- `shell::parse_shell` and the `ShellCommandIr` IR (simple commands,
  pipelines, chains, subshells, redirections, recursive substitutions,
  `Opaque` for unmodelled constructs); `ShellRun` display IR. Golden files for
  all three.
- `trongo` feature: `from_trongo` / `to_trongo` and golden files.
