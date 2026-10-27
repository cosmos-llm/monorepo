use serde::{Deserialize, Serialize};

/// One chat message, as an adapter produces it.
///
/// Deliberately the minimal shape every provider understands: a role and
/// text. It serializes with the same field names as the client crate's
/// message type, so converting between the two is a field-for-field copy and
/// this crate stays free of an HTTP stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// `"system"`, `"user"`, or `"assistant"`.
    pub role: String,
    /// The message text.
    pub content: String,
}

impl Message {
    /// Builds a message with any role.
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    /// Builds a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::new("system", content)
    }

    /// Builds a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }

    /// Builds an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", content)
    }
}
