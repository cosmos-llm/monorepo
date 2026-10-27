use serde_json::{Map, Value};

use cosmos_llm_registry::meta;
use cosmos_llm_signature::Signature;

use crate::adapter::{output_values, render_value, Adapter, AdapterPlugin};
use crate::error::AdapterError;

/// The section name that ends a block of fields.
pub const TERMINATOR: &str = "done";

/// Lays each field out under its own `[[ ## name ## ]]` header.
///
/// Free text inside each section is left alone, which is why this is the
/// default: a model can write a multi-paragraph answer with code blocks and
/// quotes without any escaping, and the headers are unlikely to occur in it
/// by accident.
///
/// # Examples
///
/// ```
/// use cosmos_llm_adapter::{Adapter, ChatAdapter};
/// use cosmos_llm_signature::Signature;
/// use serde_json::json;
///
/// let sig = Signature::parse("question -> answer: int").unwrap();
/// let reply = "[[ ## answer ## ]]\n4\n\n[[ ## done ## ]]";
/// let out = ChatAdapter.parse(&sig, reply).unwrap();
/// assert_eq!(out["answer"], json!(4));
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct ChatAdapter;

impl ChatAdapter {
    /// The header line for a section.
    pub fn marker(name: &str) -> String {
        format!("[[ ## {name} ## ]]")
    }

    /// Splits text into `(name, body)` sections, in order of appearance.
    ///
    /// Text before the first header is ignored, a `done` header closes the
    /// current section without opening another, and a repeated name keeps
    /// its last body.
    pub fn split_sections(completion: &str) -> Vec<(String, String)> {
        let mut sections: Vec<(String, String)> = Vec::new();
        let mut current: Option<String> = None;
        let mut buffer = String::new();
        let mut close = |current: &mut Option<String>, buffer: &mut String| {
            if let Some(name) = current.take() {
                let body = buffer.trim().to_owned();
                sections.retain(|(existing, _)| *existing != name);
                sections.push((name, body));
            }
            buffer.clear();
        };
        for line in completion.split_inclusive('\n') {
            match header_name(line) {
                Some(name) => {
                    close(&mut current, &mut buffer);
                    current = (name != TERMINATOR).then(|| name.to_owned());
                }
                None if current.is_some() => buffer.push_str(line),
                None => {}
            }
        }
        close(&mut current, &mut buffer);
        sections
    }
}

/// Reads `[[ ## name ## ]]`, with any spacing, into `name`.
fn header_name(line: &str) -> Option<&str> {
    let inner = line.trim().strip_prefix("[[")?.strip_suffix("]]")?.trim();
    let name = inner.strip_prefix("##")?.strip_suffix("##")?.trim();
    let is_word = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    is_word.then_some(name)
}

impl Adapter for ChatAdapter {
    fn format_instructions(&self, sig: &Signature) -> Result<String, AdapterError> {
        let mut lines = vec![
            "Respond with each output field in its own section, in this exact structure:"
                .to_owned(),
            String::new(),
        ];
        for field in sig.output_fields() {
            lines.push(Self::marker(&field.name));
            lines.push(format!("<{}, {}>", field.name, field.type_hint()?));
            lines.push(String::new());
        }
        lines.push(Self::marker(TERMINATOR));
        lines.push(String::new());
        lines.push(
            "Emit every field header exactly as shown, including the brackets and hashes. \
             Put nothing before the first header."
                .to_owned(),
        );
        Ok(lines.join("\n"))
    }

    fn render_inputs(
        &self,
        sig: &Signature,
        inputs: &Map<String, Value>,
    ) -> Result<String, AdapterError> {
        let mut sections: Vec<String> = sig
            .input_fields()
            .filter_map(|field| {
                inputs
                    .get(&field.name)
                    .map(|value| format!("{}\n{}", Self::marker(&field.name), render_value(value)))
            })
            .collect();
        sections.push(Self::marker(TERMINATOR));
        Ok(sections.join("\n\n"))
    }

    fn render_demo_outputs(
        &self,
        sig: &Signature,
        demo: &Map<String, Value>,
    ) -> Result<String, AdapterError> {
        let mut sections: Vec<String> = output_values(sig, demo)
            .iter()
            .map(|(name, value)| format!("{}\n{}", Self::marker(name), render_value(value)))
            .collect();
        sections.push(Self::marker(TERMINATOR));
        Ok(sections.join("\n\n"))
    }

