use serde_json::{Map, Value};

use crate::definition::ToolDefinition;
use crate::error::ToolError;

/// Executes a [`ToolDefinition`] against a set of parameters.
///
/// Validates all parameters, applies defaults, then invokes the handler.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{ToolDefinition, ParameterType, Executor};
/// use serde_json::json;
///
/// let tool = ToolDefinition::new("greet")
///     .param("name", ParameterType::String, true, "Name to greet")
///     .handler(|p| {
///         Ok(json!(format!("Hello, {}!", p["name"].as_str().unwrap())))
///     });
///
/// let result = Executor::execute(&tool, &json!({ "name": "Alice" })).unwrap();
/// assert_eq!(result, json!("Hello, Alice!"));
/// ```
pub struct Executor;

impl Executor {
    /// Validates parameters, applies defaults, and calls the tool handler.
    ///
    /// `params` must be a JSON object; other shapes return a validation error.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::NoHandler`] if the tool has no handler.
    /// Returns [`ToolError::Validation`] or [`ToolError::MissingParam`] on bad input.
    /// Returns [`ToolError::Execution`] if the handler returns an error string.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ToolDefinition, ParameterType, Executor};
    /// use serde_json::json;
    ///
    /// let tool = ToolDefinition::new("noop").handler(|_| Ok(json!(null)));
    /// assert!(Executor::execute(&tool, &json!({})).is_ok());
    /// ```
    pub fn execute(tool: &ToolDefinition, params: &Value) -> Result<Value, ToolError> {
        let handler = tool
            .handler
            .as_ref()
            .ok_or_else(|| ToolError::NoHandler(tool.name.clone()))?;

        let obj = match params {
            Value::Object(m) => m.clone(),
            Value::Null => Map::new(),
            _ => {
                return Err(ToolError::Validation {
                    param: "(root)".into(),
                    message: "params must be a JSON object".into(),
                })
            }
        };

        // Validate and apply defaults
        let mut resolved = Map::new();
        for param in &tool.parameters {
            let value = obj.get(&param.name).cloned().unwrap_or(Value::Null);

            // Apply default before validation
            let effective = if value.is_null() {
                param.default.clone().unwrap_or(Value::Null)
            } else {
                value
            };

            param.validate(&effective)?;

            if !effective.is_null() {
                resolved.insert(param.name.clone(), effective);
            }
        }

        let resolved_value = Value::Object(resolved);

        handler(&resolved_value).map_err(|msg| ToolError::Execution {
            tool: tool.name.clone(),
            message: msg,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameter::{ParameterDef, ParameterType};
    use serde_json::json;

    fn make_tool() -> ToolDefinition {
        ToolDefinition::new("calc")
            .param("a", ParameterType::Number, true, "")
            .param("b", ParameterType::Number, true, "")
            .handler(|p| {
                let a = p["a"].as_f64().unwrap();
                let b = p["b"].as_f64().unwrap();
                Ok(json!(a + b))
            })
    }

    #[test]
    fn happy_path() {
        let t = make_tool();
        let r = Executor::execute(&t, &json!({ "a": 1.0, "b": 2.0 })).unwrap();
        assert_eq!(r, json!(3.0));
    }

    #[test]
    fn missing_required() {
        let t = make_tool();
        assert!(Executor::execute(&t, &json!({ "a": 1.0 })).is_err());
    }

    #[test]
    fn default_applied() {
        let tool = ToolDefinition::new("t")
            .push_param(
                ParameterDef::new("limit", ParameterType::Integer, false, "")
                    .with_default(json!(10)),
            )
            .handler(|p| Ok(p["limit"].clone()));

        let r = Executor::execute(&tool, &json!({})).unwrap();
        assert_eq!(r, json!(10));
    }

    #[test]
    fn no_handler_error() {
        let t = ToolDefinition::new("t");
        assert!(matches!(
            Executor::execute(&t, &json!({})),
            Err(ToolError::NoHandler(_))
        ));
    }
}
