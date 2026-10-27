//! Replies holding several JSON objects.
//!
//! Some calls ask for one object per item — one per extracted record, one
//! per tool invocation — and models answer with objects back to back,
//! newline-separated, comma-separated, or wrapped in a fence. [`split`] finds
//! each top-level object by tracking brace depth outside strings, so it does
//! not care what separates them; [`parse_all`] decodes them and reports the
//! ones that fail without losing the rest.
//!
//! ```
//! use cosmos_llm_adapter::json_stream;
//!
//! let reply = "```jsonl\n{\"n\": 1}\n{\"n\": 2}, {\"n\": \"}\"}\n```";
//! let (objects, errors) = json_stream::parse_all(reply);
//! assert_eq!(objects.len(), 3);
//! assert!(errors.is_empty());
//! ```

use serde_json::{Map, Value};

/// Returns the source text of each top-level `{...}` object, in order.
///
/// Braces inside strings are ignored, and a stray `}` at depth zero — the
/// tail of a truncated reply, say — is skipped. An object left open at the
/// end of the text is not returned.
pub fn split(text: &str) -> Vec<&str> {
    let mut objects = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    let mut in_string = false;
    let mut escaped = false;

    for (i, c) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    if let Some(from) = start.take() {
                        objects.push(&text[from..=i]);
                    }
                }
            }
            _ => {}
        }
    }
    objects
}

/// Decodes every top-level object in `text`.
///
/// Returns the objects that decoded and one message per object that did
/// not, so a caller can keep the good records and report the bad ones.
pub fn parse_all(text: &str) -> (Vec<Map<String, Value>>, Vec<String>) {
    let mut objects = Vec::new();
    let mut errors = Vec::new();
    for (i, source) in split(strip_fences(text)).into_iter().enumerate() {
        match serde_json::from_str::<Value>(source) {
            Ok(Value::Object(object)) => objects.push(object),
            Ok(_) => errors.push(format!("object {i} was not an object")),
            Err(e) => errors.push(format!("object {i} did not parse: {e}")),
        }
    }
    (objects, errors)
}

/// Strips surrounding whitespace and one Markdown code fence.
///
/// The opening fence may be tagged `json`, `jsonl`, or `ndjson`, in any
/// case.
///
/// # Examples
///
/// ```
/// use cosmos_llm_adapter::json_stream::strip_fences;
///
/// assert_eq!(strip_fences("  ```NDJSON\n{}\n```  "), "{}");
/// assert_eq!(strip_fences("{}"), "{}");
/// ```
pub fn strip_fences(text: &str) -> &str {
    let mut text = text.trim();
    if let Some(rest) = text.strip_prefix("```") {
        let tag = ["ndjson", "jsonl", "json"]
            .into_iter()
            .find(|tag| {
                rest.get(..tag.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(tag))
            })
            .map_or(0, str::len);
        let rest = rest[tag..].trim_start_matches([' ', '\t']);
        let rest = rest.strip_prefix('\r').unwrap_or(rest);
        text = rest.strip_prefix('\n').unwrap_or(rest);
    }
    if let Some(rest) = text.strip_suffix("```") {
        let rest = rest.strip_suffix('\n').unwrap_or(rest);
        text = rest.strip_suffix('\r').unwrap_or(rest);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_ignores_braces_inside_strings_and_escapes() {
        let text = r#"{"a": "{not}"} {"b": "say \"}\""} {"c": {"d": 1}}"#;
        assert_eq!(
            split(text),
            [
                r#"{"a": "{not}"}"#,
                r#"{"b": "say \"}\""}"#,
                r#"{"c": {"d": 1}}"#
            ]
        );
    }

    #[test]
    fn split_skips_stray_closers_and_drops_an_unclosed_tail() {
        assert_eq!(split(r#"} {"a": 1} {"b": "#), [r#"{"a": 1}"#]);
    }

    #[test]
    fn split_handles_multibyte_text() {
        assert_eq!(split("é {\"q\": \"ü\"} ñ"), ["{\"q\": \"ü\"}"]);
    }

    #[test]
    fn parse_all_keeps_good_objects_and_reports_bad_ones() {
        let (objects, errors) = parse_all("{\"a\": 1}\n{\"b\": }\n{\"c\": 3}");
        assert_eq!(objects.len(), 2);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].starts_with("object 1 did not parse"),
            "{}",
            errors[0]
        );
    }

    #[test]
    fn strip_fences_handles_crlf_and_bare_fences() {
        assert_eq!(strip_fences("```json\r\n{}\r\n```"), "{}");
        assert_eq!(strip_fences("```\n{}\n```"), "{}");
        assert_eq!(strip_fences("```jsonl {}```"), "{}");
    }
}
