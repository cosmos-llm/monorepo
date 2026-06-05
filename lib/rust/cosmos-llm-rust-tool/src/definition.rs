use serde_json::Value;

use crate::error::ToolError;
use crate::executor::Executor;
use crate::parameter::{ParameterDef, ParameterType};
use crate::schemas;

/// A handler function: takes a JSON object of params, returns a JSON value.
pub type HandlerFn = Box<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;

/// Definition of a callable tool — schema plus optional execution handler.
///
/// Build one with the fluent API, then call it via [`Executor::execute`] or
/// the shorthand [`call`].
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{ToolDefinition, ParameterType, Executor};
/// use serde_json::json;
///
/// let tool = ToolDefinition::new("echo")
///     .description("Echoes its input")
///     .param("msg", ParameterType::String, true, "Message to echo")
///     .handler(|params| Ok(params["msg"].clone()));
///
/// let result = Executor::execute(&tool, &json!({ "msg": "hello" })).unwrap();
/// assert_eq!(result, json!("hello"));
/// ```
pub struct ToolDefinition {
    /// Tool name used for registration and schema generation.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Parameter specifications.
    pub parameters: Vec<ParameterDef>,
    pub(crate) handler: Option<HandlerFn>,
}

impl std::fmt::Debug for ToolDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDefinition")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("parameters", &self.parameters)
            .field("handler", &self.handler.is_some())
            .finish()
    }
}

impl ToolDefinition {
    /// Creates a new tool definition with the given name.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::ToolDefinition;
    ///
    /// let t = ToolDefinition::new("my_tool");
    /// assert_eq!(t.name, "my_tool");
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            parameters: Vec::new(),
            handler: None,
        }
    }

    /// Sets the description and returns `self`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::ToolDefinition;
    ///
    /// let t = ToolDefinition::new("t").description("does things");
    /// assert_eq!(t.description, "does things");
    /// ```
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Adds a parameter and returns `self`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ToolDefinition, ParameterType};
    ///
    /// let t = ToolDefinition::new("t")
    ///     .param("x", ParameterType::Integer, true, "A number");
    /// assert_eq!(t.parameters.len(), 1);
    /// ```
    pub fn param(
        mut self,
        name: impl Into<String>,
        param_type: ParameterType,
        required: bool,
        description: impl Into<String>,
    ) -> Self {
        self.parameters
            .push(ParameterDef::new(name, param_type, required, description));
        self
    }

    /// Adds a pre-built [`ParameterDef`] and returns `self`.
    ///
    /// Use this when you need enum values or defaults.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ToolDefinition, ParameterType, parameter::ParameterDef};
    /// use serde_json::json;
    ///
    /// let p = ParameterDef::new("op", ParameterType::String, true, "")
    ///     .with_enum(vec![json!("add"), json!("sub")]);
    /// let t = ToolDefinition::new("calc").push_param(p);
    /// assert_eq!(t.parameters.len(), 1);
    /// ```
    pub fn push_param(mut self, param: ParameterDef) -> Self {
        self.parameters.push(param);
        self
    }

    /// Attaches an execution handler and returns `self`.
    ///
    /// The closure receives the full `params` JSON object and returns either a
    /// `Value` or an error string.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ToolDefinition, ParameterType, Executor};
    /// use serde_json::json;
    ///
    /// let t = ToolDefinition::new("add")
    ///     .param("a", ParameterType::Number, true, "")
    ///     .param("b", ParameterType::Number, true, "")
    ///     .handler(|p| {
    ///         let a = p["a"].as_f64().unwrap();
    ///         let b = p["b"].as_f64().unwrap();
    ///         Ok(json!(a + b))
    ///     });
    ///
    /// let result = Executor::execute(&t, &json!({ "a": 3.0, "b": 4.0 })).unwrap();
    /// assert_eq!(result, json!(7.0));
    /// ```
    pub fn handler<F>(mut self, f: F) -> Self
    where
        F: Fn(&Value) -> Result<Value, String> + Send + Sync + 'static,
    {
        self.handler = Some(Box::new(f));
        self
    }

    /// Executes the tool with the given parameters.
    ///
    /// Shorthand for [`Executor::execute`].
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::NoHandler`] if no handler has been set.
    /// Returns [`ToolError::Validation`] on parameter validation failures.
    /// Returns [`ToolError::Execution`] if the handler itself errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ToolDefinition, ParameterType};
    /// use serde_json::json;
    ///
    /// let t = ToolDefinition::new("hi")
    ///     .handler(|_| Ok(json!("hello")));
    ///
    /// assert_eq!(t.call(&json!({})).unwrap(), json!("hello"));
    /// ```
    pub fn call(&self, params: &Value) -> Result<Value, ToolError> {
        Executor::execute(self, params)
    }

    /// Generates an OpenAI function-calling schema for this tool.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ToolDefinition, ParameterType};
    ///
    /// let t = ToolDefinition::new("search")
    ///     .param("query", ParameterType::String, true, "Search query");
    ///
    /// let schema = t.to_openai_schema();
    /// assert_eq!(schema["type"], "function");
    /// ```
    pub fn to_openai_schema(&self) -> Value {
        schemas::openai_schema(self)
    }

    /// Generates an Anthropic tool schema for this tool.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ToolDefinition, ParameterType};
    ///
    /// let t = ToolDefinition::new("search")
    ///     .param("query", ParameterType::String, true, "Search query");
    ///
    /// let schema = t.to_anthropic_schema();
    /// assert_eq!(schema["name"], "search");
    /// ```
    pub fn to_anthropic_schema(&self) -> Value {
        schemas::anthropic_schema(self)
    }

    /// Generates a JSON Schema for this tool.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ToolDefinition, ParameterType};
    ///
    /// let t = ToolDefinition::new("x");
    /// let schema = t.to_json_schema();
    /// assert_eq!(schema["type"], "object");
    /// ```
    pub fn to_json_schema(&self) -> Value {
        schemas::json_schema(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn call_no_handler() {
        let t = ToolDefinition::new("t");
        assert!(matches!(t.call(&json!({})), Err(ToolError::NoHandler(_))));
    }

    #[test]
    fn call_with_handler() {
        let t = ToolDefinition::new("t").handler(|_| Ok(json!(42)));
        assert_eq!(t.call(&json!({})).unwrap(), json!(42));
    }
}
