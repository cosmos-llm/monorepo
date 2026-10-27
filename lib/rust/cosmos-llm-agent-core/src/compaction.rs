//! Compaction (170).
//!
//! The budget check (160) decides *whether* to compact and hands a
//! [`CompactionPlan`] to whatever [`Compactor`] the host installed; the core
//! applies the returned [`CompactionOutcome`] to the thread and logs it.
//!
//! The default compactor is [`SummarizingCompactor`]: it picks a cut point
//! that never separates a `ToolUse` from its `ToolResult`, keeps the tail
//! after the cut verbatim, asks the model to summarize what came before, and
//! re-attaches the recent user messages word for word. Strategies are
//! registered by name ([`CompactorPlugin`]) so a host can swap one in with
//! `compaction.strategy` in config.

mod summarize;

pub use summarize::{
    compactor, default_compactor, is_summary, CompactorContext, CompactorPlugin, DropCompactor,
    SummarizingCompactor, SUMMARY_PREFIX,
};

use std::ops::Range;

use cosmos_llm::Message;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Why a compaction was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CompactionReason {
    /// The budget crossed its threshold.
    Threshold,
    /// The user asked (`Op::Compact`).
    Manual,
    /// The provider rejected the request as too long.
    ContextLength,
}

/// The compaction IR: what the budget check asks the compactor for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CompactionPlan {
    /// Why.
    pub reason: CompactionReason,
    /// Estimated tokens in use now.
    pub used: u32,
    /// The model's window.
    pub window: u32,
    /// Tokens the history should shrink to.
    pub target: u32,
    /// Extra instructions for the summary (`/compact focus on the parser`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Compaction settings (170), the `compaction` section of [`CoreConfig`](crate::CoreConfig).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct CompactionConfig {
    /// Install the default compactor when the host gives none. Turn off to
    /// run without compaction (the context-length error then reaches the
    /// user).
    pub enabled: bool,
    /// The registered strategy: `summarize` (default) or `drop`.
    pub strategy: String,
    /// The model that writes the summary. Defaults to the session's model;
    /// a cheaper one is often fine.
    pub model: Option<String>,
    /// Always keep at least this many of the latest user turns verbatim.
    pub keep_turns: u32,
    /// The kept tail should be about this fraction of the window.
    pub tail_fraction: f64,
    /// Budget, in estimated tokens, for the user messages re-attached
    /// verbatim to the summary (newest first).
    pub recent_user_tokens: u32,
    /// Tool results are cut to this many characters in the summarizer's input.
    pub tool_result_chars: usize,
    /// Summarizer input larger than this many estimated tokens is summarized
    /// in chunks, each chunk folding in the summary so far.
    pub chunk_tokens: u32,
    /// Output cap for one summary call.
    pub summary_max_tokens: u32,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: "summarize".to_owned(),
            model: None,
            keep_turns: 2,
            tail_fraction: 0.25,
            recent_user_tokens: 20_000,
            tool_result_chars: 2_000,
            chunk_tokens: 60_000,
            summary_max_tokens: 4_096,
        }
    }
}

/// What a compactor returns: the history range it replaced and the message
/// standing in for it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CompactionOutcome {
    /// Indices into the history that `summary` replaces.
    pub replaces: Range<usize>,
    /// The replacement message.
    pub summary: Message,
}

impl CompactionOutcome {
    /// Replaces `replaces` with `summary`.
    pub fn new(replaces: Range<usize>, summary: Message) -> Self {
        Self { replaces, summary }
    }
}

/// Replaces old turns with something shorter. The core only calls it.
pub trait Compactor: Send + Sync {
    /// Produces the outcome for `plan` over `history`.
    fn compact<'a>(
        &'a self,
        plan: &'a CompactionPlan,
        history: &'a [Message],
    ) -> BoxFuture<'a, Result<CompactionOutcome, CoreError>>;
}
