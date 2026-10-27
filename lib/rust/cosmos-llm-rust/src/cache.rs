//! Default prompt-cache breakpoint placement.

use crate::content::{CacheHint, Role};
use crate::types::CompletionRequest;

/// Marks the cache breakpoints nearly every agent wants.
///
/// Anthropic caches the prefix up to each marked block (four at most per
/// request); OpenAI caches prefixes on its own and ignores the marks. This
/// marks, in prefix order:
///
/// 1. the tool definitions ([`CompletionRequest::cache_tools`]),
/// 2. the last system message,
/// 3. the second-newest message, and
/// 4. the newest message.
///
/// The last two move forward every turn, so the history prefix written by the
/// previous request is read by this one. Earlier message hints are cleared
/// first, which makes the call idempotent and safe to repeat each turn.
///
/// It is a helper rather than automatic behaviour because a one-shot caller
/// pays the cache-write premium (25% on Anthropic) for a cache it never reads.
///
/// # Examples
///
/// ```
/// use cosmos_llm::{apply_default_cache_breakpoints, CacheHint, CompletionRequest, Message};
/// use serde_json::json;
///
/// let mut req = CompletionRequest::new(
///     "claude-sonnet-4-6",
///     vec![
///         Message::system("rules"),
///         Message::user("one"),
///         Message::assistant("two"),
///         Message::user("three"),
///     ],
/// )
/// .with_tools(vec![json!({"name": "read"})]);
///
/// apply_default_cache_breakpoints(&mut req);
/// assert_eq!(req.cache_tools, Some(CacheHint::Ephemeral));
/// assert!(req.messages[0].cache.is_some()); // system
/// assert!(req.messages[1].cache.is_none());
/// assert!(req.messages[2].cache.is_some()); // second-newest
/// assert!(req.messages[3].cache.is_some()); // newest
/// ```
pub fn apply_default_cache_breakpoints(req: &mut CompletionRequest) {
    for msg in &mut req.messages {
        msg.cache = None;
    }

    req.cache_tools = req
        .tools
        .as_ref()
        .is_some_and(|t| !t.is_empty())
        .then_some(CacheHint::Ephemeral);

    if let Some(system) = req
        .messages
        .iter_mut()
        .rev()
        .find(|m| m.role == Role::System)
    {
        system.cache = Some(CacheHint::Ephemeral);
    }

    // Only chat messages carry the moving breakpoints.
    let chat: Vec<usize> = req
        .messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role != Role::System)
        .map(|(i, _)| i)
        .collect();
    for &i in chat.iter().rev().take(2) {
        req.messages[i].cache = Some(CacheHint::Ephemeral);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    fn marked(req: &CompletionRequest) -> Vec<bool> {
        req.messages.iter().map(|m| m.cache.is_some()).collect()
    }

    #[test]
    fn marks_system_and_the_two_newest_chat_messages() {
        let mut req = CompletionRequest::new(
            "m",
            vec![
                Message::system("s"),
                Message::user("1"),
                Message::assistant("2"),
                Message::user("3"),
            ],
        );
        apply_default_cache_breakpoints(&mut req);
        assert_eq!(marked(&req), [true, false, true, true]);
        // No tools, no tool breakpoint.
        assert_eq!(req.cache_tools, None);
    }

    #[test]
    fn the_breakpoints_move_forward_and_never_exceed_four() {
        let mut req = CompletionRequest::new("m", vec![Message::system("s"), Message::user("1")])
            .with_tools(vec![serde_json::json!({"name": "t"})]);
        apply_default_cache_breakpoints(&mut req);
        req.messages.push(Message::assistant("2"));
        req.messages.push(Message::user("3"));
        apply_default_cache_breakpoints(&mut req);

        assert_eq!(marked(&req), [true, false, true, true]);
        let hints = req.messages.iter().filter(|m| m.cache.is_some()).count()
            + usize::from(req.cache_tools.is_some());
        assert_eq!(hints, 4);
    }

    #[test]
    fn a_single_message_is_marked_once() {
        let mut req = CompletionRequest::new("m", vec![Message::user("only")]);
        apply_default_cache_breakpoints(&mut req);
        assert_eq!(marked(&req), [true]);
    }
}
