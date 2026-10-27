use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::CacheError;

/// The longest key the built-in stores accept.
pub const MAX_KEY_LEN: usize = 200;

/// Builds a stable cache key from a request description.
///
/// The request is serialized canonically — object keys sorted at every
/// depth, no whitespace — and hashed with SHA-256. Two requests that differ
/// only in key order produce the same key, in any process and regardless of
/// whether `serde_json`'s `preserve_order` feature is enabled somewhere in
/// the dependency graph. Only data contributes, never identity.
///
/// Numbers are compared as written: `1` and `1.0` are different requests,
/// as they are to a provider.
///
/// # Examples
///
/// ```
/// use cosmos_llm_cache::key_for;
/// use serde_json::json;
///
/// let a = key_for(&json!({ "model": "m", "messages": [], "temperature": 0.0 }));
/// let b = key_for(&json!({ "temperature": 0.0, "messages": [], "model": "m" }));
/// assert_eq!(a, b);
/// assert_eq!(a.len(), 64);
/// ```
pub fn key_for(request: &Value) -> String {
    let mut canonical = String::new();
    write_canonical(request, &mut canonical);
    let digest = Sha256::digest(canonical.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Writes `value` as JSON with object keys sorted recursively.
fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(object) => {
            let mut entries: Vec<(&String, &Value)> = object.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (index, (key, inner)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(inner, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, inner) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(inner, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// Checks that a key can safely name a file.
///
/// # Errors
///
/// Returns [`CacheError::InvalidKey`] for an empty or over-long key, or one
/// holding anything other than ASCII letters, digits, `-`, and `_`. That
/// rules out path separators and `..`, so a key can never address a file
/// outside the cache directory.
///
/// # Examples
///
/// ```
/// use cosmos_llm_cache::validate_key;
///
/// assert!(validate_key("3f2a9c").is_ok());
/// assert!(validate_key("../etc/passwd").is_err());
/// ```
pub fn validate_key(key: &str) -> Result<(), CacheError> {
    let valid = !key.is_empty()
        && key.len() <= MAX_KEY_LEN
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if valid {
        Ok(())
    } else {
        Err(CacheError::InvalidKey(key.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_key_order_does_not_change_the_key() {
        let a = json!({ "outer": { "b": 1, "a": [{ "y": 2, "x": 1 }] } });
        let b = json!({ "outer": { "a": [{ "x": 1, "y": 2 }], "b": 1 } });
        assert_eq!(key_for(&a), key_for(&b));
    }

    #[test]
    fn array_order_and_values_do_change_the_key() {
        assert_ne!(key_for(&json!([1, 2])), key_for(&json!([2, 1])));
        assert_ne!(key_for(&json!({ "t": 1 })), key_for(&json!({ "t": 1.0 })));
    }

    #[test]
    fn canonical_form_escapes_keys_and_strings() {
        let mut out = String::new();
        write_canonical(&json!({ "q\"": "line\nbreak", "a": null }), &mut out);
        assert_eq!(out, r#"{"a":null,"q\"":"line\nbreak"}"#);
    }

    #[test]
    fn the_digest_is_sha256_of_the_canonical_form() {
        // sha256("{}")
        assert_eq!(
            key_for(&json!({})),
            "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
    }

    #[test]
    fn keys_that_could_escape_the_directory_are_rejected() {
        for bad in ["", "a/b", "..", "a.json", "white space", &"x".repeat(201)] {
            assert!(validate_key(bad).is_err(), "{bad:?}");
        }
        assert!(validate_key("abc-DEF_123").is_ok());
    }
}
