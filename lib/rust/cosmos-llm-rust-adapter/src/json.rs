use serde_json::{json, Map, Value};

use cosmos_llm_registry::meta;
use cosmos_llm_signature::Signature;

use crate::adapter::{input_values, output_values, Adapter, AdapterPlugin};
use crate::error::AdapterError;

/// The schema name sent with native structured output when none is set.
pub const DEFAULT_SCHEMA_NAME: &str = "signature_output";

/// Asks for the outputs as a single JSON object.
///
/// In its plain form the format is requested in the prompt only, which works
/// with every provider. Built with [`JsonAdapter::native`], it also sends the
/// output schema as an OpenAI-style `response_format`, so a provider that
/// supports structured output enforces the shape itself.
///
/// Parsing is forgiving about where the object sits: the whole reply, a
/// fenced code block, or the span from the first `{` to the last `}` are
/// tried in that order, since models routinely wrap JSON in prose or fences
/// despite being told not to.
///
/// # Examples
///
/// ```
/// use cosmos_llm_adapter::{Adapter, JsonAdapter};
/// use cosmos_llm_signature::Signature;
/// use serde_json::json;
///
/// let sig = Signature::parse("text -> tags: array[string]").unwrap();
/// let reply = "Here you go:\n```json\n{\"tags\": [\"rust\"]}\n```";
/// let out = JsonAdapter::new().parse(&sig, reply).unwrap();
/// assert_eq!(out["tags"], json!(["rust"]));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonAdapter {
    native_structured_output: bool,
    schema_name: String,
}

impl Default for JsonAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonAdapter {
    /// A JSON adapter that requests the format in the prompt only.
    pub fn new() -> Self {
        Self {
            native_structured_output: false,
            schema_name: DEFAULT_SCHEMA_NAME.to_owned(),
        }
    }

    /// A JSON adapter that also sends the schema as `response_format`.
    pub fn native() -> Self {
        Self {
            native_structured_output: true,
            ..Self::new()
        }
    }

    /// Sets the schema name sent with native structured output.
    #[must_use]
    pub fn with_schema_name(mut self, name: impl Into<String>) -> Self {
        self.schema_name = name.into();
        self
    }

    /// Whether the schema is sent as `response_format`.
    pub fn is_native(&self) -> bool {
        self.native_structured_output
    }

    /// The schema name sent with native structured output.
    pub fn schema_name(&self) -> &str {
        &self.schema_name
    }

    /// Finds the first JSON object in a reply, trying the whole text, a
    /// fenced block, and the outermost brace span in turn.
    pub fn find_object(completion: &str) -> Option<Map<String, Value>> {
        let candidates = [
            Some(completion),
            fenced_block(completion),
            brace_span(completion),
        ];
        candidates.into_iter().flatten().find_map(|candidate| {
            match serde_json::from_str(candidate.trim()) {
                Ok(Value::Object(object)) => Some(object),
                _ => None,
            }
        })
    }
}

/// The body of the first ```` ``` ```` or ```` ```json ```` fenced block.
fn fenced_block(text: &str) -> Option<&str> {
    let start = text.find("```")?;
    let rest = &text[start + 3..];
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    let newline = rest.find('\n')?;
    if !rest[..newline].trim().is_empty() {
        return None;
    }
    let body = &rest[newline + 1..];
    let end = body.find("```")?;
    Some(&body[..end])
}

fn brace_span(text: &str) -> Option<&str> {
    let first = text.find('{')?;
    let last = text.rfind('}')?;
    (last > first).then(|| &text[first..=last])
}

impl Adapter for JsonAdapter {
    fn request_params(&self, sig: &Signature) -> Result<Map<String, Value>, AdapterError> {
        let mut params = Map::new();
        if self.native_structured_output {
            params.insert(
                "response_format".to_owned(),
                json!({
                    "type": "json_schema",
                    "json_schema": {
                        "name": self.schema_name,
                        "strict": true,
                        "schema": sig.output_json_schema()?,
                    }
                }),
            );
        }
        Ok(params)
    }

    fn format_instructions(&self, sig: &Signature) -> Result<String, AdapterError> {
        let keys = sig
            .output_fields()
            .map(|field| Ok(format!("- \"{}\": {}", field.name, field.type_hint()?)))
            .collect::<Result<Vec<_>, AdapterError>>()?;
        Ok([
            "Respond with a single JSON object and nothing else. It must have exactly these keys:",
            "",
            &keys.join("\n"),
            "",
            "Do not wrap the object in prose. Do not add keys that are not listed.",
        ]
        .join("\n"))
    }

    fn render_inputs(
        &self,
        sig: &Signature,
        inputs: &Map<String, Value>,
    ) -> Result<String, AdapterError> {
        let present = Value::Object(input_values(sig, inputs));
        Ok(serde_json::to_string_pretty(&present).unwrap_or_else(|_| present.to_string()))
    }

