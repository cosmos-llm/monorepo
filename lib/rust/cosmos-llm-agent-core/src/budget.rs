//! Token accounting (160): how full is the window, before sending.
//!
//! Exact local tokenization is not available for every provider, so the count
//! anchors on the provider's own number:
//!
//! ```text
//! estimate(next request) =
//!     last_response.prompt_tokens        // exact, includes cached input
//!   + last_response.completion_tokens    // the assistant message now in history
//!   + estimate(messages appended since)  // tool results, new user input
//! ```
//!
//! Only the tail is estimated, so the error stays small. The cheap estimator
//! lives in `cosmos-llm` ([`cosmos_llm::tokens`]); [`ContextBudget`] and
//! [`ContextTracker`] are the core's use of it.

use std::sync::Arc;

use cosmos_llm::{CharsEstimator, Message, ModelInfo, TokenEstimator};
use cosmos_llm_tool::TokenUsage;
use serde::{Deserialize, Serialize};

use crate::config::ContextConfig;

/// Never reserve more than this for the reply.
pub const MAX_RESERVE_OUTPUT: u32 = 32_000;

/// How much of the window the next request may use.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ContextBudget {
    /// The model's context window (050).
    pub window: u32,
    /// Tokens kept free for the reply.
    pub reserve_output: u32,
    /// Fraction of `window - reserve_output` at which to compact, e.g. 0.85.
    pub compact_at: f64,
}

/// How full the window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Pressure {
    /// Room to spare.
    Ok,
    /// Past the compaction threshold.
    Compact,
    /// Past what the window can hold; the provider would reject the request.
    Overflow,
}

impl ContextBudget {
    /// A budget from explicit numbers.
    pub fn new(window: u32, reserve_output: u32, compact_at: f64) -> Self {
        Self {
            window,
            reserve_output,
            compact_at,
        }
    }

    /// A budget for `info`, with `cfg` overriding the window, the reserve and
    /// the threshold. `None` when the window is unknown.
    pub fn for_model(info: Option<&ModelInfo>, cfg: &ContextConfig) -> Option<Self> {
        let window = cfg.window.or_else(|| info.and_then(|i| i.context_window))?;
        let reserve = cfg.reserve_output.unwrap_or_else(|| {
            info.and_then(|i| i.max_output_tokens)
                .unwrap_or(8_192)
                .min(MAX_RESERVE_OUTPUT)
        });
        // A window smaller than the reserve would leave nothing to fill.
        let reserve = reserve.min(window / 2);
        Some(Self::new(window, reserve, cfg.compact_at))
    }

    /// Tokens the prompt may use: the window less the reply reserve.
    pub fn usable(&self) -> u32 {
        self.window.saturating_sub(self.reserve_output)
    }

    /// The token count at which compaction starts.
    pub fn compact_threshold(&self) -> u32 {
        (self.usable() as f64 * self.compact_at.clamp(0.0, 1.0)) as u32
    }

    /// Classifies `used` tokens.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm_agent_core::{ContextBudget, Pressure};
    ///
    /// let b = ContextBudget::new(100_000, 20_000, 0.5); // usable 80k, threshold 40k
    /// assert_eq!(b.pressure(39_999), Pressure::Ok);
    /// assert_eq!(b.pressure(40_000), Pressure::Compact);
    /// assert_eq!(b.pressure(80_001), Pressure::Overflow);
    /// ```
    pub fn pressure(&self, used: u32) -> Pressure {
        if used > self.usable() {
            Pressure::Overflow
        } else if used >= self.compact_threshold() {
            Pressure::Compact
        } else {
            Pressure::Ok
        }
    }
}

/// The context a response left behind: prompt plus reply.
pub fn context_tokens(usage: &TokenUsage) -> u32 {
    u32::try_from(usage.prompt_tokens.saturating_add(usage.completion_tokens)).unwrap_or(u32::MAX)
}

#[derive(Debug, Clone, Copy)]
struct Anchor {
    tokens: u32,
    /// Messages in the history when the response arrived, reply included.
    messages: usize,
}

/// Estimates the size of the next request from the last response's usage.
pub struct ContextTracker {
    estimator: Arc<dyn TokenEstimator>,
    system_tokens: u32,
    anchor: Option<Anchor>,
}

impl std::fmt::Debug for ContextTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextTracker")
            .field("system_tokens", &self.system_tokens)
            .field("anchor", &self.anchor)
            .finish_non_exhaustive()
    }
}

impl Default for ContextTracker {
    fn default() -> Self {
        Self::new(Arc::new(CharsEstimator), 0)
    }
}

