//! Field types, as swappable codecs.
//!
//! A field's type is the name of a [`FieldCodec`] registered with
//! [`CodecPlugin`]. The six JSON types are registered as built-ins; a crate
//! can add a type by registering a codec under a new name, or change how a
//! built-in type behaves by registering under its name.
//!
//! ```
//! use cosmos_llm_signature::codec::{CodecPlugin, FieldCodec};
//! use cosmos_llm_signature::{Field, Signature, SignatureError};
//! use cosmos_llm_signature::registry::meta;
//! use serde_json::{json, Value};
//!
//! /// An ISO-8601 calendar date, kept as a string.
//! struct DateCodec;
//!
//! impl FieldCodec for DateCodec {
//!     fn type_hint(&self, _field: &Field) -> Result<String, SignatureError> {
//!         Ok("a date as YYYY-MM-DD".to_owned())
//!     }
//!
//!     fn json_schema(&self, _field: &Field) -> Result<Value, SignatureError> {
//!         Ok(json!({ "type": "string", "format": "date" }))
//!     }
//!
//!     fn coerce(&self, field: &Field, value: Value) -> Result<Value, SignatureError> {
//!         let text = value.as_str().map(str::trim).unwrap_or_default();
//!         let shaped = text.len() == 10 && text.as_bytes()[4] == b'-' && text.as_bytes()[7] == b'-';
//!         if shaped {
//!             Ok(json!(text))
//!         } else {
//!             Err(SignatureError::coercion(&field.name, format!("expected YYYY-MM-DD, got {value}")))
//!         }
//!     }
//! }
//!
//! cosmos_llm_signature::registry::inventory::submit! {
//!     CodecPlugin { meta: meta!("date").summary("ISO-8601 date"), codec: &DateCodec }
//! }
//!
//! let sig = Signature::parse("invoice -> due: date").unwrap();
//! let out = sig.coerce_outputs(&json!({ "due": " 2026-11-01 " })).unwrap();
//! assert_eq!(out["due"], "2026-11-01");
//! ```

use serde_json::{json, Map, Value};

use cosmos_llm_registry::{meta, Meta, Plugin};

use crate::error::SignatureError;
use crate::field::Field;

/// How one field type is described to a model and coerced back.
///
/// Implementations must be stateless or internally synchronized: a single
/// `&'static` instance is shared by every field of that type.
pub trait FieldCodec: Send + Sync {
    /// A short phrase for the prompt, e.g. `"an integer"`.
    ///
    /// # Errors
    ///
    /// Returns an error when the hint depends on another type that cannot be
    /// resolved, such as an array's element type.
    fn type_hint(&self, field: &Field) -> Result<String, SignatureError>;

    /// The plural noun used when this type is an array element, e.g.
    /// `"integers"` in "a JSON array of integers".
    fn plural(&self) -> String {
        "values".to_owned()
    }

    /// The JSON schema fragment for the type, without `description` or
    /// `enum`, which [`Field::json_schema`] adds.
    ///
    /// # Errors
    ///
    /// As for [`FieldCodec::type_hint`].
    fn json_schema(&self, field: &Field) -> Result<Value, SignatureError>;

    /// Converts a raw value into this type.
    ///
    /// # Errors
    ///
    /// Returns [`SignatureError::Coercion`] when the value cannot be
    /// converted.
    fn coerce(&self, field: &Field, value: Value) -> Result<Value, SignatureError>;
}

/// A field type registration.
pub struct CodecPlugin {
    /// The type name and its aliases.
    pub meta: Meta,
    /// The codec handling the type.
    pub codec: &'static dyn FieldCodec,
}

impl Plugin for CodecPlugin {
    fn meta(&self) -> &Meta {
        &self.meta
    }
}

cosmos_llm_registry::inventory::collect!(CodecPlugin);

/// Resolves a type name to its winning registration.
///
/// # Errors
///
/// Returns [`SignatureError::UnknownType`] naming the registered types.
pub fn resolve(field: &str, ty: &str) -> Result<&'static CodecPlugin, SignatureError> {
    cosmos_llm_registry::resolve::<CodecPlugin>(ty).ok_or_else(|| SignatureError::UnknownType {
        field: field.to_owned(),
        ty: ty.to_owned(),
        available: cosmos_llm_registry::names::<CodecPlugin>(),
    })
}

