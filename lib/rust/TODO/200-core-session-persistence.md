# 200 — Session persistence

**Status:** implemented — `Record`/`DisplayItem` IRs, `SessionStore`/`SessionWriter` (writer thread, `fsync` at turn end, `fs4` lock, `0600`/`0700`), `SessionLog::load` with torn-line and unknown-record tolerance, continue/resume/fork/delete/retention, image blobs, spill dir; golden files in JSON and trongo. Deferred: restoring the `FileTracker` from `file_changes` records (the tracker has no import API), the SQLite index, `session dump --format trongo`, and per-step logging inside a run (messages are logged when the run ends).

**Crate:** `cosmos-llm-agent-core` · **Depends on:** 010, 070 · **Blocks:** resume, 130 undo across restarts

## Problem

A session lives only in memory. Quitting loses it, so there is no
`--continue`, no `--resume`, no history to search, and no record of what the
agent did. `cosmos-llm-cache` is a key/value cache for completions and is not
the right shape for this.

## Design

### Format: append-only JSONL per session

Codex (`rollout`) and Claude Code both use this; crush uses SQLite. JSONL wins
here because:

- writes are appends, so a crash loses at most the last line,
- the file is readable with `jq`, which helps debugging a lot,
- no schema migrations, since each line carries a `type` and a `v`.

Add a small SQLite index later for listing and search if directory scans get
slow.

The `Record` enum is the IR (005); JSONL is only its storage encoding.
TrongoScript is not used on disk here, because an append log needs one
self-delimiting record per line and a reader that tolerates a torn last line.
`cosmos-code session dump <id> --format trongo` prints the log as trongo
data literals for reading and diffing, and golden-file tests cover both
encodings.

Layout:

```
$XDG_DATA_HOME/cosmos/sessions/
  <project-hash>/
    2026-10-27T14-03-11_<uuid>.jsonl
    <uuid>/tool-output/…      (180 spill files)
    <uuid>/blobs/<sha256>     (130 undo snapshots, pasted images)
```

`project-hash` is derived from the canonical workspace path, so `--continue`
finds the latest session for this directory.

### Records

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Record {
    SessionStart { v: u32, id: Uuid, cwd: PathBuf, model: String, started_at: DateTime<Utc>, parent: Option<Fork> },
    Message { id: MsgId, message: Message, usage: Option<Usage> },  // Message from 010
    ToolOutcome { call_id: String, display: Option<Value> },        // UI-only data from 060
    Compaction { replaces: Range<MsgId>, summary: MsgId },          // 170
    ModelChanged { model: String },
    Truncated { after: MsgId },                                     // 070 edit-and-resend
    FileChanges { turn: u32, before: Vec<(PathBuf, Option<BlobRef>)> }, // 130
}
```

Images inside messages are stored as blob refs, not base64 inline, or one
pasted screenshot bloats every resume.

### Replay

`Session::load(path) -> (Thread, Vec<DisplayItem>)` folds the records in
order. The model view applies `Compaction` and `Truncated`; the display view
keeps everything, so the UI can show the full transcript.

### Operations

- `continue`: latest session for this project.
- `resume <id>`: specific session; the listing shows the first user message,
  timestamp and turn count.
- `fork <id> [at <msg>]`: a new file whose `SessionStart.parent` points to the
  source. Copy records up to the fork point. Do not reference the source
  file; it may be deleted.
- `delete`, and a retention setting (e.g. 30 days) applied at startup.

### Writer

One writer task per session, fed by an mpsc channel. `fsync` at turn
boundaries, not per line. Take an exclusive lock on the file (`fs4`) so two
processes cannot append to one session.

### Secrets

Tool output can contain secrets (`env`, `cat .env`). Files are created
`0600`, the directory `0700`. Mention it in the docs. Redaction is out of
scope.

## Tests

- Write a session with tools, compaction and truncation; load it; model view
  and display view both match the in-memory originals.
- Kill mid-write (truncate the last line) → loads everything before it.
- Unknown record type from a newer version → skipped with a warning.
- Fork at message 3 → new file has messages 1–3 and a parent pointer.
- `continue` picks the newest file for the project.

## References

- `codex/codex-rs/rollout/src/recorder.rs`, `list.rs`.
- `crush/internal/db`, `crush/internal/session`: SQLite approach.
- `opencode/packages/opencode/src/session/`, `storage/`.
