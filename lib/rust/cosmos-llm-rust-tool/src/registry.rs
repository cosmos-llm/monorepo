use std::collections::HashMap;

use crate::definition::ToolDefinition;

/// A named store of [`ToolDefinition`]s.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{Registry, ToolDefinition};
///
/// let mut registry = Registry::new();
/// let tool = ToolDefinition::new("search");
/// registry.register(tool);
///
/// assert!(registry.get("search").is_some());
/// assert_eq!(registry.count(), 1);
/// ```
#[derive(Default, Debug)]
pub struct Registry {
    tools: HashMap<String, ToolDefinition>,
}

impl Registry {
    /// Creates an empty registry.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::Registry;
    ///
    /// let r = Registry::new();
    /// assert_eq!(r.count(), 0);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a tool, replacing any existing tool with the same name.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{Registry, ToolDefinition};
    ///
    /// let mut r = Registry::new();
    /// r.register(ToolDefinition::new("foo"));
    /// assert!(r.registered("foo"));
    /// ```
    pub fn register(&mut self, tool: ToolDefinition) {
        self.tools.insert(tool.name.clone(), tool);
    }

    /// Returns a reference to a tool by name, or `None`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{Registry, ToolDefinition};
    ///
    /// let mut r = Registry::new();
    /// r.register(ToolDefinition::new("bar"));
    /// assert!(r.get("bar").is_some());
    /// assert!(r.get("missing").is_none());
    /// ```
    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    /// Returns `true` if a tool with `name` is registered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{Registry, ToolDefinition};
    ///
    /// let mut r = Registry::new();
    /// r.register(ToolDefinition::new("x"));
    /// assert!(r.registered("x"));
    /// assert!(!r.registered("y"));
    /// ```
    pub fn registered(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Removes and returns a tool by name.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{Registry, ToolDefinition};
    ///
    /// let mut r = Registry::new();
    /// r.register(ToolDefinition::new("z"));
    /// assert!(r.unregister("z").is_some());
    /// assert!(!r.registered("z"));
    /// ```
    pub fn unregister(&mut self, name: &str) -> Option<ToolDefinition> {
        self.tools.remove(name)
    }

    /// Returns all registered tools.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{Registry, ToolDefinition};
    ///
    /// let mut r = Registry::new();
    /// r.register(ToolDefinition::new("a"));
    /// r.register(ToolDefinition::new("b"));
    /// assert_eq!(r.all().len(), 2);
    /// ```
    pub fn all(&self) -> Vec<&ToolDefinition> {
        self.tools.values().collect()
    }

    /// Returns all registered tool names.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{Registry, ToolDefinition};
    ///
    /// let mut r = Registry::new();
    /// r.register(ToolDefinition::new("ping"));
    /// assert!(r.names().contains(&&"ping".to_string()));
    /// ```
    pub fn names(&self) -> Vec<&String> {
        self.tools.keys().collect()
    }

    /// Removes all registered tools.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{Registry, ToolDefinition};
    ///
    /// let mut r = Registry::new();
    /// r.register(ToolDefinition::new("t"));
    /// r.clear();
    /// assert_eq!(r.count(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.tools.clear();
    }

    /// Returns the number of registered tools.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::Registry;
    ///
    /// let r = Registry::new();
    /// assert_eq!(r.count(), 0);
    /// ```
    pub fn count(&self) -> usize {
        self.tools.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_get() {
        let mut r = Registry::new();
        r.register(ToolDefinition::new("foo"));
        assert!(r.get("foo").is_some());
    }

    #[test]
    fn unregister() {
        let mut r = Registry::new();
        r.register(ToolDefinition::new("foo"));
        assert!(r.unregister("foo").is_some());
        assert!(!r.registered("foo"));
    }

    #[test]
    fn clear_empties() {
        let mut r = Registry::new();
        r.register(ToolDefinition::new("a"));
        r.register(ToolDefinition::new("b"));
        r.clear();
        assert_eq!(r.count(), 0);
    }
}
