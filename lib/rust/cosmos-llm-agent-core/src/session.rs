//! Session persistence (200): append-only JSONL, replay, continue, resume,
//! fork.
//!
//! ```text
//! <root>/                          $XDG_DATA_HOME/cosmos/sessions
//!   <project-hash>/                sha256 of the canonical workspace path
//!     2026-10-27T14-03-11_<uuid>.jsonl
//!     <uuid>/tool-output/…         spill files (180)
//!     <uuid>/blobs/<sha256>        pasted images, undo snapshots (130)
//! ```
//!
//! Tool output can contain secrets (`env`, `cat .env`), so log files are
//! created `0600` and directories `0700`. Redaction is out of scope.

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use cosmos_llm::{ContentBlock, ImageSource, Message, Role};
use cosmos_llm_tool::{validate_history, HistoryError, Thread, ThreadId, TokenUsage};
use fs4::fs_std::FileExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::error::{CoreError, Result};
use crate::record::{BlobRef, DisplayItem, FileChange, Fork, MsgId, Record, RECORD_VERSION};

const BLOB_PREFIX: &str = "blob:";

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(unix)]
fn private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
fn private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

fn open_private(path: &Path, create_new: bool) -> std::io::Result<File> {
    let mut o = OpenOptions::new();
    o.append(true);
    if create_new {
        o.create_new(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)
}

/// The directory of auxiliary files for the session at `log`.
pub fn aux_dir(log: &Path) -> PathBuf {
    let stem = log
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session");
    let id = stem.rsplit('_').next().unwrap_or(stem);
    log.parent().unwrap_or(Path::new(".")).join(id)
}

/// Where spill files for the session at `log` go (180).
pub fn tool_output_dir(log: &Path) -> PathBuf {
    aux_dir(log).join("tool-output")
}

/// Writes `bytes` into the session's blob store; returns the reference.
pub fn write_blob(log: &Path, bytes: &[u8]) -> Result<BlobRef> {
    let dir = aux_dir(log).join("blobs");
    private_dir(&dir).map_err(|e| CoreError::io(&dir, e))?;
    let sha = sha256_hex(bytes);
    let path = dir.join(&sha);
    if !path.exists() {
        std::fs::write(&path, bytes).map_err(|e| CoreError::io(&path, e))?;
    }
    Ok(BlobRef(sha))
}

/// Reads a blob back.
pub fn read_blob(log: &Path, blob: &BlobRef) -> Result<Vec<u8>> {
    let path = aux_dir(log).join("blobs").join(&blob.0);
    std::fs::read(&path).map_err(|e| CoreError::io(path, e))
}

/// Replaces inline base64 images with blob references.
fn externalize_images(log: &Path, message: &Message) -> Result<Message> {
    let mut out = message.clone();
    for block in &mut out.content {
        externalize_block(log, block)?;
    }
    Ok(out)
}

fn externalize_block(log: &Path, block: &mut ContentBlock) -> Result<()> {
    match block {
        ContentBlock::Image {
            source: ImageSource::Base64 { data, .. },
            ..
        } if !data.starts_with(BLOB_PREFIX) => {
            let blob = write_blob(log, data.as_bytes())?;
            *data = format!("{BLOB_PREFIX}{}", blob.0);
        }
        ContentBlock::ToolResult { content, .. } => {
            for b in content {
                externalize_block(log, b)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn internalize_block(log: &Path, block: &mut ContentBlock, warnings: &mut Vec<String>) {
    match block {
        ContentBlock::Image {
            source: ImageSource::Base64 { data, .. },
            ..
        } => {
            if let Some(sha) = data.strip_prefix(BLOB_PREFIX) {
                match read_blob(log, &BlobRef(sha.to_owned())) {
                    Ok(bytes) => *data = String::from_utf8_lossy(&bytes).into_owned(),
                    Err(e) => {
                        warnings.push(format!("image blob {sha} is missing: {e}"));
                        *block = ContentBlock::text("[image unavailable]");
                    }
                }
            }
        }
        ContentBlock::ToolResult { content, .. } => {
            for b in content {
                internalize_block(log, b, warnings);
            }
        }
        _ => {}
    }
}

enum Cmd {
    Line(String),
    Sync(oneshot::Sender<std::io::Result<()>>),
    Close(oneshot::Sender<std::io::Result<()>>),
}

/// Appends records to one session file from a dedicated writer thread.
///
/// Records go through a channel, so the agent never waits on the disk.
/// [`sync`](SessionWriter::sync) (call it at turn boundaries) flushes and
/// `fsync`s. The file is held under an exclusive lock so two processes cannot
/// append to one session.
#[derive(Debug)]
pub struct SessionWriter {
    tx: std_mpsc::Sender<Cmd>,
    path: PathBuf,
    id: Uuid,
}

impl SessionWriter {
    fn spawn(path: PathBuf, id: Uuid, mut file: File) -> Self {
        let (tx, rx) = std_mpsc::channel::<Cmd>();
        let name = format!("cosmos-session-{id}");
        let _ = std::thread::Builder::new().name(name).spawn(move || {
            let mut failed: Option<std::io::Error> = None;
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Cmd::Line(line) => {
                        if failed.is_none() {
                            if let Err(e) = file.write_all(line.as_bytes()) {
                                failed = Some(e);
                            }
                        }
                    }
                    Cmd::Sync(done) => {
                        let r = match failed.take() {
                            Some(e) => Err(e),
                            None => file.flush().and_then(|()| file.sync_data()),
                        };
                        let _ = done.send(r);
                    }
                    Cmd::Close(done) => {
                        let r = file.flush().and_then(|()| file.sync_all());
                        let _ = FileExt::unlock(&file);
                        let _ = done.send(r);
                        return;
                    }
                }
            }
            let _ = FileExt::unlock(&file);
        });
        Self { tx, path, id }
    }

    /// The log file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The session id.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Queues a record. Never blocks.
    pub fn append(&self, record: &Record) -> Result<()> {
        let mut line = serde_json::to_string(record)?;
        line.push('\n');
        self.tx
            .send(Cmd::Line(line))
            .map_err(|_| CoreError::Session("session writer has stopped".into()))
    }

    /// Queues a message record, moving inline images into the blob store.
    pub fn append_message(
        &self,
        id: MsgId,
        message: &Message,
        usage: Option<TokenUsage>,
    ) -> Result<()> {
        let message = externalize_images(&self.path, message)?;
        self.append(&Record::Message { id, message, usage })
    }

    /// Flushes and `fsync`s everything queued so far.
    pub async fn sync(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Sync(tx))
            .map_err(|_| CoreError::Session("session writer has stopped".into()))?;
        rx.await
            .map_err(|_| CoreError::Session("session writer has stopped".into()))?
            .map_err(|e| CoreError::io(&self.path, e))
    }

    /// Flushes, `fsync`s, releases the lock and stops the writer.
    pub async fn close(self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Cmd::Close(tx)).is_err() {
            return Ok(());
        }
        rx.await
            .map_err(|_| CoreError::Session("session writer has stopped".into()))?
            .map_err(|e| CoreError::io(&self.path, e))
    }
}

/// What the picker shows for a session.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SessionSummary {
    /// The session id.
    pub id: Uuid,
    /// The log file.
    pub path: PathBuf,
    /// The workspace.
    pub cwd: PathBuf,
    /// When it began.
    pub started_at: DateTime<Utc>,
    /// The model at the start.
    pub model: String,
    /// The first thing the user typed.
    pub first_user_message: Option<String>,
    /// User turns in the model's view.
    pub turns: usize,
    /// Set on a fork.
    pub parent: Option<Fork>,
}

