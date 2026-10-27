//! Typed message content: roles, content blocks, and cache hints.
//!
//! A [`Message`](crate::Message) is a role plus an ordered list of
//! [`ContentBlock`]s. Plain text is one `Text` block; everything a coding
//! agent needs beyond that (images, signed thinking, tool calls and their
//! results) is another variant. These types are the IR providers lower to and
//! from wire JSON, so they hold data only.

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::ToolCall;

/// Who authored a message.
///
/// Anthropic has no `tool` role on the wire; its provider folds `Tool`
/// messages into `user` turns. The variant exists so a tool result is never
/// mistaken for something the user typed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Role {
    /// Instructions to the model.
    System,
    /// Input from the user.
    #[default]
    User,
    /// Output from the model.
    Assistant,
    /// The result of a tool call.
    Tool,
}

impl Role {
    /// The wire name: `"system"`, `"user"`, `"assistant"` or `"tool"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Role;
    /// assert_eq!(Role::Assistant.as_str(), "assistant");
    /// ```
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }

    /// Parses a wire name, returning `None` for anything unrecognized.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Role;
    /// assert_eq!(Role::parse("tool"), Some(Role::Tool));
    /// assert_eq!(Role::parse("developer"), None);
    /// ```
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "system" => Some(Self::System),
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "tool" => Some(Self::Tool),
            _ => None,
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for Role {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// A prompt-cache breakpoint, attached to a message (see
/// [`Message::with_cache`](crate::Message::with_cache)).
///
/// Only Anthropic (and OpenRouter, for Anthropic models) acts on it; other
/// providers ignore it. OpenAI caches prefixes on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CacheHint {
    /// The default five-minute cache.
    Ephemeral,
    /// Anthropic's one-hour TTL option.
    EphemeralLong,
}

impl CacheHint {
    /// The `cache_control` object Anthropic expects.
    pub(crate) fn to_cache_control(self) -> Value {
        match self {
            Self::Ephemeral => serde_json::json!({ "type": "ephemeral" }),
            Self::EphemeralLong => serde_json::json!({ "type": "ephemeral", "ttl": "1h" }),
        }
    }
}

/// Where an image's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ImageSource {
    /// Inline bytes, base64 encoded.
    Base64 {
        /// MIME type, e.g. `"image/png"`.
        media_type: String,
        /// Base64 payload, without a `data:` prefix.
        data: String,
    },
    /// An image the provider fetches itself.
    Url {
        /// Absolute URL.
        url: String,
    },
}

/// One piece of a message.
///
/// Providers lower these to their own wire shapes; the table is in the crate
/// README. A response keeps every block the provider returned, in order,
/// including thinking, so an assistant turn can be replayed unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContentBlock {
    /// Plain text.
    Text {
        /// The text.
        text: String,
    },
    /// An image.
    Image {
        /// Where the bytes are.
        source: ImageSource,
    },
    /// Model reasoning. Anthropic requires the signed block to be replayed,
    /// unchanged, on the assistant turn that holds a `tool_use`.
    Thinking {
        /// Reasoning text (a summary, for some models).
        text: String,
        /// Provider signature that makes the block replayable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    /// Anthropic's `redacted_thinking`: opaque, replayed verbatim.
    RedactedThinking {
        /// Encrypted payload.
        data: String,
    },
    /// A tool call requested by the model.
    ToolUse {
        /// Identifier the matching result refers to.
        id: String,
        /// Tool name.
        name: String,
        /// Parsed arguments.
        input: Value,
    },
    /// The result of a tool call. Its content may mix text and images.
    ToolResult {
        /// The `id` of the [`ContentBlock::ToolUse`] this answers.
        tool_use_id: String,
        /// Result content.
        content: Vec<ContentBlock>,
        /// Whether the tool failed.
        #[serde(default, skip_serializing_if = "is_false")]
        is_error: bool,
    },
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl ContentBlock {
    /// A text block.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::ContentBlock;
    /// assert_eq!(ContentBlock::text("hi").as_text(), Some("hi"));
    /// ```
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// An image block from raw bytes, base64 encoded here.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::ContentBlock;
    /// let block = ContentBlock::image_base64("image/png", [0x89, b'P', b'N', b'G']);
    /// assert!(matches!(block, ContentBlock::Image { .. }));
    /// ```
    pub fn image_base64(media_type: impl Into<String>, bytes: impl AsRef<[u8]>) -> Self {
        Self::Image {
            source: ImageSource::Base64 {
                media_type: media_type.into(),
                data: base64::engine::general_purpose::STANDARD.encode(bytes),
            },
        }
    }

    /// An image block the provider fetches from `url`.
    pub fn image_url(url: impl Into<String>) -> Self {
        Self::Image {
            source: ImageSource::Url { url: url.into() },
        }
    }

    /// A tool-use block from a [`ToolCall`].
    pub fn tool_use(call: ToolCall) -> Self {
        Self::ToolUse {
            id: call.id,
            name: call.name,
            input: call.input,
        }
    }

    /// The text of a `Text` block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Whether this is a `Thinking` or `RedactedThinking` block.
    pub fn is_thinking(&self) -> bool {
        matches!(self, Self::Thinking { .. } | Self::RedactedThinking { .. })
    }
}
