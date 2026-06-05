use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ToolError;

/// The type of a tool parameter, mirroring the Ruby `Parameter::VALID_TYPES`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterType {
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
}

impl ParameterType {
    /// Returns the JSON Schema type string for this parameter type.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::ParameterType;
    ///
    /// assert_eq!(ParameterType::String.json_schema_type(), "string");
    /// assert_eq!(ParameterType::Number.json_schema_type(), "number");
    /// ```
    pub fn json_schema_type(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Integer => "integer",
            Self::Boolean => "boolean",
            Self::Array => "array",
            Self::Object => "object",
        }
    }

    /// Returns `true` if `value` matches this parameter type.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::ParameterType;
    /// use serde_json::json;
    ///
    /// assert!(ParameterType::String.matches(&json!("hello")));
    /// assert!(!ParameterType::String.matches(&json!(42)));
    /// ```
    pub fn matches(&self, value: &Value) -> bool {
        match self {
            Self::String => value.is_string(),
            Self::Number => value.is_number(),
            Self::Integer => value.is_i64() || value.is_u64(),
            Self::Boolean => value.is_boolean(),
            Self::Array => value.is_array(),
            Self::Object => value.is_object(),
        }
    }
}

/// Definition of a single parameter for a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDef {
    /// Parameter name.
    pub name: String,
    /// Expected type.
    pub param_type: ParameterType,
    /// Human-readable description.
    pub description: String,
    /// Whether a caller must supply this parameter.
    pub required: bool,
    /// Allowed values, if any.
    pub enum_values: Option<Vec<Value>>,
    /// Default value applied when the parameter is absent.
    pub default: Option<Value>,
}

impl ParameterDef {
    /// Creates a new parameter definition.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ParameterType, parameter::ParameterDef};
    ///
    /// let p = ParameterDef::new("query", ParameterType::String, true, "Search query");
    /// assert_eq!(p.name, "query");
    /// assert!(p.required);
    /// ```
    pub fn new(
        name: impl Into<String>,
        param_type: ParameterType,
        required: bool,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            param_type,
            description: description.into(),
            required,
            enum_values: None,
            default: None,
        }
    }

    /// Sets the allowed enum values.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ParameterType, parameter::ParameterDef};
    /// use serde_json::json;
    ///
    /// let p = ParameterDef::new("op", ParameterType::String, true, "")
    ///     .with_enum(vec![json!("add"), json!("sub")]);
    /// assert!(p.enum_values.is_some());
    /// ```
    pub fn with_enum(mut self, values: Vec<Value>) -> Self {
        self.enum_values = Some(values);
        self
    }

    /// Sets the default value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ParameterType, parameter::ParameterDef};
    /// use serde_json::json;
    ///
    /// let p = ParameterDef::new("limit", ParameterType::Integer, false, "")
    ///     .with_default(json!(100));
    /// assert_eq!(p.default, Some(json!(100)));
    /// ```
    pub fn with_default(mut self, value: Value) -> Self {
        self.default = Some(value);
        self
    }

    /// Validates a `value` against this parameter's specification.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::MissingParam`] if the value is `null` and the
    /// parameter is required. Returns [`ToolError::Validation`] on type or
    /// enum mismatches.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ParameterType, parameter::ParameterDef};
    /// use serde_json::json;
    ///
    /// let p = ParameterDef::new("n", ParameterType::Integer, true, "");
    /// assert!(p.validate(&json!(5)).is_ok());
    /// assert!(p.validate(&json!(null)).is_err());
    /// ```
    pub fn validate(&self, value: &Value) -> Result<(), ToolError> {
        if value.is_null() {
            if self.required {
                return Err(ToolError::MissingParam(self.name.clone()));
            }
            return Ok(());
        }

        if !self.param_type.matches(value) {
            return Err(ToolError::Validation {
                param: self.name.clone(),
                message: format!(
                    "expected {}, got {}",
                    self.param_type.json_schema_type(),
                    value
                ),
            });
        }

        if let Some(allowed) = &self.enum_values {
            if !allowed.contains(value) {
                return Err(ToolError::Validation {
                    param: self.name.clone(),
                    message: format!("value {} is not in allowed set", value),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn string_match() {
        assert!(ParameterType::String.matches(&json!("hi")));
        assert!(!ParameterType::String.matches(&json!(1)));
    }

    #[test]
    fn required_null_fails() {
        let p = ParameterDef::new("x", ParameterType::String, true, "");
        assert!(p.validate(&json!(null)).is_err());
    }

    #[test]
    fn optional_null_ok() {
        let p = ParameterDef::new("x", ParameterType::String, false, "");
        assert!(p.validate(&json!(null)).is_ok());
    }

    #[test]
    fn enum_violation() {
        let p = ParameterDef::new("op", ParameterType::String, true, "")
            .with_enum(vec![json!("add"), json!("sub")]);
        assert!(p.validate(&json!("mul")).is_err());
        assert!(p.validate(&json!("add")).is_ok());
    }
}
