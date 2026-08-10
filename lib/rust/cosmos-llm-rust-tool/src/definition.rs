use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::error::ToolError;
use crate::executor::Executor;
use crate::parameter::{ParameterDef, ParameterType};
use crate::schemas;

/// A synchronous handler: takes a JSON object of params, returns a JSON value.
pub type HandlerFn = Arc<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;

/// An asynchronous handler, returning a future that resolves to a JSON value.
///
/// Takes `Value` by value rather than by reference: the future outlives the
/// call, so it cannot borrow the caller's parameters.
#[cfg(feature = "async")]
pub type AsyncHandlerFn = Arc<
    dyn Fn(Value) -> futures_util::future::BoxFuture<'static, Result<Value, String>> + Send + Sync,
>;

/// A tool's execution body.
///
/// Both kinds exist because both are genuine. Parsing, arithmetic, and string
/// work are synchronous, and wrapping them in futures costs an allocation and a
/// poll for nothing; network and filesystem work is I/O-bound and must not
/// block the executor. [`Executor::execute`] runs sync handlers only;
/// [`Executor::execute_async`] runs either.
#[derive(Clone)]
#[non_exhaustive]
pub enum Handler {
    /// A handler that returns immediately.
    Sync(HandlerFn),
    /// A handler that returns a future.
    #[cfg(feature = "async")]
    Async(AsyncHandlerFn),
}

impl std::fmt::Debug for Handler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sync(_) => f.write_str("Sync(<handler>)"),
            #[cfg(feature = "async")]
            Self::Async(_) => f.write_str("Async(<handler>)"),
        }
    }
}

impl Handler {
    /// Returns `true` if this handler must be awaited.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ParameterType, ToolDefinition};
    /// use serde_json::json;
    ///
    /// let tool = ToolDefinition::new("t").handler(|_| Ok(json!(1)));
    /// assert!(!tool.handler_ref().unwrap().is_async());
    /// ```
    pub fn is_async(&self) -> bool {
        match self {
            Self::Sync(_) => false,
            #[cfg(feature = "async")]
            Self::Async(_) => true,
        }
    }
}

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
#[derive(Clone)]
pub struct ToolDefinition {
    /// Tool name used for registration and schema generation.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Parameter specifications.
    pub parameters: Vec<ParameterDef>,
    /// Wall-clock limit for one execution, enforced by
    /// [`Executor::execute_async`].
    ///
    /// `None` means unbounded. Set it with [`ToolDefinition::timeout`].
    pub timeout: Option<Duration>,
    pub(crate) handler: Option<Handler>,
}

impl std::fmt::Debug for ToolDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDefinition")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("parameters", &self.parameters)
            .field("timeout", &self.timeout)
            .field("handler", &self.handler)
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
            timeout: None,
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
        self.handler = Some(Handler::Sync(Arc::new(f)));
        self
    }

    /// Attaches an asynchronous execution handler and returns `self`.
    ///
    /// The closure receives the resolved params by value and returns a future.
    /// Use this for anything I/O-bound; a sync handler doing network work
    /// blocks the thread it runs on, and with a bounded executor that stalls
    /// every other tool in the batch.
    ///
    /// Async tools can only be run by [`Executor::execute_async`].
    /// [`Executor::execute`] returns [`ToolError::AsyncHandler`] for them,
    /// rather than blocking to drive the future — which would deadlock on a
    /// current-thread runtime.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{Executor, ParameterType, ToolDefinition};
    /// use serde_json::json;
    ///
    /// # tokio_test::block_on(async {
    /// let tool = ToolDefinition::new("slow_double")
    ///     .param("n", ParameterType::Number, true, "")
    ///     .async_handler(|params| Box::pin(async move {
    ///         let n = params["n"].as_f64().ok_or("n must be a number")?;
    ///         Ok(json!(n * 2.0))
    ///     }));
    ///
    /// let result = Executor::execute_async(&tool, json!({ "n": 21.0 })).await.unwrap();
    /// assert_eq!(result, json!(42.0));
    /// # })
    /// ```
    #[cfg(feature = "async")]
    pub fn async_handler<F>(mut self, f: F) -> Self
    where
        F: Fn(Value) -> futures_util::future::BoxFuture<'static, Result<Value, String>>
            + Send
            + Sync
            + 'static,
    {
        self.handler = Some(Handler::Async(Arc::new(f)));
        self
    }

    /// Sets a wall-clock timeout for one execution and returns `self`.
    ///
    /// Enforced by [`Executor::execute_async`]. A tool that hangs must not hang
    /// the agent loop, and putting the limit here rather than at each call site
    /// keeps it from being applied inconsistently — or forgotten.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::time::Duration;
    /// use cosmos_llm_tool::ToolDefinition;
    /// use serde_json::json;
    ///
    /// let tool = ToolDefinition::new("fetch")
    ///     .timeout(Duration::from_secs(10))
    ///     .handler(|_| Ok(json!(null)));
    /// assert_eq!(tool.timeout, Some(Duration::from_secs(10)));
    /// ```
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Returns this tool's handler, if one is set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::ToolDefinition;
    /// use serde_json::json;
    ///
    /// assert!(ToolDefinition::new("t").handler_ref().is_none());
    /// assert!(ToolDefinition::new("t")
    ///     .handler(|_| Ok(json!(1)))
    ///     .handler_ref()
    ///     .is_some());
    /// ```
    pub fn handler_ref(&self) -> Option<&Handler> {
        self.handler.as_ref()
    }

    /// Returns `true` if this tool's handler must be awaited.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::ToolDefinition;
    /// use serde_json::json;
    ///
    /// let sync = ToolDefinition::new("t").handler(|_| Ok(json!(1)));
    /// assert!(!sync.is_async());
    /// // A tool with no handler is not async; it is unrunnable either way.
    /// assert!(!ToolDefinition::new("t").is_async());
    /// ```
    pub fn is_async(&self) -> bool {
        self.handler.as_ref().is_some_and(Handler::is_async)
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

    /// Executes the tool, awaiting an async handler if it has one.
    ///
    /// Shorthand for [`Executor::execute_async`]. Works for both handler kinds,
    /// and enforces [`ToolDefinition::timeout`].
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::NoHandler`] if no handler has been set,
    /// [`ToolError::Validation`] on parameter validation failures,
    /// [`ToolError::Timeout`] if the tool exceeds its timeout, and
    /// [`ToolError::Execution`] if the handler itself errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::ToolDefinition;
    /// use serde_json::json;
    ///
    /// # tokio_test::block_on(async {
    /// let t = ToolDefinition::new("hi").handler(|_| Ok(json!("hello")));
    /// assert_eq!(t.call_async(json!({})).await.unwrap(), json!("hello"));
    /// # })
    /// ```
    #[cfg(feature = "async")]
    pub async fn call_async(&self, params: Value) -> Result<Value, ToolError> {
        Executor::execute_async(self, params).await
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
