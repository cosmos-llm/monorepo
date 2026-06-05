use std::collections::HashMap;

use cosmos_llm_virtual_filesystem::Filesystem;
use serde_json::Value;

use crate::block::Block;
use crate::renderers::RenderFormat;

/// Builder for assembling an LLM context from blocks and an optional filesystem.
///
/// Use the fluent API to add blocks, attach a virtual filesystem, and then
/// call [`render`] to produce a provider-specific string.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_context::{ContextBuilder, RenderFormat};
///
/// let output = ContextBuilder::new()
///     .block("system", "You are helpful.", Default::default())
///     .block("user", "Explain Rust lifetimes.", Default::default())
///     .render(RenderFormat::Default);
///
/// assert!(output.contains("You are helpful."));
/// ```
#[derive(Debug, Default)]
pub struct ContextBuilder {
    /// Ordered list of content blocks.
    pub blocks: Vec<Block>,
    /// Optional root filesystem attached to the context.
    pub filesystem: Option<Filesystem>,
}

impl ContextBuilder {
    /// Creates a new, empty builder.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_context::ContextBuilder;
    ///
    /// let b = ContextBuilder::new();
    /// assert!(b.blocks.is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a block and returns `self` for chaining.
    ///
    /// Blocks whose name is empty are silently skipped.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_context::ContextBuilder;
    ///
    /// let b = ContextBuilder::new()
    ///     .block("system", "hi", Default::default());
    /// assert_eq!(b.blocks.len(), 1);
    /// ```
    pub fn block(
        mut self,
        name: impl Into<String>,
        content: impl Into<String>,
        metadata: HashMap<String, Value>,
    ) -> Self {
        if let Ok(b) = Block::new(name, content, metadata) {
            self.blocks.push(b);
        }
        self
    }

    /// Appends a pre-built [`Block`] and returns `self`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::collections::HashMap;
    /// use cosmos_llm_context::{Block, ContextBuilder};
    ///
    /// let b = Block::new("user", "Hello!", HashMap::new()).unwrap();
    /// let ctx = ContextBuilder::new().push_block(b);
    /// assert_eq!(ctx.blocks.len(), 1);
    /// ```
    pub fn push_block(mut self, block: Block) -> Self {
        self.blocks.push(block);
        self
    }

    /// Attaches a virtual filesystem to the context.
    ///
    /// Replaces any previously attached filesystem.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_context::ContextBuilder;
    /// use cosmos_llm_virtual_filesystem::Filesystem;
    ///
    /// let fs = Filesystem::new("/");
    /// let ctx = ContextBuilder::new().with_filesystem(fs);
    /// assert!(ctx.filesystem.is_some());
    /// ```
    pub fn with_filesystem(mut self, fs: Filesystem) -> Self {
        self.filesystem = Some(fs);
        self
    }

    /// Renders the assembled context using the requested format.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_context::{ContextBuilder, RenderFormat};
    ///
    /// let out = ContextBuilder::new()
    ///     .block("user", "test", Default::default())
    ///     .render(RenderFormat::Xml);
    ///
    /// assert!(out.contains("<context>"));
    /// ```
    pub fn render(&self, format: RenderFormat) -> String {
        format.render(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderers::RenderFormat;

    #[test]
    fn blocks_accumulate() {
        let ctx = ContextBuilder::new()
            .block("system", "s", Default::default())
            .block("user", "u", Default::default());
        assert_eq!(ctx.blocks.len(), 2);
    }

    #[test]
    fn render_default_contains_content() {
        let out = ContextBuilder::new()
            .block("system", "hello world", Default::default())
            .render(RenderFormat::Default);
        assert!(out.contains("hello world"));
    }
}
