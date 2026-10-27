# cosmos-llm-coding-tools

Tools a coding agent needs, built on `cosmos-llm-tool`: a disk filesystem
behind the `ToolFs` trait, exact-match edit tools, a file tracker, and
ignore-aware search, and a shell tool with background jobs.

```rust,no_run
use std::sync::Arc;
use cosmos_llm_coding_tools::{CodingTools, DiskFs};
use cosmos_llm_tool::Registry;

let fs = Arc::new(DiskFs::new("/path/to/workspace").unwrap());
let tools = CodingTools::new(fs);
let mut registry = Registry::new();
tools.register_all(&mut registry);
// `tools.tracker()` is the shared FileTracker; call `begin_turn()` on it
// between turns so changes group per turn.
```

## Modules

| Module | What |
|---|---|
| `disk` | `DiskFs`, the `ToolFs` over a working tree |
| `tracker` | `FileTracker`, and the `FileDiff` / `TurnChanges` IR types |
| `edit` | pure edit functions |
| `tools` | `CodingTools`: `read`, `write`, `edit`, `multi_edit`, `glob`, `grep`, plus the shell tools (`file_tools()` for the first six only) |
| `search` | `glob`/`grep` implementations and `FileIndex` |
| `shell` | `bash`, `job_output`, `job_kill` (`ShellTools`); `parse_shell` and the `ShellCommandIr` parse tree the safety policy classifies; the `ShellRun` display IR |
| `trongo` | `from_trongo` / `to_trongo` (feature `trongo`, on by default) |

The IR types are plain serde. Golden files in JSON and TrongoScript live under
`tests/golden/`.

## Shell

`bash` runs each command as a fresh `bash -c` in its own process group, with
stdout and stderr merged. The working directory carries over between calls
(reset if a command ends outside the allowed roots); environment changes do
not. A timeout or interrupt kills the whole process group. Non-zero exit is a
result, not an error. After every command the tracker's stamps are marked
unverified, since a shell can change any file.

`parse_shell(&str) -> Result<ShellCommandIr, ShellParseError>` lowers a command
line to a tree of simple commands, pipelines, `&&`/`||` chains, subshells,
redirections and (recursively parsed) substitutions. Constructs it does not
model (`if`, `for`, `case`, `[[ ]]`, functions) become `Opaque`. Nothing is
sandboxed; the approval policy is the only gate.

## Not yet

`apply_patch`, git-backed snapshots, multiline grep, image reads,
persisting the tracker with a session, a PTY for the shell, and parsing
compound shell commands.
