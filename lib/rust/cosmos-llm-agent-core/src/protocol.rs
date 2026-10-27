//! The core's boundary (210): [`Op`]s in, [`Event`]s out.
//!
//! These two types are the only ones a frontend sees. Both are plain serde
//! (`op` / `event` tagged, `snake_case`), so the same protocol works in
//! process over channels and out of process as JSONL on stdio. Wrap either in
//! an [`Envelope`] to carry the protocol version `v`.
//!
//! The core sends structured data (`FileDiff`, tool `display` values), never
//! ANSI. Rendering, keybindings and the composer are the frontend's.

use std::path::PathBuf;

use cosmos_llm::{ContentBlock, Reasoning};
use cosmos_llm_coding_tools::FileDiff;
use cosmos_llm_tool::{AgentEvent, Decision, StopReason, TokenUsage, ToolHints};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::budget::Pressure;
use crate::compaction::CompactionReason;
use crate::policy::ApprovalMode;
use crate::record::{DisplayItem, MsgId};

/// The protocol version, sent in `SessionConfigured`.
pub const PROTOCOL_VERSION: u32 = 1;

/// A versioned wrapper for a message sent across a process boundary.
///
/// ```
/// use cosmos_llm_agent_core::{Envelope, Op};
///
/// let line = serde_json::to_string(&Envelope::new(Op::Interrupt)).unwrap();
/// assert_eq!(line, r#"{"v":1,"op":"interrupt"}"#);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Envelope<T> {
    /// [`PROTOCOL_VERSION`].
    pub v: u32,
    /// The message.
    #[serde(flatten)]
    pub body: T,
}

impl<T> Envelope<T> {
    /// Wraps `body` at the current version.
    pub fn new(body: T) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            body,
        }
    }
}

/// Frontend to core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Op {
    /// Text and pasted images. Sent while a turn runs, it steers the turn: the
    /// loop delivers it between tool results and the next model call.
    UserInput {
        /// The content.
        blocks: Vec<ContentBlock>,
    },
    /// Stop the running turn.
    Interrupt,
    /// Answer an `ApprovalRequested`.
    Approve {
        /// The call being decided.
        call_id: String,
        /// The answer.
        decision: Decision,
        /// With `AllowAlways`, also write the grant to
        /// `<repo>/.cosmos/permissions.trongo`.
        #[serde(default, skip_serializing_if = "is_false")]
        remember: bool,
    },
    /// Switch models for the next request.
    SetModel {
        /// The model id.
        model: String,
    },
    /// Change the approval mode.
    SetMode {
        /// The mode.
        mode: ApprovalMode,
    },
    /// Change the reasoning request.
    SetReasoning {
        /// The setting.
        reasoning: Reasoning,
    },
    /// Compact the context now (170).
    Compact {
        /// Extra instructions for the summary.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
    },
    /// Revert the files a turn changed (130). Without `turn`, the latest turn
    /// with changes.
    Undo {
        /// The tracker's turn number.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn: Option<u32>,
    },
    /// Edit and resend: drop `message` and everything after it (070).
    TruncateTo {
        /// A user message id.
        message: MsgId,
    },
    /// Run a slash command (230).
    RunCommand {
        /// The command name.
        name: String,
        /// Its arguments.
        args: String,
    },
    /// Stop everything and flush the session.
    Shutdown,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Core to frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Event {
    /// The first event: the session is ready.
    SessionConfigured {
        /// [`PROTOCOL_VERSION`].
        protocol: u32,
        /// The session id (a uuid), also the log file's id.
        session_id: String,
        /// The model.
        model: String,
        /// The workspace.
        cwd: PathBuf,
        /// The approval mode.
        mode: ApprovalMode,
        /// The model's context window, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_window: Option<u32>,
        /// The session log, when one is written.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        log_path: Option<PathBuf>,
    },
    /// A resumed session's transcript, for the frontend to draw.
    HistoryLoaded {
        /// The full display view.
        items: Vec<DisplayItem>,
    },
    /// A loop event: deltas, tool lifecycle, usage. `ApprovalRequested` is not
    /// forwarded this way; see [`Event::ApprovalRequested`].
    Agent(AgentEvent),
    /// An event from a subagent (240) started by the `task` call `call_id`
    /// (a nested subagent's id is `<outer>/<inner>`). The UI can show it
    /// indented under the call, or hide it. Approval questions are not
    /// forwarded this way; they arrive as [`Event::ApprovalRequested`] with the
    /// call id `<task call id>/<child call id>`.
    Subagent {
        /// The `task` call that started the subagent.
        call_id: String,
        /// The agent's name.
        #[serde(default)]
        agent: String,
        /// What happened inside it.
        inner: Box<AgentEvent>,
    },
    /// The user must decide. Answer with [`Op::Approve`].
    ApprovalRequested {
        /// The call.
        call_id: String,
        /// The tool.
        tool: String,
        /// The arguments.
        input: Value,
        /// A preview (a diff for edits).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview: Option<Value>,
        /// What the tool declared about the call.
        hints: ToolHints,
    },
    /// How full the window is (160).
    ContextUsage {
        /// Estimated tokens in use.
        used: u32,
        /// The window.
        window: u32,
        /// Where that sits against the budget.
        pressure: Pressure,
    },
    /// Compaction began.
    CompactionStarted {
        /// Why.
        reason: CompactionReason,
    },
    /// Compaction finished.
    CompactionFinished {
        /// Estimated tokens before.
        before: u32,
        /// Estimated tokens after.
        after: u32,
    },
    /// The files the turn changed.
    TurnDiff {
        /// One diff per file.
        files: Vec<FileDiff>,
    },
    /// An undo ran.
    Undone {
        /// The turn.
        turn: u32,
        /// Files restored.
        reverted: Vec<String>,
        /// Files left alone because they changed afterwards.
        skipped: Vec<String>,
    },
    /// The model changed.
    ModelChanged {
        /// The model.
        model: String,
        /// Its window, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_window: Option<u32>,
    },
    /// The mode changed.
    ModeChanged {
        /// The mode.
        mode: ApprovalMode,
    },
    /// An input has been run to completion (or stopped). One per
    /// `UserInput` that started a run.
    RunFinished {
        /// Why the loop stopped; absent when it failed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<StopReason>,
        /// Model turns taken.
        steps: u32,
        /// Tokens used by the run.
        usage: TokenUsage,
    },
    /// Something the user should know that is not an error.
    Warning {
        /// The text.
        message: String,
    },
    /// Something failed.
    Error {
        /// The text.
        message: String,
        /// Whether the core can continue.
        fatal: bool,
    },
    /// The last event; the channel closes after it.
    ShutdownComplete,
}