    fn render_demo_outputs(
        &self,
        sig: &Signature,
        demo: &Map<String, Value>,
    ) -> Result<String, AdapterError> {
        Ok(Value::Object(output_values(sig, demo)).to_string())
    }

    fn extract(
        &self,
        sig: &Signature,
        completion: &str,
    ) -> Result<Map<String, Value>, AdapterError> {
        let object = Self::find_object(completion).ok_or_else(|| {
            AdapterError::parse(
                "Expected a JSON object in the response but could not find one.",
                completion,
            )
        })?;
        Ok(output_values(sig, &object))
    }
}

fn build_json() -> Box<dyn Adapter> {
    Box::new(JsonAdapter::new())
}

fn build_json_schema() -> Box<dyn Adapter> {
    Box::new(JsonAdapter::native())
}

cosmos_llm_registry::inventory::submit! {
    AdapterPlugin {
        meta: meta!("json").builtin().summary("A single JSON object, requested in the prompt"),
        build: build_json,
    }
}

cosmos_llm_registry::inventory::submit! {
    AdapterPlugin {
        meta: meta!("json_schema")
            .builtin()
            .summary("A JSON object, also enforced via response_format"),
        build: build_json_schema,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig() -> Signature {
        Signature::parse("question -> answer, confident: bool").unwrap()
    }

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn instructions_list_each_key_with_its_hint() {
        let text = JsonAdapter::new().format_instructions(&sig()).unwrap();
        assert!(
            text.contains("- \"answer\": a string\n- \"confident\": true or false"),
            "{text}"
        );
    }

    #[test]
    fn inputs_render_as_pretty_json_of_declared_fields() {
        let text = JsonAdapter::new()
            .render_inputs(&sig(), &map(json!({ "question": "Why?", "noise": 1 })))
            .unwrap();
        assert_eq!(text, "{\n  \"question\": \"Why?\"\n}");
    }

    #[test]
    fn the_object_is_found_bare_fenced_or_in_prose() {
        let adapter = JsonAdapter::new();
        for reply in [
            r#"{"answer": "4", "confident": "yes"}"#,
            "```json\n{\"answer\": \"4\", \"confident\": true}\n```",
            "```\n{\"answer\": \"4\", \"confident\": true}\n```",
            "I think {\"answer\": \"4\", \"confident\": true} is right.",
        ] {
            let out = adapter.parse(&sig(), reply).unwrap();
            assert_eq!(
                Value::Object(out),
                json!({ "answer": "4", "confident": true }),
                "{reply}"
            );
        }
    }

    #[test]
    fn undeclared_keys_are_dropped() {
        let out = JsonAdapter::new()
            .extract(
                &sig(),
                r#"{"answer": "4", "why": "math", "confident": true}"#,
            )
            .unwrap();
        assert_eq!(out.keys().collect::<Vec<_>>(), ["answer", "confident"]);
    }

    #[test]
    fn a_reply_without_an_object_is_a_parse_error() {
        for reply in ["no json here", "[1, 2]", "} backwards {"] {
            let err = JsonAdapter::new().parse(&sig(), reply).unwrap_err();
            assert!(
                err.to_string().starts_with("Expected a JSON object"),
                "{reply}: {err}"
            );
        }
    }

    #[test]
    fn only_the_native_form_sends_response_format() {
        assert!(JsonAdapter::new()
            .request_params(&sig())
            .unwrap()
            .is_empty());
        let params = JsonAdapter::native()
            .with_schema_name("qa")
            .request_params(&sig())
            .unwrap();
        let format = &params["response_format"];
        assert_eq!(format["type"], "json_schema");
        assert_eq!(format["json_schema"]["name"], "qa");
        assert_eq!(format["json_schema"]["strict"], true);
        assert_eq!(
            format["json_schema"]["schema"],
            sig().output_json_schema().unwrap()
        );
    }

    #[test]
    fn demo_outputs_are_compact_json_that_parses_back() {
        let demo = map(json!({ "question": "q", "answer": "a", "confident": false }));
        let text = JsonAdapter::new()
            .render_demo_outputs(&sig(), &demo)
            .unwrap();
        assert_eq!(text, r#"{"answer":"a","confident":false}"#);
        assert_eq!(
            Value::Object(JsonAdapter::new().parse(&sig(), &text).unwrap()),
            json!({ "answer": "a", "confident": false })
        );
    }

    #[test]
    fn json_schema_is_registered_as_the_native_form() {
        let params = crate::adapter("json_schema")
            .unwrap()
            .request_params(&sig())
            .unwrap();
        assert!(params.contains_key("response_format"));
    }
}