impl ContextTracker {
    /// A tracker using `estimator` for the unanchored tail. `system_tokens`
    /// stands in for the system prompt and tools until a response anchors the
    /// count.
    pub fn new(estimator: Arc<dyn TokenEstimator>, system_tokens: u32) -> Self {
        Self {
            estimator,
            system_tokens,
            anchor: None,
        }
    }

    /// Anchors on a response. `messages` is the history length after the
    /// reply was appended. Providers that report nothing leave the anchor
    /// alone.
    pub fn record_response(&mut self, usage: &TokenUsage, messages: usize) {
        let tokens = context_tokens(usage);
        if usage.prompt_tokens > 0 {
            self.anchor = Some(Anchor { tokens, messages });
        }
    }

    /// Forgets the anchor, after the history was rewritten (truncation,
    /// compaction).
    pub fn invalidate(&mut self) {
        self.anchor = None;
    }

    /// Whether a provider count anchors the estimate.
    pub fn is_anchored(&self) -> bool {
        self.anchor.is_some()
    }

    /// The estimated size of a request carrying `history`.
    pub fn estimate(&self, history: &[Message]) -> u32 {
        match self.anchor {
            Some(a) if a.messages <= history.len() => a
                .tokens
                .saturating_add(self.estimator.estimate(&history[a.messages..])),
            _ => self
                .system_tokens
                .saturating_add(self.estimator.estimate(history)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(prompt: u64, completion: u64) -> TokenUsage {
        TokenUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            ..TokenUsage::default()
        }
    }

    #[test]
    fn threshold_math() {
        let b = ContextBudget::new(200_000, 32_000, 0.85);
        assert_eq!(b.usable(), 168_000);
        assert_eq!(b.compact_threshold(), 142_800);
        assert_eq!(b.pressure(142_799), Pressure::Ok);
        assert_eq!(b.pressure(142_800), Pressure::Compact);
        assert_eq!(b.pressure(168_000), Pressure::Compact);
        assert_eq!(b.pressure(168_001), Pressure::Overflow);
    }

    #[test]
    fn budget_from_model_and_config() {
        let info = ModelInfo::new("x", "m")
            .with_context_window(100_000)
            .with_max_output_tokens(64_000);
        let b = ContextBudget::for_model(Some(&info), &ContextConfig::default()).unwrap();
        assert_eq!(b.window, 100_000);
        assert_eq!(b.reserve_output, MAX_RESERVE_OUTPUT);
        let cfg = ContextConfig {
            window: Some(10_000),
            reserve_output: Some(1_000),
            compact_at: 0.5,
        };
        let b = ContextBudget::for_model(None, &cfg).unwrap();
        assert_eq!(
            (b.window, b.reserve_output, b.compact_threshold()),
            (10_000, 1_000, 4_500)
        );
        assert!(ContextBudget::for_model(None, &ContextConfig::default()).is_none());
    }

    #[test]
    fn anchor_plus_tail() {
        let mut t = ContextTracker::default();
        let mut history = vec![Message::user("q"), Message::assistant("a")];
        // No anchor yet: estimated from scratch.
        let cold = t.estimate(&history);
        assert!(cold > 0);
        t.record_response(&usage(1_000, 50), history.len());
        assert_eq!(t.estimate(&history), 1_050);
        history.push(Message::user("x".repeat(400)));
        let with_tail = t.estimate(&history);
        assert_eq!(with_tail, 1_050 + 100 + 4);
        t.invalidate();
        assert!(t.estimate(&history) < 1_050);
    }

    #[test]
    fn silent_providers_do_not_anchor() {
        let mut t = ContextTracker::default();
        t.record_response(&usage(0, 0), 3);
        assert!(!t.is_anchored());
    }

    /// The anchor + tail estimate lands within 5% of a recorded session: a
    /// fixture of (history length, reported prompt tokens) per response, where
    /// the next response's prompt tokens are the truth for the estimate made
    /// after the previous one plus the appended tool result.
    #[test]
    fn estimate_is_within_five_percent_on_a_fixture_session() {
        // Real-ish numbers: responses report prompt tokens; the tool result
        // appended in between is ~chars/4 tokens plus the new reply's overhead.
        let tool_result = "fn main() {}\n".repeat(300); // 3900 chars
        let mut history = vec![Message::user("fix it"), Message::assistant("reading")];
        let mut t = ContextTracker::default();
        t.record_response(&usage(20_000, 40), history.len());
        history.push(Message::user(tool_result));
        let predicted = t.estimate(&history);
        // The provider then reported 20_000 + 40 + 990 prompt tokens.
        let actual = 20_000 + 40 + 990;
        let err = (f64::from(predicted) - f64::from(actual)).abs() / f64::from(actual);
        assert!(
            err < 0.05,
            "predicted {predicted}, actual {actual}, err {err}"
        );
    }
}