pub(crate) fn lookup(field: &str, ty: &str) -> Result<&'static dyn FieldCodec, SignatureError> {
    resolve(field, ty).map(|plugin| plugin.codec)
}

/// Text a model may write for `true`, compared after trimming and lowercasing.
pub const TRUTHY: &[&str] = &["true", "yes", "y", "1"];
/// Text a model may write for `false`, compared after trimming and lowercasing.
pub const FALSEY: &[&str] = &["false", "no", "n", "0"];

/// The built-in `string` type.
pub struct StringCodec;
/// The built-in `integer` type.
pub struct IntegerCodec;
/// The built-in `number` type.
pub struct NumberCodec;
/// The built-in `boolean` type.
pub struct BooleanCodec;
/// The built-in `array` type, with an optional element type.
pub struct ArrayCodec;
/// The built-in `object` type.
pub struct ObjectCodec;

impl FieldCodec for StringCodec {
    fn type_hint(&self, _field: &Field) -> Result<String, SignatureError> {
        Ok("a string".to_owned())
    }

    fn plural(&self) -> String {
        "strings".to_owned()
    }

    fn json_schema(&self, _field: &Field) -> Result<Value, SignatureError> {
        Ok(json!({ "type": "string" }))
    }

    fn coerce(&self, _field: &Field, value: Value) -> Result<Value, SignatureError> {
        Ok(match value {
            Value::String(_) => value,
            // Structured values are rendered as JSON rather than lost: a model
            // that answers a string field with an object usually meant it.
            Value::Array(_) | Value::Object(_) => Value::String(value.to_string()),
            Value::Null => Value::String(String::new()),
            other => Value::String(other.to_string()),
        })
    }
}

impl FieldCodec for IntegerCodec {
    fn type_hint(&self, _field: &Field) -> Result<String, SignatureError> {
        Ok("an integer".to_owned())
    }

    fn plural(&self) -> String {
        "integers".to_owned()
    }

    fn json_schema(&self, _field: &Field) -> Result<Value, SignatureError> {
        Ok(json!({ "type": "integer" }))
    }

    fn coerce(&self, field: &Field, value: Value) -> Result<Value, SignatureError> {
        let fail =
            || SignatureError::coercion(&field.name, format!("expected an integer, got {value}"));
        match &value {
            Value::Number(n) if n.is_i64() || n.is_u64() => Ok(value.clone()),
            // A float is accepted only when it is integral. Truncating 3.7 to
            // 3 would hand the caller a number the model never wrote.
            Value::Number(n) => match n.as_f64() {
                Some(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15 => {
                    Ok(json!(f as i64))
                }
                _ => Err(fail()),
            },
            Value::String(text) => {
                let text = text.trim();
                text.parse::<i64>()
                    .map(|n| json!(n))
                    .or_else(|_| text.parse::<u64>().map(|n| json!(n)))
                    .map_err(|_| fail())
            }
            _ => Err(fail()),
        }
    }
}

impl FieldCodec for NumberCodec {
    fn type_hint(&self, _field: &Field) -> Result<String, SignatureError> {
        Ok("a number".to_owned())
    }

    fn plural(&self) -> String {
        "numbers".to_owned()
    }

    fn json_schema(&self, _field: &Field) -> Result<Value, SignatureError> {
        Ok(json!({ "type": "number" }))
    }

    fn coerce(&self, field: &Field, value: Value) -> Result<Value, SignatureError> {
        let parsed = match &value {
            Value::Number(n) => n.as_f64(),
            Value::String(text) => text.trim().parse::<f64>().ok(),
            _ => None,
        };
        // NaN and infinity parse from text but have no JSON representation.
        match parsed.filter(|f| f.is_finite()) {
            Some(f) => Ok(json!(f)),
            None => Err(SignatureError::coercion(
                &field.name,
                format!("expected a number, got {value}"),
            )),
        }
    }
}

impl FieldCodec for BooleanCodec {
    fn type_hint(&self, _field: &Field) -> Result<String, SignatureError> {
        Ok("true or false".to_owned())
    }

