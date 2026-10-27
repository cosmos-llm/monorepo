//! Cheap token estimates for messages that have not been sent yet.
//!
//! The provider's own count is exact but only arrives after a request. To know
//! how full the window is *before* sending, anchor on the last response's
//! [`Usage::prompt_tokens`](crate::Usage) and estimate only the messages
//! appended since, with [`estimate_tokens`]. The error stays small because the
//! tail is small next to the whole.
//!
//! The default estimator is `ceil(chars / 4)` for text plus a fixed cost per
//! image and per message. It over-counts code and under-counts CJK a little;
//! implement [`TokenEstimator`] for something sharper.

use crate::content::ContentBlock;
use crate::types::Message;

/// Tokens charged per image. Anthropic bills roughly `w*h/750` capped near
/// 1600; the image bytes are not decoded here, so the cap stands in.
pub const IMAGE_TOKENS: u32 = 1_600;

/// Tokens charged per message for role and framing.
pub const MESSAGE_OVERHEAD_TOKENS: u32 = 4;

/// Estimates how many tokens messages will cost.
pub trait TokenEstimator: Send + Sync {
    /// The estimated token count of `msgs`.
    fn estimate(&self, msgs: &[Message]) -> u32;
}

/// The default estimator: `ceil(chars / 4)`, [`IMAGE_TOKENS`] per image and
/// [`MESSAGE_OVERHEAD_TOKENS`] per message.
#[derive(Debug, Clone, Copy, Default)]
pub struct CharsEstimator;

impl TokenEstimator for CharsEstimator {
    fn estimate(&self, msgs: &[Message]) -> u32 {
        estimate_tokens(msgs)
    }
}

/// Estimates the tokens in a string: `ceil(chars / 4)`.
///
/// # Examples
///
/// ```
/// use cosmos_llm::tokens::estimate_text_tokens;
/// assert_eq!(estimate_text_tokens(""), 0);
/// assert_eq!(estimate_text_tokens("abcde"), 2);
/// ```
pub fn estimate_text_tokens(text: &str) -> u32 {
    (text.chars().count() as u32).div_ceil(4)
}

/// Estimates the tokens in one content block.
///
/// Tool calls count their name and serialized arguments; a tool result counts
/// its nested content.
pub fn estimate_block_tokens(block: &ContentBlock) -> u32 {
    match block {
        ContentBlock::Text { text } | ContentBlock::Thinking { text, .. } => {
            estimate_text_tokens(text)
        }
        ContentBlock::RedactedThinking { data } => estimate_text_tokens(data),
        ContentBlock::Image { .. } => IMAGE_TOKENS,
        ContentBlock::ToolUse { name, input, .. } => {
            estimate_text_tokens(name) + estimate_text_tokens(&input.to_string())
        }
        ContentBlock::ToolResult { content, .. } => content.iter().map(estimate_block_tokens).sum(),
    }
}

/// Estimates the tokens in `msgs` with the default heuristic.
///
/// # Examples
///
/// ```
/// use cosmos_llm::{tokens::estimate_tokens, Message};
/// let n = estimate_tokens(&[Message::user("hello world!")]);
/// assert_eq!(n, 3 + 4); // 12 chars -> 3, plus per-message overhead
/// ```
pub fn estimate_tokens(msgs: &[Message]) -> u32 {
    msgs.iter()
        .map(|m| {
            let blocks: u32 = m.content.iter().map(estimate_block_tokens).sum();
            blocks.saturating_add(MESSAGE_OVERHEAD_TOKENS)
        })
        .fold(0u32, u32::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolCall;

    #[test]
    fn text_rounds_up() {
        assert_eq!(estimate_text_tokens("a"), 1);
        assert_eq!(estimate_text_tokens("abcd"), 1);
        assert_eq!(estimate_text_tokens("abcde"), 2);
    }

    #[test]
    fn counts_every_block_kind() {
        let msg = Message::assistant_blocks(vec![
            ContentBlock::text("abcdefgh"),
            ContentBlock::image_url("https://x/y.png"),
            ContentBlock::tool_use(ToolCall {
                id: "1".into(),
                name: "read".into(),
                input: serde_json::json!({"path": "a"}),
            }),
        ]);
        let n = estimate_tokens(&[msg]);
        assert_eq!(n, 2 + IMAGE_TOKENS + 1 + 3 + MESSAGE_OVERHEAD_TOKENS);
    }

    #[test]
    fn tool_results_count_nested_content() {
        let msg = Message::tool_result("c", "abcdefgh");
        assert_eq!(estimate_tokens(&[msg]), 2 + MESSAGE_OVERHEAD_TOKENS);
    }

    #[test]
    fn estimator_trait_matches_the_function() {
        let msgs = [Message::user("abcd")];
        assert_eq!(CharsEstimator.estimate(&msgs), estimate_tokens(&msgs));
    }
}
