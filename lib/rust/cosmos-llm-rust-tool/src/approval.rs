//! The approval gate: a single asynchronous question asked before a tool runs.
//!
//! The loop decides nothing itself. It builds an [`ApprovalRequest`], hands it
//! to an [`Approver`], and acts on the [`Decision`]. A policy engine (150) or an
//! interactive prompt implements the trait; the data types are IRs (005).

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::spec::ToolHints;

/// What the approver is asked about one tool call.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{ApprovalRequest, ToolHints};
/// use serde_json::json;
///
/// let req = ApprovalRequest::new("call_1", "shell", json!({"command": "ls"}));
/// assert!(req.preview.is_none());
/// assert_eq!(req.hints, ToolHints::new());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ApprovalRequest {
    /// The model's id for the call.
    pub call_id: String,
    /// The tool's name.
    pub tool: String,
    /// The arguments as the model sent them.
    pub input: Value,
    /// Tool-supplied summary for the prompt: the command, the path, the diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<Value>,
    /// Metadata the policy can read without knowing the tool.
    #[serde(default)]
    pub hints: ToolHints,
}

impl ApprovalRequest {
    /// A request with no preview and no hints.
    pub fn new(call_id: impl Into<String>, tool: impl Into<String>, input: Value) -> Self {
        Self {
            call_id: call_id.into(),
            tool: tool.into(),
            input,
            preview: None,
            hints: ToolHints::default(),
        }
    }
}

/// Which future calls an [`Decision::AllowAlways`] covers.
///
/// The loop remembers grants for its own lifetime and skips the approver for
/// calls they match. A policy that wants durable grants should keep its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApprovalScope {
    /// Every call to the named tool.
    Tool {
        /// The tool's name.
        name: String,
    },
    /// Calls to the tool with exactly these arguments.
    Input {
        /// The tool's name.
        name: String,
        /// The arguments.
        input: Value,
    },
    /// Calls to the tool whose paths all lie under `prefix`.
    Path {
        /// The tool's name.
        name: String,
        /// A directory or file path prefix.
        prefix: String,
    },
}

impl ApprovalScope {
    /// Returns `true` if a call to `tool` with `input` and `hints` is covered.
    pub fn covers(&self, tool: &str, input: &Value, hints: &ToolHints) -> bool {
        match self {
            Self::Tool { name } => name == tool,
            Self::Input {
                name,
                input: granted,
            } => name == tool && granted == input,
            Self::Path { name, prefix } => {
                name == tool
                    && !hints.paths.is_empty()
                    && hints
                        .paths
                        .iter()
                        .all(|p| p.starts_with(std::path::Path::new(prefix)))
            }
        }
    }
}

/// The answer to an [`ApprovalRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Decision {
    /// Run this call.
    Allow,
    /// Run this call, and skip the approver for later calls matching `scope`.
    AllowAlways {
        /// What future calls are covered.
        scope: ApprovalScope,
    },
    /// Reject; `reason` goes back to the model as the tool result.
    Deny {
        /// Why, in words the model can act on.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Reject this call and the rest of the turn, and end the run (the user
    /// pressed Esc on the prompt). Every call still gets a result.
    Abort,
}

impl Decision {
    /// A denial with a reason.
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: Some(reason.into()),
        }
    }
}

/// Decides whether a tool call may run.
///
/// Calls in one turn are checked one at a time, in order, so the user never
/// sees two prompts at once. Approved calls then execute concurrently. The
/// future is dropped if the user interrupts while it waits, which the loop
/// treats as [`Decision::Abort`].
pub trait Approver: Send + Sync {
    /// Answers for one call.
    fn check<'a>(&'a self, req: &'a ApprovalRequest) -> BoxFuture<'a, Decision>;
}

/// Allows everything. What the loop does with no approver at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAll;

impl Approver for AllowAll {
    fn check<'a>(&'a self, _req: &'a ApprovalRequest) -> BoxFuture<'a, Decision> {
        Box::pin(async { Decision::Allow })
    }
}

/// Wraps a synchronous closure as an [`Approver`], for policies that answer
/// without involving the user and for tests.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{Decision, FnApprover};
///
/// let policy = FnApprover::new(|req| {
///     if req.hints.destructive { Decision::deny("read-only mode") } else { Decision::Allow }
/// });
/// # let _ = policy;
/// ```
pub struct FnApprover<F>(F);

impl<F> FnApprover<F>
where
    F: Fn(&ApprovalRequest) -> Decision + Send + Sync,
{
    /// Wraps `f`.
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<F> Approver for FnApprover<F>
where
    F: Fn(&ApprovalRequest) -> Decision + Send + Sync,
{
    fn check<'a>(&'a self, req: &'a ApprovalRequest) -> BoxFuture<'a, Decision> {
        let decision = (self.0)(req);
        Box::pin(async move { decision })
    }
}
