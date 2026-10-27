//! The session log's IR (200): one [`Record`] per JSONL line.
//!
//! JSONL is only the storage encoding. Each line is a `type`-tagged object;
//! `SessionStart` carries the format version `v`. A reader skips a record type
//! it does not know (written by a newer version) with a warning instead of
//! failing.

use std::ops::Range;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use cosmos_llm::Message;
use cosmos_llm_tool::TokenUsage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// The log format version written into `SessionStart`.
pub const RECORD_VERSION: u32 = 1;

/// A message's id within one session. Ids start at 1 and only grow; `0` means
/// "before the first message".
pub type MsgId = u64;

/// A content-addressed blob in `<session>/blobs/`, named by its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlobRef(pub String);

/// Where a forked session came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Fork {
    /// The source session.
    pub session: Uuid,
    /// The last message copied, or everything if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<MsgId>,
}

impl Fork {
    /// A fork of `session` at message `at`.
    pub fn new(session: Uuid, at: Option<MsgId>) -> Self {
        Self { session, at }
    }
}

/// One file a turn changed, for undo across restarts (130).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileChange {
    /// The file, as the tracker names it.
    pub path: String,
    /// Content before the turn's first write; absent if the file was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<BlobRef>,
    /// Content hash after the turn's last write; absent if deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

impl FileChange {
    /// A change record.
    pub fn new(path: impl Into<String>, before: Option<BlobRef>, after: Option<String>) -> Self {
        Self {
            path: path.into(),
            before,
            after,
        }
    }
}

/// One line of the session log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Record {
    /// The first line.
    SessionStart {
        /// Format version, [`RECORD_VERSION`].
        v: u32,
        /// The session id.
        id: Uuid,
        /// The workspace.
        cwd: PathBuf,
        /// The model at the start.
        model: String,
        /// When the session began.
        started_at: DateTime<Utc>,
        /// Set on a forked session.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<Fork>,
    },
    /// A message added to the thread. Images are stored as blob references
    /// (`blob:<sha256>` in the base64 `data` field).
    Message {
        /// Its id.
        id: MsgId,
        /// The message.
        message: Message,
        /// The usage the provider reported with this reply.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsage>,
    },
    /// UI-only data for a tool result (diffs, exit codes) that the model
    /// never saw.
    ToolOutcome {
        /// The call it belongs to.
        call_id: String,
        /// The `display` value from the loop.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display: Option<Value>,
    },
    /// Old messages replaced by a summary (170). In-context messages with an
    /// id in `replaces`, except those in `except`, are removed and the
    /// summary takes the position of the first one.
    Compaction {
        /// Ids covered, `start..end`.
        replaces: Range<MsgId>,
        /// The summary message (an earlier `Message` record).
        summary: MsgId,
        /// Ids inside `replaces` that were kept (ids do not follow position
        /// once a summary has been inserted).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        except: Vec<MsgId>,
    },
    /// The model changed.
    ModelChanged {
        /// The new model.
        model: String,
    },
    /// Edit-and-resend: everything after the message with id `after`, in
    /// thread order, was dropped. `0` drops everything.
    Truncated {
        /// The last message kept.
        after: MsgId,
    },
    /// The files a turn changed (130).
    FileChanges {
        /// The tracker's turn number.
        turn: u32,
        /// The files.
        files: Vec<FileChange>,
    },
}

/// One entry of the display view: the full transcript, including what the
/// model no longer sees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DisplayItem {
    /// A message.
    Message {
        /// Its id.
        id: MsgId,
        /// The message.
        message: Message,
        /// Reported usage.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsage>,
        /// Whether the model still sees it (false once compacted away or
        /// truncated).
        in_context: bool,
    },
    /// UI data for a tool result.
    ToolDisplay {
        /// The call.
        call_id: String,
        /// The data.
        display: Value,
    },
    /// A compaction happened here.
    Compaction {
        /// Ids covered.
        replaces: Range<MsgId>,
        /// The summary's id.
        summary: MsgId,
    },
    /// A truncation happened here.
    Truncated {
        /// The last message kept.
        after: MsgId,
    },
    /// The model changed here.
    ModelChanged {
        /// The new model.
        model: String,
    },
}