    fn plural(&self) -> String {
        "booleans".to_owned()
    }

    fn json_schema(&self, _field: &Field) -> Result<Value, SignatureError> {
        Ok(json!({ "type": "boolean" }))
    }

    fn coerce(&self, field: &Field, value: Value) -> Result<Value, SignatureError> {
        let text = match &value {
            Value::Bool(_) => return Ok(value),
            Value::String(text) => text.trim().to_lowercase(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        if TRUTHY.contains(&text.as_str()) {
            Ok(Value::Bool(true))
        } else if FALSEY.contains(&text.as_str()) {
            Ok(Value::Bool(false))
        } else {
            Err(SignatureError::coercion(
                &field.name,
                format!("expected a boolean, got {value}"),
            ))
        }
    }
}

impl FieldCodec for ArrayCodec {
    fn type_hint(&self, field: &Field) -> Result<String, SignatureError> {
        match field.element() {
            Some(element) => Ok(format!("a JSON array of {}", element.codec()?.plural())),
            None => Ok("a JSON array".to_owned()),
        }
    }

    fn plural(&self) -> String {
        "arrays".to_owned()
    }

    fn json_schema(&self, field: &Field) -> Result<Value, SignatureError> {
        let items = match field.element() {
            Some(element) => element.json_schema()?,
            None => json!({}),
        };
        Ok(json!({ "type": "array", "items": items }))
    }

    fn coerce(&self, field: &Field, value: Value) -> Result<Value, SignatureError> {
        let items = match value {
            Value::Array(items) => items,
            Value::String(text) => match parse_json(field, &text)? {
                Value::Array(items) => items,
                other => {
                    return Err(SignatureError::coercion(
                        &field.name,
                        format!("expected an array, got {other}"),
                    ))
                }
            },
            other => {
                return Err(SignatureError::coercion(
                    &field.name,
                    format!("expected an array, got {other}"),
                ))
            }
        };
        let Some(element) = field.element() else {
            return Ok(Value::Array(items));
        };
        items
            .into_iter()
            .map(|item| element.coerce(item))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array)
    }
}

impl FieldCodec for ObjectCodec {
    fn type_hint(&self, _field: &Field) -> Result<String, SignatureError> {
        Ok("a JSON object".to_owned())
    }

    fn plural(&self) -> String {
        "objects".to_owned()
    }

    fn json_schema(&self, _field: &Field) -> Result<Value, SignatureError> {
        Ok(json!({ "type": "object" }))
    }

