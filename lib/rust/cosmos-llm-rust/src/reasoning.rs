//! Requesting reasoning, and the stream deltas it produces.

use serde::{Deserialize, Serialize};

/// Provider-neutral reasoning effort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReasoningEffort {
    /// The least reasoning the provider offers.
    Minimal,
    /// Light reasoning.
    Low,
    /// A balanced default.
    Medium,
    /// As much as the provider allows.
    High,
}

impl ReasoningEffort {
    /// The Anthropic thinking budget this effort maps to, in tokens.
    pub(crate) fn anthropic_budget(self) -> u32 {
        match self {
            Self::Minimal => 1024,
            Self::Low => 2048,
            Self::Medium => 8192,
            Self::High => 24576,
        }
    }

    /// The OpenAI `reasoning_effort` string.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// The effort nearest to an explicit token budget.
    pub(crate) fn from_budget(tokens: u32) -> Self {
        match tokens {
            0..=1024 => Self::Minimal,
            1025..=4096 => Self::Low,
            4097..=16384 => Self::Medium,
            _ => Self::High,
        }
    }
}

/// How much the model should reason before answering.
///
/// Set it with
/// [`CompletionRequest::with_reasoning`](crate::CompletionRequest::with_reasoning).
/// A model the built-in table knows cannot reason makes the request fail
/// locally with [`CosmosError::Configuration`](crate::CosmosError) rather than
/// come back as a 400; a model it does not know is sent as-is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Reasoning {
    /// Send nothing; the model's default applies.
    Off,
    /// Provider-neutral effort, mapped per provider.
    Effort {
        /// The effort level.
        effort: ReasoningEffort,
    },
    /// Explicit token budget. Anthropic takes it as-is (minimum 1024); others
    /// map it to the nearest effort.
    Budget {
        /// Thinking tokens.
        tokens: u32,
    },
}

impl Reasoning {
    /// Shorthand for `Reasoning::Effort { effort }`.
    pub fn effort(effort: ReasoningEffort) -> Self {
        Self::Effort { effort }
    }

    /// Shorthand for `Reasoning::Budget { tokens }`.
    pub fn budget(tokens: u32) -> Self {
        Self::Budget { tokens }
    }

    /// Whether this asks for any reasoning at all.
    pub fn is_on(&self) -> bool {
        !matches!(self, Self::Off)
    }

    /// The Anthropic `budget_tokens`, or `None` for `Off`.
    pub(crate) fn anthropic_budget(&self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Effort { effort } => Some(effort.anthropic_budget()),
            Self::Budget { tokens } => Some((*tokens).max(1024)),
        }
    }

    /// The nearest effort level, or `None` for `Off`.
    pub(crate) fn nearest_effort(&self) -> Option<ReasoningEffort> {
        match self {
            Self::Off => None,
            Self::Effort { effort } => Some(*effort),
            Self::Budget { tokens } => Some(ReasoningEffort::from_budget(*tokens)),
        }
    }
}

/// A fragment of reasoning in a [`StreamChunk`](crate::StreamChunk).
///
/// `index` identifies the thinking block, as the provider numbers it.
/// [`StreamAccumulator`](crate::StreamAccumulator) assembles these into
/// [`ContentBlock::Thinking`](crate::ContentBlock) and `RedactedThinking`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ThinkingDelta {
    /// More reasoning text.
    Text {
        /// Block index.
        index: u32,
        /// The text fragment.
        text: String,
    },
    /// The block's signature (sent once, near the end of the block).
    Signature {
        /// Block index.
        index: u32,
        /// The signature.
        signature: String,
    },
    /// An opaque, redacted block.
    Redacted {
        /// Block index.
        index: u32,
        /// Encrypted payload.
        data: String,
    },
}

impl ThinkingDelta {
    /// The block index this delta belongs to.
    pub fn index(&self) -> u32 {
        match self {
            Self::Text { index, .. }
            | Self::Signature { index, .. }
            | Self::Redacted { index, .. } => *index,
        }
    }
}
