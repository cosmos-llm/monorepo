use serde_json::{json, Value};

use crate::definition::ToolDefinition;
use crate::parameter::ParameterDef;

/// Generates an OpenAI function-calling schema from a tool definition.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{ToolDefinition, ParameterType};
/// use cosmos_llm_tool::schemas::openai_schema;
///
/// let t = ToolDefinition::new("search")
///     .description("Search the web")
///     .param("query", ParameterType::String, true, "Search terms");
///
/// let s = openai_schema(&t);
/// assert_eq!(s["type"], "function");
/// assert_eq!(s["function"]["name"], "search");
/// ```
pub fn openai_schema(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": parameters_schema(tool),
        }
    })
}

/// Generates an Anthropic tool schema from a tool definition.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{ToolDefinition, ParameterType};
/// use cosmos_llm_tool::schemas::anthropic_schema;
///
/// let t = ToolDefinition::new("calc")
///     .param("x", ParameterType::Number, true, "operand");
///
/// let s = anthropic_schema(&t);
/// assert_eq!(s["name"], "calc");
/// assert!(s["input_schema"].is_object());
/// ```
pub fn anthropic_schema(tool: &ToolDefinition) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": parameters_schema(tool),
    })
}

/// Generates a JSON Schema object for a tool definition.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{ToolDefinition, ParameterType};
/// use cosmos_llm_tool::schemas::json_schema;
///
/// let t = ToolDefinition::new("ping");
/// let s = json_schema(&t);
/// assert_eq!(s["type"], "object");
/// assert_eq!(s["title"], "ping");
/// ```
pub fn json_schema(tool: &ToolDefinition) -> Value {
    let mut schema = parameters_schema(tool);
    schema["title"] = json!(tool.name);
    schema["description"] = json!(tool.description);
    schema["$schema"] = json!("http://json-schema.org/draft-07/schema#");
    schema
}

fn parameters_schema(tool: &ToolDefinition) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required: Vec<Value> = Vec::new();

    for param in &tool.parameters {
        properties.insert(param.name.clone(), param_property(param));
        if param.required {
            required.push(json!(param.name));
        }
    }

    let mut schema = json!({
        "type": "object",
        "properties": properties,
    });

    if !required.is_empty() {
        schema["required"] = json!(required);
    }

    schema
}

fn param_property(param: &ParameterDef) -> Value {
    let mut prop = json!({ "type": param.param_type.json_schema_type() });

    if !param.description.is_empty() {
        prop["description"] = json!(param.description);
    }
    if let Some(vals) = &param.enum_values {
        prop["enum"] = json!(vals);
    }
    if let Some(def) = &param.default {
        prop["default"] = def.clone();
    }

    prop
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameter::ParameterType;

    #[test]
    fn openai_structure() {
        let t = ToolDefinition::new("test").param("q", ParameterType::String, true, "query");
        let s = openai_schema(&t);
        assert_eq!(s["type"], "function");
        assert!(s["function"]["parameters"]["properties"]["q"].is_object());
        assert_eq!(s["function"]["parameters"]["required"][0], "q");
    }

    #[test]
    fn anthropic_structure() {
        let t = ToolDefinition::new("test");
        let s = anthropic_schema(&t);
        assert!(s["input_schema"]["properties"].is_object());
    }
}