    fn coerce(&self, field: &Field, value: Value) -> Result<Value, SignatureError> {
        let object: Map<String, Value> = match value {
            Value::Object(object) => object,
            Value::String(text) => match parse_json(field, &text)? {
                Value::Object(object) => object,
                other => {
                    return Err(SignatureError::coercion(
                        &field.name,
                        format!("expected an object, got {other}"),
                    ))
                }
            },
            other => {
                return Err(SignatureError::coercion(
                    &field.name,
                    format!("expected an object, got {other}"),
                ))
            }
        };
        Ok(Value::Object(object))
    }
}

fn parse_json(field: &Field, text: &str) -> Result<Value, SignatureError> {
    serde_json::from_str(text.trim()).map_err(|e| {
        SignatureError::coercion(&field.name, format!("expected JSON, got {text:?} ({e})"))
    })
}

cosmos_llm_registry::inventory::submit! {
    CodecPlugin {
        meta: meta!("string").builtin().aliases(&["str", "text"]).summary("Free text"),
        codec: &StringCodec,
    }
}

cosmos_llm_registry::inventory::submit! {
    CodecPlugin {
        meta: meta!("integer").builtin().aliases(&["int"]).summary("Whole numbers; integral floats accepted"),
        codec: &IntegerCodec,
    }
}

cosmos_llm_registry::inventory::submit! {
    CodecPlugin {
        meta: meta!("number").builtin().aliases(&["float", "f64"]).summary("Finite JSON numbers"),
        codec: &NumberCodec,
    }
}

cosmos_llm_registry::inventory::submit! {
    CodecPlugin {
        meta: meta!("boolean").builtin().aliases(&["bool"]).summary("true/false, yes/no, y/n, 1/0"),
        codec: &BooleanCodec,
    }
}

cosmos_llm_registry::inventory::submit! {
    CodecPlugin {
        meta: meta!("array").builtin().aliases(&["list"]).summary("JSON arrays, elements typed by `of`"),
        codec: &ArrayCodec,
    }
}

cosmos_llm_registry::inventory::submit! {
    CodecPlugin {
        meta: meta!("object").builtin().aliases(&["hash", "map"]).summary("JSON objects"),
        codec: &ObjectCodec,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(ty: &str) -> Field {
        Field::output("value").ty(ty)
    }

    #[test]
    fn aliases_resolve_to_builtins() {
        for (alias, name) in [
            ("int", "integer"),
            ("bool", "boolean"),
            ("list", "array"),
            ("hash", "object"),
        ] {
            assert_eq!(resolve("f", alias).unwrap().meta.name, name);
        }
    }

    #[test]
    fn unknown_types_list_the_registered_ones() {
        let err = resolve("when", "timestamp").err().unwrap();
        assert!(err
            .to_string()
            .contains("unknown field type 'timestamp' on field 'when'"));
        assert!(err.to_string().contains("integer"));
    }

    #[test]
    fn string_renders_structured_values_as_json() {
        assert_eq!(
            field("string").coerce(json!({"a": 1})).unwrap(),
            json!("{\"a\":1}")
        );
        assert_eq!(field("string").coerce(json!(3)).unwrap(), json!("3"));
    }

    #[test]
    fn integer_accepts_integral_floats_and_rejects_fractions() {
        assert_eq!(field("integer").coerce(json!(4.0)).unwrap(), json!(4));
        assert!(field("integer").coerce(json!(3.7)).is_err());
        assert!(field("integer").coerce(json!(true)).is_err());
    }

    #[test]
    fn number_parses_text_and_rejects_non_finite() {
        assert_eq!(
            field("number").coerce(json!(" 0.25 ")).unwrap(),
            json!(0.25)
        );
        assert!(field("number").coerce(json!("NaN")).is_err());
        assert!(field("number").coerce(json!("inf")).is_err());
    }

    #[test]
    fn boolean_understands_the_words_models_use() {
        for (raw, expected) in [("YES", true), (" n ", false), ("1", true), ("false", false)] {
            assert_eq!(
                field("boolean").coerce(json!(raw)).unwrap(),
                json!(expected)
            );
        }
        assert_eq!(field("boolean").coerce(json!(0)).unwrap(), json!(false));
        assert!(field("boolean").coerce(json!("maybe")).is_err());
    }

    #[test]
    fn array_parses_json_text_and_coerces_elements() {
        let ints = field("array").of("integer");
        assert_eq!(ints.coerce(json!("[\"1\", 2]")).unwrap(), json!([1, 2]));
        let err = ints.coerce(json!(["1", "x"])).unwrap_err();
        assert_eq!(
            err.to_string(),
            "field 'value' expected an integer, got \"x\""
        );
        assert!(ints.coerce(json!("{}")).is_err());
    }

    #[test]
    fn array_schema_and_hint_describe_the_element_type() {
        let tags = field("array").of("string");
        assert_eq!(tags.type_hint().unwrap(), "a JSON array of strings");
        assert_eq!(
            tags.json_schema().unwrap(),
            json!({ "type": "array", "items": { "type": "string" } })
        );
        assert_eq!(
            field("array").json_schema().unwrap(),
            json!({ "type": "array", "items": {} })
        );
    }

    #[test]
    fn object_parses_json_text() {
        assert_eq!(
            field("object").coerce(json!("{\"a\": 1}")).unwrap(),
            json!({"a": 1})
        );
        assert!(field("object").coerce(json!("[1]")).is_err());
        assert!(field("object").coerce(json!("not json")).is_err());
    }

    #[test]
    fn enums_match_on_display_form() {
        let level = field("integer").one_of([1, 2, 3]);
        assert_eq!(level.coerce(json!("2")).unwrap(), json!(2));
        let err = level.coerce(json!(9)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "field 'value' expected one of: 1, 2, 3, got 9"
        );
    }
}
