use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ContextError;

/// An immutable, named content block within an LLM context.
///
/// Blocks can represent system prompts, user messages, tool declarations,
/// or any other labelled content. Once constructed they are immutable;
/// use [`with_content`] and [`with_metadata`] to derive modified copies.
///
/// # Examples
///
/// ```rust
/// use std::collections::HashMap;
/// use cosmos_llm_context::Block;
///
/// let b = Block::new("system", "You are helpful.", HashMap::new()).unwrap();
/// assert_eq!(b.name, "system");
/// assert!(b.is_type("system"));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    /// The block type / role name (e.g. `"system"`, `"user"`).
    pub name: String,
    /// The block's text content.
    pub content: String,
    /// Arbitrary metadata attached to this block.
    pub metadata: HashMap<String, Value>,
}

impl Block {
    /// Creates a new block.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::InvalidName`] if `name` is empty.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::HashMap;
    /// use cosmos_llm_context::Block;
    ///
    /// let b = Block::new("user", "Hello!", HashMap::new()).unwrap();
    /// assert_eq!(b.content, "Hello!");
    /// ```
    pub fn new(
        name: impl Into<String>,
        content: impl Into<String>,
        metadata: HashMap<String, Value>,
    ) -> Result<Self, ContextError> {
        let name = name.into();
        if name.is_empty() {
            return Err(ContextError::InvalidName("name cannot be empty".into()));
        }
        Ok(Self {
            name,
            content: content.into(),
            metadata,
        })
    }

    /// Returns `true` if this block's name matches `block_type` (case-sensitive).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::HashMap;
    /// use cosmos_llm_context::Block;
    ///
    /// let b = Block::new("system", "...", HashMap::new()).unwrap();
    /// assert!(b.is_type("system"));
    /// assert!(!b.is_type("user"));
    /// ```
    pub fn is_type(&self, block_type: &str) -> bool {
        self.name == block_type
    }

    /// Gets a metadata value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::HashMap;
    /// use cosmos_llm_context::Block;
    /// use serde_json::json;
    ///
    /// let meta = [("priority".to_string(), json!(1))].into();
    /// let b = Block::new("user", "hi", meta).unwrap();
    /// assert_eq!(b.meta("priority"), Some(&json!(1)));
    /// ```
    pub fn meta(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    /// Returns a copy of this block with updated content.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::HashMap;
    /// use cosmos_llm_context::Block;
    ///
    /// let b = Block::new("user", "old", HashMap::new()).unwrap();
    /// let b2 = b.with_content("new");
    /// assert_eq!(b2.content, "new");
    /// ```
    pub fn with_content(&self, content: impl Into<String>) -> Self {
        Self {
            name: self.name.clone(),
            content: content.into(),
            metadata: self.metadata.clone(),
        }
    }

    /// Returns a copy of this block with merged metadata.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::HashMap;
    /// use cosmos_llm_context::Block;
    /// use serde_json::json;
    ///
    /// let b = Block::new("system", "hi", HashMap::new()).unwrap();
    /// let b2 = b.with_metadata([("role".to_string(), json!("system"))].into());
    /// assert!(b2.meta("role").is_some());
    /// ```
    pub fn with_metadata(&self, updates: HashMap<String, Value>) -> Self {
        let mut meta = self.metadata.clone();
        meta.extend(updates);
        Self {
            name: self.name.clone(),
            content: self.content.clone(),
            metadata: meta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_name_is_invalid() {
        assert!(Block::new("", "content", HashMap::new()).is_err());
    }

    #[test]
    fn type_check() {
        let b = Block::new("system", "hi", HashMap::new()).unwrap();
        assert!(b.is_type("system"));
        assert!(!b.is_type("user"));
    }

    #[test]
    fn with_content_is_copy() {
        let b = Block::new("user", "old", HashMap::new()).unwrap();
        let b2 = b.with_content("new");
        assert_eq!(b.content, "old");
        assert_eq!(b2.content, "new");
    }

    #[test]
    fn meta_lookup() {
        let meta = [("k".to_string(), json!("v"))].into();
        let b = Block::new("user", "hi", meta).unwrap();
        assert_eq!(b.meta("k"), Some(&json!("v")));
        assert_eq!(b.meta("missing"), None);
    }
}
