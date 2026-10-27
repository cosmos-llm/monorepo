# 130 — File tracking: staleness, turn diffs, undo

**Status:** implemented — `FileTracker` with read stamps, `check_fresh`, per-turn `TurnChanges`, `turn_diff`, `revert_turn` (skips later changes unless forced) and the `FileDiff`/`Hunk`/`TurnChanges`/`RevertReport` IR with JSON and trongo golden files in `cosmos-llm-coding-tools`; deferred: persisting the tracker and blobs with the session (200; blobs are in memory), the git snapshot second source, and calling `mark_all_unverified` from the shell tool (120).

**Crate:** `cosmos-llm-coding-tools` · **Depends on:** 100 · **Blocks:** 110

## Problem

Three things need a record of what the agent read and wrote:

1. **Staleness.** Editing a file the model has not read, or that changed on
   disk since it read it, is the most common source of broken edits. 110
   refuses those edits, which needs read timestamps.
2. **Turn diffs.** After a turn the UI shows "changed 3 files" with a
   combined diff.
3. **Undo.** "Revert what you just did" without involving git.

## Design

```rust
pub struct FileTracker {
    reads: HashMap<PathBuf, ReadStamp>,  // mtime + len + content hash at last read
    turns: Vec<TurnChanges>,
}

pub struct TurnChanges {
    pub turn: u32,
    /// Content before the first write in this turn; None = file did not exist.
    pub before: HashMap<PathBuf, Option<Vec<u8>>>,
}

impl FileTracker {
    pub fn record_read(&mut self, path: &Path, meta: &FsMeta, hash: u64);
    pub fn check_fresh(&self, path: &Path, meta: &FsMeta) -> Result<(), Stale>;
    pub fn record_write(&mut self, turn: u32, path: &Path, before: Option<Vec<u8>>);
    pub fn turn_diff(&self, fs: &dyn ToolFs, turn: u32) -> Vec<FileDiff>;
    pub fn revert_turn(&mut self, fs: &dyn ToolFs, turn: u32) -> Result<Vec<PathBuf>, RevertError>;
}
```

- One tracker per thread, shared by the tools through `Arc<Mutex<_>>`.
- `check_fresh` compares mtime and length first and hashes only when those
  differ. Some editors rewrite a file without changing its content.
- A write by the agent updates the read stamp, so a write followed by an edit
  does not trip the guard.
- The shell tool cannot be tracked per file. After a shell command, mark all
  stamps "unverified" so the next edit re-checks by hash. Codex's
  `turn_diff_tracker` takes a different route: it snapshots git state around
  the turn. Do both: git snapshot when in a repo, hashes otherwise.

### Undo

`revert_turn` restores `before` contents (deleting files that did not exist).
It refuses a path that was modified after the turn, by a later turn or by the
user, unless forced, and reports which paths it skipped. Undo for shell side
effects is out of scope; say so in the UI.

Persisted with the session (200) so undo survives a restart. Store `before`
blobs content-addressed on disk, not inline in the log.

### Git-backed alternative

Aider commits after every edit. That gives free history but clutters the
user's branch. Codex and Claude Code avoid touching the user's git state.
Recommendation: do not commit. Optionally create shadow snapshots with `git
stash create` (does not touch the index or the working tree) as a second undo
source in a repo.

## Tests

- Read → external modify → `check_fresh` is `Stale`.
- Read → agent write → `check_fresh` is fresh.
- Two edits to one file in one turn: `before` is the content before the
  first.
- `revert_turn` restores content and deletes created files.
- Revert after a later user edit → skipped and reported.

## References

- `crush/internal/filetracker/service.go`.
- `codex/codex-rs/core/src/turn_diff_tracker.rs`.
- `aider/aider/repo.py`: git commit per edit.