    fn extract(
        &self,
        sig: &Signature,
        completion: &str,
    ) -> Result<Map<String, Value>, AdapterError> {
        let sections = Self::split_sections(completion);
        if sections.is_empty() {
            let first = sig
                .output_fields()
                .next()
                .map_or("field", |field| field.name.as_str());
            return Err(AdapterError::parse(
                format!(
                    "Expected field headers like {} but found none in the response.",
                    Self::marker(first)
                ),
                completion,
            ));
        }
        Ok(sections
            .into_iter()
            .filter(|(name, _)| sig.output_fields().any(|field| field.name == *name))
            .map(|(name, body)| (name, Value::String(body)))
            .collect())
    }
}

fn build() -> Box<dyn Adapter> {
    Box::new(ChatAdapter)
}

cosmos_llm_registry::inventory::submit! {
    AdapterPlugin {
        meta: meta!("chat").builtin().summary("Each field under a [[ ## name ## ]] header"),
        build,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sig() -> Signature {
        Signature::parse("question, context -> answer, score: number").unwrap()
    }

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn format_instructions_show_every_output_header() {
        let text = ChatAdapter.format_instructions(&sig()).unwrap();
        assert!(
            text.contains("[[ ## answer ## ]]\n<answer, a string>"),
            "{text}"
        );
        assert!(
            text.contains("[[ ## score ## ]]\n<score, a number>"),
            "{text}"
        );
        assert!(text.contains("[[ ## done ## ]]"), "{text}");
    }

    #[test]
    fn inputs_render_in_field_order_and_skip_absent_fields() {
        let text = ChatAdapter
            .render_inputs(
                &sig(),
                &map(json!({ "context": ["a", "b"], "question": "Why?" })),
            )
            .unwrap();
        assert_eq!(
            text,
            "[[ ## question ## ]]\nWhy?\n\n[[ ## context ## ]]\n[\"a\",\"b\"]\n\n[[ ## done ## ]]"
        );
        let text = ChatAdapter
            .render_inputs(&sig(), &map(json!({ "question": "Why?" })))
            .unwrap();
        assert!(!text.contains("context"));
    }

    #[test]
    fn format_puts_demos_between_the_system_prompt_and_the_inputs() {
        let demo = map(
            json!({ "question": "1+1?", "context": "", "answer": "2", "score": 1, "noise": true }),
        );
        let messages = ChatAdapter
            .format(
                &sig(),
                &map(json!({ "question": "2+2?", "context": "" })),
                &[demo],
            )
            .unwrap();
        let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user"]);
        assert!(messages[0]
            .content
            .starts_with("Given the fields `question`, `context`"));
        assert!(messages[0]
            .content
            .contains("Your output fields are:\n- answer (a string)"));
        assert_eq!(
            messages[2].content,
            "[[ ## answer ## ]]\n2\n\n[[ ## score ## ]]\n1\n\n[[ ## done ## ]]"
        );
        assert!(!messages[1].content.contains("noise"));
    }

    #[test]
    fn parse_reads_sections_and_coerces_them() {
        let reply = "Sure!\n[[ ## answer ## ]]\nFour,\nas in 4.\n\n[[## score##]]\n 0.9 \n[[ ## extra ## ]]\nx\n[[ ## done ## ]]\ntrailing";
        let out = ChatAdapter.parse(&sig(), reply).unwrap();
        assert_eq!(
            Value::Object(out),
            json!({ "answer": "Four,\nas in 4.", "score": 0.9 })
        );
    }

    #[test]
    fn a_reply_without_headers_is_a_parse_error_carrying_the_reply() {
        let err = ChatAdapter.parse(&sig(), "The answer is 4.").unwrap_err();
        assert_eq!(
            err,
            AdapterError::parse(
                "Expected field headers like [[ ## answer ## ]] but found none in the response.",
                "The answer is 4."
            )
        );
    }

    #[test]
    fn coercion_failures_become_parse_errors() {
        let reply = "[[ ## answer ## ]]\nx\n[[ ## score ## ]]\nhigh";
        let err = ChatAdapter.parse(&sig(), reply).unwrap_err();
        assert!(matches!(&err, AdapterError::Parse { completion, .. } if completion == reply));
        assert_eq!(
            err.to_string(),
            "field 'score' expected a number, got \"high\""
        );
    }

    #[test]
    fn repeated_sections_keep_the_last_body() {
        let sections = ChatAdapter::split_sections("[[ ## a ## ]]\n1\n[[ ## a ## ]]\n2\n");
        assert_eq!(sections, vec![("a".to_owned(), "2".to_owned())]);
    }

    #[test]
    fn demo_outputs_round_trip_through_parse() {
        let demo = map(json!({ "answer": "line one\nline two", "score": 0.5 }));
        let text = ChatAdapter.render_demo_outputs(&sig(), &demo).unwrap();
        assert_eq!(ChatAdapter.parse(&sig(), &text).unwrap(), demo);
    }
}