/// A session replayed from its log.
#[derive(Debug)]
#[non_exhaustive]
pub struct LoadedSession {
    /// The session id.
    pub id: Uuid,
    /// The log file.
    pub path: PathBuf,
    /// The workspace recorded at the start.
    pub cwd: PathBuf,
    /// When it began.
    pub started_at: DateTime<Utc>,
    /// Set on a fork.
    pub parent: Option<Fork>,
    /// The model now (the last `ModelChanged`, else the start).
    pub model: String,
    /// The model's view: compaction and truncation applied.
    pub thread: Thread,
    /// The id of each message in `thread`'s history, in order.
    pub msg_ids: Vec<MsgId>,
    /// The full transcript.
    pub display: Vec<DisplayItem>,
    /// The next unused message id.
    pub next_id: MsgId,
    /// File changes by turn, for undo across restarts.
    pub file_changes: Vec<(u32, Vec<FileChange>)>,
    /// Torn lines, unknown records, missing blobs, repaired history.
    pub warnings: Vec<String>,
}

fn is_user_turn(m: &Message) -> bool {
    m.role == Role::User && !m.is_tool_result()
}

fn text_of(m: &Message) -> String {
    m.content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parses log text into records. A line that is not a known record (torn last
/// line, newer record type) becomes a warning.
pub fn parse_records(text: &str) -> (Vec<Record>, Vec<String>) {
    let mut records = Vec::new();
    let mut warnings = Vec::new();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let last = lines.len().saturating_sub(1);
    for (i, line) in lines.iter().enumerate() {
        match serde_json::from_str::<Value>(line) {
            Err(_) => warnings.push(if i == last {
                "ignored a torn last line".to_owned()
            } else {
                format!("ignored an unreadable line {}", i + 1)
            }),
            Ok(v) => match serde_json::from_value::<Record>(v.clone()) {
                Ok(r) => records.push(r),
                Err(e) => {
                    let kind = v.get("type").and_then(Value::as_str).unwrap_or("?");
                    warnings.push(format!("skipped record {} (type {kind}): {e}", i + 1));
                }
            },
        }
    }
    (records, warnings)
}

struct Folded {
    history: Vec<(MsgId, Message)>,
    display: Vec<DisplayItem>,
    model: String,
    next_id: MsgId,
    file_changes: Vec<(u32, Vec<FileChange>)>,
}

fn fold(records: &[Record], log: &Path, warnings: &mut Vec<String>) -> Folded {
    let mut all: HashMap<MsgId, (Message, Option<TokenUsage>)> = HashMap::new();
    let mut order: Vec<MsgId> = Vec::new();
    let mut history: Vec<MsgId> = Vec::new();
    let mut display: Vec<DisplayItem> = Vec::new();
    let mut model = String::new();
    let mut next_id = 1;
    let mut file_changes = Vec::new();

    for r in records {
        match r {
            Record::SessionStart { model: m, .. } => model = m.clone(),
            Record::Message { id, message, usage } => {
                let mut message = message.clone();
                for b in &mut message.content {
                    internalize_block(log, b, warnings);
                }
                next_id = next_id.max(id + 1);
                all.insert(*id, (message, *usage));
                order.push(*id);
                // A compaction summary is logged before the record that places
                // it, so it joins the thread only when that record arrives.
                history.push(*id);
            }
            Record::ToolOutcome {
                call_id,
                display: d,
            } => {
                if let Some(d) = d {
                    display.push(DisplayItem::ToolDisplay {
                        call_id: call_id.clone(),
                        display: d.clone(),
                    });
                }
            }
            Record::Compaction {
                replaces,
                summary,
                except,
            } => {
                // The summary was appended as an ordinary message; lift it out
                // and put it where the first replaced message was.
                history.retain(|id| id != summary);
                let hit: Vec<usize> = history
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| replaces.contains(id) && !except.contains(id))
                    .map(|(i, _)| i)
                    .collect();
                if let Some(&first) = hit.first() {
                    let gone: HashSet<usize> = hit.iter().copied().collect();
                    let mut next = Vec::with_capacity(history.len());
                    for (i, id) in history.iter().enumerate() {
                        if i == first {
                            next.push(*summary);
                        }
                        if !gone.contains(&i) {
                            next.push(*id);
                        }
                    }
                    history = next;
                } else {
                    warnings.push("a compaction record replaced nothing".into());
                }
                display.push(DisplayItem::Compaction {
                    replaces: replaces.clone(),
                    summary: *summary,
                });
            }
            Record::ModelChanged { model: m } => {
                model = m.clone();
                display.push(DisplayItem::ModelChanged { model: m.clone() });
            }
            Record::Truncated { after } => {
                let keep = if *after == 0 {
                    0
                } else {
                    history
                        .iter()
                        .position(|id| id == after)
                        .map_or(history.len(), |p| p + 1)
                };
                history.truncate(keep);
                display.push(DisplayItem::Truncated { after: *after });
            }
            Record::FileChanges { turn, files } => file_changes.push((*turn, files.clone())),
        }
    }

    // The display view: every message once, in log order, flagged by whether
    // the model still sees it. Interleave with the other items by rebuilding
    // from the record order.
    let in_context: HashSet<MsgId> = history.iter().copied().collect();
    let mut items = Vec::with_capacity(display.len() + order.len());
    let mut markers = display.into_iter();
    let mut msgs = order.iter();
    // Messages and markers were collected separately; merge by replaying the
    // records once more to recover their relative order.
    for r in records {
        match r {
            Record::Message { id, .. } => {
                if msgs.next().is_some() {
                    if let Some((message, usage)) = all.get(id) {
                        items.push(DisplayItem::Message {
                            id: *id,
                            message: message.clone(),
                            usage: *usage,
                            in_context: in_context.contains(id),
                        });
                    }
                }
            }
            Record::ToolOutcome {
                display: Some(_), ..
            }
            | Record::Compaction { .. }
            | Record::ModelChanged { .. }
            | Record::Truncated { .. } => {
                if let Some(m) = markers.next() {
                    items.push(m);
                }
            }
            _ => {}
        }
    }

    Folded {
        history: history
            .into_iter()
            .filter_map(|id| all.get(&id).map(|(m, _)| (id, m.clone())))
            .collect(),
        display: items,
        model,
        next_id,
        file_changes,
    }
}

/// Drops a dangling tail (a crash mid-turn) so the history is one every
/// provider accepts.
fn repair(history: &mut Vec<(MsgId, Message)>, warnings: &mut Vec<String>) {
    loop {
        let msgs: Vec<Message> = history.iter().map(|(_, m)| m.clone()).collect();
        let cut = match validate_history(&msgs) {
            Ok(()) => return,
            Err(HistoryError::DanglingToolUse { index, .. })
            | Err(HistoryError::OrphanToolResult { index, .. })
            | Err(HistoryError::NotUserMessage { index }) => index,
            Err(_) => 0,
        };
        warnings.push(format!(
            "dropped {} message(s) from an interrupted turn",
            history.len() - cut
        ));
        history.truncate(cut);
    }
}

/// Reads and replays one session log.
pub struct SessionLog;

impl SessionLog {
    /// Folds the records in order into the model's view and the full
    /// transcript.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be read or has no `SessionStart`.
    pub fn load(path: &Path) -> Result<LoadedSession> {
        let bytes = std::fs::read(path).map_err(|e| CoreError::io(path, e))?;
        let text = String::from_utf8_lossy(&bytes);
        let (records, mut warnings) = parse_records(&text);
        let Some(Record::SessionStart {
            v,
            id,
            cwd,
            started_at,
            parent,
            ..
        }) = records.first().cloned()
        else {
            return Err(CoreError::Session(format!(
                "{} does not start with a session_start record",
                path.display()
            )));
        };
        if v > RECORD_VERSION {
            warnings.push(format!(
                "log format v{v} is newer than this build (v{RECORD_VERSION})"
            ));
        }
        let mut f = fold(&records, path, &mut warnings);
        repair(&mut f.history, &mut warnings);
        let msg_ids = f.history.iter().map(|(i, _)| *i).collect();
        let history = f.history.into_iter().map(|(_, m)| m).collect();
        Ok(LoadedSession {
            id,
            path: path.to_owned(),
            cwd,
            started_at,
            parent,
            model: f.model,
            thread: Thread::with_history(ThreadId(id.to_string()), history),
            msg_ids,
            display: f.display,
            next_id: f.next_id,
            file_changes: f.file_changes,
            warnings,
        })
    }

    /// Summarizes a log for a picker.
    pub fn summarize(path: &Path) -> Result<SessionSummary> {
        let s = Self::load(path)?;
        let first = s
            .thread
            .history()
            .iter()
            .find(|m| is_user_turn(m))
            .map(text_of)
            .or_else(|| {
                s.display.iter().find_map(|d| match d {
                    DisplayItem::Message { message, .. } if is_user_turn(message) => {
                        Some(text_of(message))
                    }
                    _ => None,
                })
            });
        let turns = s
            .thread
            .history()
            .iter()
            .filter(|m| is_user_turn(m))
            .count();
        Ok(SessionSummary {
            id: s.id,
            path: s.path,
            cwd: s.cwd,
            started_at: s.started_at,
            model: s.model,
            first_user_message: first,
            turns,
            parent: s.parent,
        })
    }
}

/// The sessions directory tree.
#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    /// A store rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `$XDG_DATA_HOME/cosmos/sessions` (else `~/.local/share/...`).
    pub fn default_root() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|h| PathBuf::from(h).join(".local/share"))
            })?;
        Some(base.join("cosmos").join("sessions"))
    }

    /// The store root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory holding sessions for the workspace `cwd`.
    pub fn project_dir(&self, cwd: &Path) -> PathBuf {
        let canon = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        let hash = sha256_hex(canon.to_string_lossy().as_bytes());
        self.root.join(&hash[..16])
    }

    fn start_record(
        id: Uuid,
        cwd: &Path,
        model: &str,
        parent: Option<Fork>,
    ) -> (Record, DateTime<Utc>) {
        let now = Utc::now();
        (
            Record::SessionStart {
                v: RECORD_VERSION,
                id,
                cwd: cwd.to_owned(),
                model: model.to_owned(),
                started_at: now,
                parent,
            },
            now,
        )
    }

    fn new_path(&self, cwd: &Path, id: Uuid, at: DateTime<Utc>) -> Result<PathBuf> {
        let dir = self.project_dir(cwd);
        private_dir(&dir).map_err(|e| CoreError::io(&dir, e))?;
        Ok(dir.join(format!("{}_{id}.jsonl", at.format("%Y-%m-%dT%H-%M-%S"))))
    }

    fn open_locked(path: &Path, create_new: bool) -> Result<File> {
        let mut file = open_private(path, create_new).map_err(|e| CoreError::io(path, e))?;
        match file.try_lock_exclusive() {
            Ok(true) => {}
            Ok(false) => return Err(CoreError::SessionLocked(path.to_owned())),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(CoreError::SessionLocked(path.to_owned()))
            }
            Err(e) => return Err(CoreError::io(path, e)),
        }
        if !create_new {
            // A crash can leave a torn last line; start on a fresh one.
            let torn = std::fs::read(path)
                .map(|b| b.last().is_some_and(|c| *c != b'\n'))
                .unwrap_or(false);
            if torn {
                file.write_all(b"\n").map_err(|e| CoreError::io(path, e))?;
            }
        }
        Ok(file)
    }

    /// Starts a new session log for `cwd`.
    pub fn create(&self, cwd: &Path, model: &str) -> Result<SessionWriter> {
        self.create_with_parent(cwd, model, None)
    }

    fn create_with_parent(
        &self,
        cwd: &Path,
        model: &str,
        parent: Option<Fork>,
    ) -> Result<SessionWriter> {
        let id = Uuid::new_v4();
        let (start, at) = Self::start_record(id, cwd, model, parent);
        let path = self.new_path(cwd, id, at)?;
        let file = Self::open_locked(&path, true)?;
        let w = SessionWriter::spawn(path, id, file);
        w.append(&start)?;
        Ok(w)
    }

    /// Reopens an existing log for appending (resume, continue).
    pub fn reopen(&self, path: &Path) -> Result<SessionWriter> {
        let file = Self::open_locked(path, false)?;
        let s = SessionLog::summarize(path)?;
        Ok(SessionWriter::spawn(path.to_owned(), s.id, file))
    }

    /// Sessions for `cwd`, newest first.
    pub fn list(&self, cwd: &Path) -> Result<Vec<SessionSummary>> {
        let dir = self.project_dir(cwd);
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(s) = SessionLog::summarize(&p) {
                out.push(s);
            }
        }
        out.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(b.path.cmp(&a.path)));
        Ok(out)
    }

    /// The newest session for `cwd` (`--continue`).
    pub fn latest(&self, cwd: &Path) -> Result<Option<SessionSummary>> {
        Ok(self.list(cwd)?.into_iter().next())
    }

    /// A session of `cwd` by id or unique id prefix (`--resume`).
    pub fn find(&self, cwd: &Path, id: &str) -> Result<SessionSummary> {
        let hits: Vec<_> = self
            .list(cwd)?
            .into_iter()
            .filter(|s| s.id.to_string().starts_with(id))
            .collect();
        match hits.len() {
            1 => Ok(hits.into_iter().next().unwrap_or_else(|| unreachable!())),
            0 => Err(CoreError::NoSuchSession(id.to_owned())),
            _ => Err(CoreError::Session(format!("{id} matches several sessions"))),
        }
    }

    /// Copies the session at `src` up to message `at` (all of it when `None`)
    /// into a new file whose `SessionStart.parent` points at the source. The
    /// copy shares nothing with the source, so the source can be deleted.
    ///
    /// A cut inside a tool exchange backs up to the last complete one.
    pub fn fork(&self, src: &Path, at: Option<MsgId>) -> Result<PathBuf> {
        let text = std::fs::read_to_string(src).map_err(|e| CoreError::io(src, e))?;
        let (records, _) = parse_records(&text);
        let Some(Record::SessionStart {
            id: src_id,
            cwd,
            model,
            ..
        }) = records.first().cloned()
        else {
            return Err(CoreError::Session("no session_start record".into()));
        };
        let message_ids: Vec<MsgId> = records
            .iter()
            .filter_map(|r| match r {
                Record::Message { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        let mut cut = at;
        let kept: Vec<Record> = loop {
            let limit = cut.unwrap_or(MsgId::MAX);
            let mut v = Vec::new();
            for r in records.iter().skip(1) {
                if let Record::Message { id, .. } = r {
                    if *id > limit {
                        break;
                    }
                }
                v.push(r.clone());
            }
            let mut probe = vec![records[0].clone()];
            probe.extend(v.iter().cloned());
            let mut w = Vec::new();
            let f = fold(&probe, src, &mut w);
            let msgs: Vec<Message> = f.history.into_iter().map(|(_, m)| m).collect();
            if validate_history(&msgs).is_ok() {
                break v;
            }
            let Some(prev) = message_ids.iter().rev().find(|i| **i < limit).copied() else {
                break Vec::new();
            };
            cut = Some(prev);
        };

        let id = Uuid::new_v4();
        let parent = Fork {
            session: src_id,
            at: cut,
        };
        let (start, started) = Self::start_record(id, &cwd, &model, Some(parent));
        let path = self.new_path(&cwd, id, started)?;
        let mut file = Self::open_locked(&path, true)?;
        let mut write = |r: &Record| -> Result<()> {
            let mut line = serde_json::to_string(r)?;
            line.push('\n');
            file.write_all(line.as_bytes())
                .map_err(|e| CoreError::io(&path, e))
        };
        write(&start)?;
        for r in &kept {
            write(r)?;
        }
        file.sync_all().map_err(|e| CoreError::io(&path, e))?;
        let _ = FileExt::unlock(&file);

        let src_blobs = aux_dir(src).join("blobs");
        if let Ok(rd) = std::fs::read_dir(&src_blobs) {
            let dst = aux_dir(&path).join("blobs");
            private_dir(&dst).map_err(|e| CoreError::io(&dst, e))?;
            for e in rd.flatten() {
                let _ = std::fs::copy(e.path(), dst.join(e.file_name()));
            }
        }
        Ok(path)
    }

    /// Deletes a session's log and auxiliary files.
    pub fn delete(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path).map_err(|e| CoreError::io(path, e))?;
        let aux = aux_dir(path);
        if aux.is_dir() {
            let _ = std::fs::remove_dir_all(&aux);
        }
        Ok(())
    }

    /// Deletes sessions untouched for `days` days, across all projects.
    /// Returns how many.
    pub fn apply_retention(&self, days: u32) -> Result<usize> {
        let cutoff = SystemTime::now()
            .checked_sub(Duration::from_secs(u64::from(days) * 86_400))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let mut n = 0;
        let Ok(projects) = std::fs::read_dir(&self.root) else {
            return Ok(0);
        };
        for p in projects.flatten() {
            let Ok(rd) = std::fs::read_dir(p.path()) else {
                continue;
            };
            for e in rd.flatten() {
                let path = e.path();
                if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                    continue;
                }
                let old = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t < cutoff);
                if old && self.delete(&path).is_ok() {
                    n += 1;
                }
            }
        }
        Ok(n)
    }
}
