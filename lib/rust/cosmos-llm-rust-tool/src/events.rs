//! The loop's output IRs: what a run tells its frontend, and what a tool
//! call produced (005).
//!
//! Everything here is owned, serializable data. The core, the session log and
//! every frontend consume these without depending on loop internals. On the
//! wire each enum is a `type`-tagged, `snake_case` object.

use cosmos_llm::ContentBlock;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::TokenUsage;
use crate::approval::{ApprovalRequest, Decision};

/// Text shown to the model for a call that was cut short by an interrupt.
pub const INTERRUPTED_TEXT: &str = "Interrupted by user";

/// What a tool call produced.
///
/// `content` is what the model sees; `display` is for the UI only. An edit
/// tool returns a short confirmation in `content` and a structured diff in
/// `display`. Each tool crate documents the IR it puts in `display`
/// (`FileDiff` for edits, a shell-run record for bash), so frontends
/// deserialize a known type instead of probing JSON.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::ToolOutcome;
///
/// let ok = ToolOutcome::text("wrote 3 lines");
/// assert_eq!(ok.as_text(), "wrote 3 lines");
/// assert!(!ok.is_error);
///
/// let bad = ToolOutcome::error("no such file");
/// assert!(bad.is_error);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolOutcome {
    /// What the model sees.
    pub content: Vec<ContentBlock>,
    /// The call failed; the model should treat `content` as an error.
    #[serde(default)]
    pub is_error: bool,
    /// Richer data for the UI: a diff, an exit code, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<Value>,
    /// The output limit cut the result.
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
    /// Where the full text was kept when it was cut and a spill store is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spill_path: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl ToolOutcome {
    /// A successful, text-only outcome.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::text(text)],
            is_error: false,
            display: None,
            truncated: false,
            spill_path: None,
        }
    }

    /// A failed, text-only outcome.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            is_error: true,
            ..Self::text(text)
        }
    }

    /// The outcome for a call the user interrupted.
    pub fn interrupted() -> Self {
        Self::error(INTERRUPTED_TEXT)
    }

    /// Attaches UI-only data.
    #[must_use]
    pub fn with_display(mut self, display: Value) -> Self {
        self.display = Some(display);
        self
    }

    /// The text blocks of `content`, concatenated.
    pub fn as_text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// Why a turn (one model call and the tools it asked for) ended.
///
/// The last `TurnFinished` of a run carries the reason the run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnStop {
    /// Tools ran; the loop continues with another turn.
    ToolUse,
    /// The model stopped calling tools.
    EndTurn,
    /// A tool called `Session::finish`.
    Finished,
    /// `max_steps` was reached.
    Steps,
    /// The budget callback stopped the run.
    Budget,
    /// An approval returned `Abort`.
    Aborted,
    /// The cancellation token fired.
    Interrupted,
}

/// One thing that happened during a run, in order.
///
/// Sent on a bounded `tokio::sync::mpsc` channel: when the receiver falls
/// behind, the loop waits, which is the right backpressure for a UI. A dropped
/// receiver is ignored and the run continues.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::AgentEvent;
///
/// let e = AgentEvent::TextDelta { text: "hi".into() };
/// assert_eq!(serde_json::to_value(&e).unwrap()["type"], "text_delta");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// A model call is about to start. `turn` counts from 1 within a run.
    TurnStarted {
        /// The turn number.
        turn: u32,
    },
    /// A fragment of the model's text.
    TextDelta {
        /// The fragment.
        text: String,
    },
    /// A fragment of the model's reasoning.
    ThinkingDelta {
        /// The fragment.
        text: String,
    },
    /// The model has started a tool call; arguments are still streaming.
    ToolCallStarted {
        /// The model's id for the call.
        call_id: String,
        /// The tool's name.
        name: String,
    },
    /// Raw partial JSON, for UIs that preview e.g. the file being written.
    ToolCallArgsDelta {
        /// The call this belongs to.
        call_id: String,
        /// The fragment.
        delta: String,
    },
    /// Arguments complete; about to ask for approval or execute.
    ToolCallReady {
        /// The model's id for the call.
        call_id: String,
        /// The tool's name.
        name: String,
        /// The parsed arguments.
        input: Value,
    },
    /// The approver is being asked. The frontend should show the prompt.
    ApprovalRequested {
        /// What is being asked.
        request: ApprovalRequest,
    },
    /// The approver answered (or the user interrupted, which reads as abort).
    ApprovalResolved {
        /// The call that was decided.
        call_id: String,
        /// The answer.
        decision: Decision,
    },
    /// The call passed approval and is executing.
    ToolStarted {
        /// The call.
        call_id: String,
    },
    /// Progress from a long tool, such as shell output lines.
    ToolOutputDelta {
        /// The call.
        call_id: String,
        /// The chunk.
        chunk: String,
    },
    /// The call has a result (including denials, errors and interruptions).
    ToolFinished {
        /// The call.
        call_id: String,
        /// What it produced.
        result: ToolOutcome,
    },
    /// Queued user input was delivered into the history at a safe point.
    InputDelivered {
        /// The delivered text.
        text: String,
    },
    /// Usage for the turn that just ended, and the run so far.
    Usage {
        /// This turn only.
        turn: TokenUsage,
        /// The run so far.
        total: TokenUsage,
    },
    /// A turn ended.
    TurnFinished {
        /// The turn number.
        turn: u32,
        /// Why it ended.
        stop: TurnStop,
    },
    /// The run failed; the error is also returned from `run`.
    Error {
        /// A description.
        message: String,
        /// Whether trying again may succeed.
        retryable: bool,
    },
}
