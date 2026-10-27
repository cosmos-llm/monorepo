use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use cosmos_llm_registry::{Meta, Plugin};

use crate::codec;
use crate::error::SignatureError;
use crate::field::{Field, Kind};
use crate::parser;

/// A declarative description of one LLM call: named typed inputs, named typed
/// outputs, and the instructions that connect them.
///
/// A signature says *what* a call does, not *how* to prompt for it. Rendering
/// the fields into messages and parsing the reply back into typed values is
/// the job of an adapter (see the `cosmos-llm-adapter` crate), so the same
/// signature works across providers and across prompting strategies.
///
/// Signatures are immutable values. Methods that change one, such as
/// [`Signature::with_instructions`], return a new signature, which is what an
/// optimizer trying instruction variants against fixed fields needs.
///
/// # Examples
///
/// The shorthand string form:
///
/// ```
/// use cosmos_llm_signature::Signature;
///
/// let sig = Signature::parse("question -> answer: int").unwrap();
/// assert_eq!(sig.to_string(), "question -> answer");
/// ```
///
/// The builder form, for descriptions and constraints:
///
/// ```
/// use cosmos_llm_signature::{Field, Signature};
///
/// let sig = Signature::builder()
///     .instructions("Summarize the document for a technical reader.")
///     .field(Field::input("document").desc("raw text to summarize"))
///     .field(Field::output("summary").desc("three sentences, no preamble"))
///     .field(Field::output("confidence").ty("number"))
///     .build()
///     .unwrap();
///
/// assert_eq!(sig.output_fields().count(), 2);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SignatureSpec", into = "SignatureSpec")]
pub struct Signature {
    instructions: Option<String>,
    fields: Vec<Field>,
}

/// The serialized shape of a [`Signature`], validated on the way in.
#[derive(Serialize, Deserialize)]
struct SignatureSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    fields: Vec<Field>,
}

impl TryFrom<SignatureSpec> for Signature {
    type Error = SignatureError;

    fn try_from(spec: SignatureSpec) -> Result<Self, Self::Error> {
        let mut builder = Signature::builder();
        builder.instructions = spec.instructions;
        builder.fields = spec.fields;
        builder.build()
    }
}

impl From<Signature> for SignatureSpec {
    fn from(signature: Signature) -> Self {
        Self {
            instructions: signature.instructions,
            fields: signature.fields,
        }
    }
}

/// Assembles a [`Signature`] field by field.
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct SignatureBuilder {
    instructions: Option<String>,
    fields: Vec<Field>,
}

impl SignatureBuilder {
    /// Sets the instructions.
    pub fn instructions(mut self, text: impl Into<String>) -> Self {
        self.instructions = Some(text.into());
        self
    }

    /// Appends a field. Fields keep declaration order, which is the order an
    /// adapter renders them in.
    pub fn field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    /// Appends an input field of the given type.
    pub fn input(self, name: impl Into<String>, ty: impl Into<String>) -> Self {
        self.field(Field::input(name).ty(ty))
    }

    /// Appends an output field of the given type.
    pub fn output(self, name: impl Into<String>, ty: impl Into<String>) -> Self {
        self.field(Field::output(name).ty(ty))
    }

    /// Validates the fields and builds the signature.
    ///
    /// Type names are resolved against the codec registry here and replaced
    /// with the canonical name, so `int` and `integer` produce identical
    /// signatures and identical cache keys.
    ///
    /// # Errors
    ///
    /// Returns [`SignatureError::Definition`] for an empty or duplicate field
    /// name or a signature with no outputs, and
    /// [`SignatureError::UnknownType`] for an unregistered type.
    pub fn build(mut self) -> Result<Signature, SignatureError> {
        for (index, field) in self.fields.iter().enumerate() {
            if field.name.trim().is_empty() {
                return Err(SignatureError::Definition(
                    "field names cannot be empty".to_owned(),
                ));
            }
            if self.fields[..index]
                .iter()
                .any(|earlier| earlier.name == field.name)
            {
                return Err(SignatureError::Definition(format!(
                    "field '{}' is declared more than once",
                    field.name
                )));
            }
        }
        if !self.fields.iter().any(Field::is_output) {
            return Err(SignatureError::Definition(
                "a signature needs at least one output field".to_owned(),
            ));
        }

        for field in &mut self.fields {
            field.ty = codec::resolve(&field.name, &field.ty)?.meta.name.to_owned();
            if let Some(of) = &field.of {
                field.of = Some(codec::resolve(&field.name, of)?.meta.name.to_owned());
            }
            // Resolve once now so a bad element type fails at build time, not
            // at the first coercion.
            field.type_hint()?;
        }

        Ok(Signature {
            instructions: self.instructions.filter(|text| !text.trim().is_empty()),
            fields: self.fields,
        })
    }
}

impl Signature {
    /// Starts a builder.
    pub fn builder() -> SignatureBuilder {
        SignatureBuilder::default()
    }

    /// Builds a signature from the shorthand string form.
    ///
    /// # Errors
    ///
    /// Returns an error when the string is malformed or names an unknown
    /// type. See [`parser`] for the grammar.
    pub fn parse(text: &str) -> Result<Self, SignatureError> {
        let (inputs, outputs) = parser::parse(text)?;
        SignatureBuilder {
            instructions: None,
            fields: inputs.into_iter().chain(outputs).collect(),
        }
        .build()
    }

    /// Builds a signature from a spec, whichever form it is in.
    ///
    /// This is what an API or CLI boundary wants, since a caller may have any
    /// of the three forms in hand:
    ///
    /// - a shorthand string, recognized by its `->`;
    /// - `@path/to/file.json`, a signature serialized with serde;
    /// - otherwise, the name of a signature registered with
    ///   [`SignaturePlugin`].
    ///
    /// # Errors
    ///
    /// Returns an error when the spec does not parse, the file cannot be read
    /// or decoded, or no signature is registered under the name.
    pub fn from_spec(spec: &str) -> Result<Self, SignatureError> {
        let spec = spec.trim();
        if spec.contains(parser::ARROW) {
            return Self::parse(spec);
        }
        if let Some(path) = spec.strip_prefix('@') {
            let text = std::fs::read_to_string(path)
                .map_err(|e| SignatureError::Definition(format!("cannot read {path}: {e}")))?;
            return Self::from_json(&text);
        }
        let plugin = cosmos_llm_registry::require::<SignaturePlugin>("signature", spec)?;
        (plugin.build)()
    }

    /// Decodes a signature serialized as JSON, validating it.
    ///
    /// # Errors
    ///
    /// Returns [`SignatureError::Definition`] for malformed JSON, and any
    /// validation error [`SignatureBuilder::build`] would raise.
    pub fn from_json(text: &str) -> Result<Self, SignatureError> {
        let spec: SignatureSpec = serde_json::from_str(text)
            .map_err(|e| SignatureError::Definition(format!("invalid signature JSON: {e}")))?;
        Self::try_from(spec)
    }

    /// A builder holding this signature's fields and instructions, for
    /// deriving a variant.
    pub fn to_builder(&self) -> SignatureBuilder {
        SignatureBuilder {
            instructions: self.instructions.clone(),
            fields: self.fields.clone(),
        }
    }

    /// The instructions, or a default generated from the field names.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm_signature::Signature;
    ///
    /// let sig = Signature::parse("question -> answer").unwrap();
    /// assert_eq!(sig.instructions(), "Given the fields `question`, produce the fields `answer`.");
    /// ```
    pub fn instructions(&self) -> String {
        match &self.instructions {
            Some(text) => text.clone(),
            None => {
                let quote = |fields: Vec<&Field>| {
                    fields
                        .iter()
                        .map(|field| format!("`{}`", field.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                format!(
                    "Given the fields {}, produce the fields {}.",
                    quote(self.input_fields().collect()),
                    quote(self.output_fields().collect())
                )
            }
        }
    }

    /// Whether instructions were set explicitly rather than generated.
    pub fn has_instructions(&self) -> bool {
        self.instructions.is_some()
    }

    /// A copy of this signature with different instructions.
    pub fn with_instructions(&self, text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            instructions: Some(text).filter(|text| !text.trim().is_empty()),
            fields: self.fields.clone(),
        }
    }

    /// A copy of this signature with `field` placed before every other output.
    ///
    /// Adapters render outputs in order, so a field placed first is written
    /// first. That is the whole mechanism behind chain-of-thought: a
    /// `reasoning` output ahead of the answer.
    ///
    /// # Errors
    ///
    /// Returns [`SignatureError::Definition`] when `field` is not an output
    /// or its name is already taken.
    pub fn with_leading_output(&self, field: Field) -> Result<Self, SignatureError> {
        if !field.is_output() {
            return Err(SignatureError::Definition(format!(
                "with_leading_output needs an output field, got input '{}'",
                field.name
            )));
        }
        let at = self
            .fields
            .iter()
            .position(Field::is_output)
            .unwrap_or(self.fields.len());
        let mut fields = self.fields.clone();
        fields.insert(at, field);
        SignatureBuilder {
            instructions: self.instructions.clone(),
            fields,
        }
        .build()
    }

    /// All fields, in declaration order.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The input fields, in declaration order.
    pub fn input_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(|field| field.is_input())
    }

    /// The output fields, in declaration order.
    pub fn output_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(|field| field.is_output())
    }

    /// The field with the given name, if declared.
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }

    /// A JSON schema describing the output fields.
    ///
    /// This is what a provider with native structured output receives, and
    /// what a JSON adapter can validate against.
    ///
    /// # Errors
    ///
    /// Returns [`SignatureError::UnknownType`] if a codec was unregistered
    /// after the signature was built, which only happens when a signature is
    /// deserialized in a binary that links a different set of codecs.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm_signature::Signature;
    /// use serde_json::json;
    ///
    /// let sig = Signature::parse("q -> a: int").unwrap();
    /// assert_eq!(
    ///     sig.output_json_schema().unwrap(),
    ///     json!({
    ///         "type": "object",
    ///         "properties": { "a": { "type": "integer" } },
    ///         "required": ["a"],
    ///         "additionalProperties": false
    ///     })
    /// );
    /// ```
    pub fn output_json_schema(&self) -> Result<Value, SignatureError> {
        let mut properties = Map::new();
        let mut required = Vec::new();
        for field in self.output_fields() {
            properties.insert(field.name.clone(), field.json_schema()?);
            if field.required {
                required.push(json!(field.name));
            }
        }
        Ok(json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        }))
    }

    /// Coerces raw input values into their declared types.
    ///
    /// # Errors
    ///
    /// As for [`Signature::coerce_outputs`].
    pub fn coerce_inputs(&self, values: &Value) -> Result<Map<String, Value>, SignatureError> {
        self.coerce(Kind::Input, values)
    }

    /// Coerces raw output values — usually parsed from a model reply — into
    /// their declared types.
    ///
    /// The result holds exactly the declared fields of that kind, in
    /// declaration order. Undeclared keys are dropped. A missing or `null`
    /// field takes its default if it has one, is omitted if optional, and is
    /// an error otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`SignatureError::Definition`] if `values` is not a JSON
    /// object, [`SignatureError::MissingField`] for an absent required field,
    /// and [`SignatureError::Coercion`] for a value of the wrong type.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm_signature::Signature;
    /// use serde_json::json;
    ///
    /// let sig = Signature::parse("document -> summary, confidence: number").unwrap();
    /// let out = sig
    ///     .coerce_outputs(&json!({ "summary": "Short.", "confidence": "0.9", "extra": 1 }))
    ///     .unwrap();
    /// assert_eq!(serde_json::Value::Object(out), json!({ "summary": "Short.", "confidence": 0.9 }));
    /// ```
    pub fn coerce_outputs(&self, values: &Value) -> Result<Map<String, Value>, SignatureError> {
        self.coerce(Kind::Output, values)
    }

    fn coerce(&self, kind: Kind, values: &Value) -> Result<Map<String, Value>, SignatureError> {
        let Value::Object(values) = values else {
            return Err(SignatureError::Definition(format!(
                "expected a JSON object of {kind} values, got {values}"
            )));
        };
        let mut result = Map::new();
        for field in self.fields.iter().filter(|field| field.kind == kind) {
            match values.get(&field.name).filter(|value| !value.is_null()) {
                Some(value) => {
                    result.insert(field.name.clone(), field.coerce(value.clone())?);
                }
                None => {
                    if let Some(default) = &field.default {
                        result.insert(field.name.clone(), default.clone());
                    } else if field.required {
                        return Err(SignatureError::MissingField {
                            kind,
                            field: field.name.clone(),
                            have: values.keys().cloned().collect(),
                        });
                    }
                }
            }
        }
        Ok(result)
    }

    /// A stable description of this signature, for cache keys.
    ///
    /// Two signatures with the same fields and effective instructions produce
    /// the same string in any process, which is what lets a disk cache
    /// survive restarts.
    pub fn cache_key(&self) -> String {
        json!({ "instructions": self.instructions(), "fields": self.fields }).to_string()
    }
}

impl fmt::Display for Signature {
    /// Renders the field names as `inputs -> outputs`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = |fields: Vec<&Field>| {
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        write!(
            f,
            "{} -> {}",
            names(self.input_fields().collect()),
            names(self.output_fields().collect())
        )
    }
}

/// A named signature registration.
///
/// Registering a signature lets every CLI and config file refer to it by
/// name through [`Signature::from_spec`], and lets an application replace a
/// library's signature — with better instructions, say — by registering the
/// same name.
///
/// ```
/// use cosmos_llm_signature::{Signature, SignatureError, SignaturePlugin};
/// use cosmos_llm_signature::registry::meta;
///
/// fn triage() -> Result<Signature, SignatureError> {
///     Signature::parse("ticket -> severity: int, team")
///         .map(|sig| sig.with_instructions("Route the support ticket."))
/// }
///
/// cosmos_llm_signature::registry::inventory::submit! {
///     SignaturePlugin { meta: meta!("triage").summary("Route a support ticket"), build: triage }
/// }
///
/// let sig = Signature::from_spec("triage").unwrap();
/// assert_eq!(sig.instructions(), "Route the support ticket.");
/// ```
pub struct SignaturePlugin {
    /// The signature's name and aliases.
    pub meta: Meta,
    /// Builds the signature.
    pub build: fn() -> Result<Signature, SignatureError>,
}

impl Plugin for SignaturePlugin {
    fn meta(&self) -> &Meta {
        &self.meta
    }
}

cosmos_llm_registry::inventory::collect!(SignaturePlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_names_are_rejected_across_kinds() {
        let err = Signature::builder()
            .input("question", "string")
            .output("question", "string")
            .build()
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid signature: field 'question' is declared more than once"
        );
    }

    #[test]
    fn a_signature_needs_an_output() {
        let err = Signature::builder()
            .input("q", "string")
            .build()
            .unwrap_err();
        assert!(err.to_string().contains("at least one output"));
    }

    #[test]
    fn type_aliases_are_canonicalized_at_build() {
        let a = Signature::parse("q -> n: int, tags: list[str]").unwrap();
        let b = Signature::parse("q -> n: integer, tags: array[string]").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.cache_key(), b.cache_key());
    }

    #[test]
    fn unknown_element_types_fail_at_build() {
        let err = Signature::parse("q -> xs: array[nope]").unwrap_err();
        assert!(matches!(err, SignatureError::UnknownType { .. }), "{err}");
    }

    #[test]
    fn missing_required_fields_name_what_was_present() {
        let sig = Signature::parse("q -> answer").unwrap();
        let err = sig.coerce_outputs(&json!({ "anser": "x" })).unwrap_err();
        assert_eq!(
            err.to_string(),
            "missing required output field 'answer' (have: anser)"
        );
    }

    #[test]
    fn null_is_treated_as_absent() {
        let sig = Signature::builder()
            .field(Field::input("q"))
            .field(Field::output("a"))
            .field(Field::output("note").optional())
            .field(Field::output("score").ty("number").default_value(0.5))
            .build()
            .unwrap();
        let out = sig
            .coerce_outputs(&json!({ "a": "x", "note": null, "score": null }))
            .unwrap();
        assert_eq!(Value::Object(out), json!({ "a": "x", "score": 0.5 }));
        assert!(sig.coerce_outputs(&json!({ "a": null })).is_err());
    }

    #[test]
    fn coercion_requires_an_object() {
        let sig = Signature::parse("q -> a").unwrap();
        assert!(sig
            .coerce_inputs(&json!(["q"]))
            .unwrap_err()
            .to_string()
            .contains("JSON object"));
    }

    #[test]
    fn outputs_come_back_in_declaration_order() {
        let sig = Signature::parse("q -> b, a").unwrap();
        let out = sig.coerce_outputs(&json!({ "a": "1", "b": "2" })).unwrap();
        assert_eq!(out.keys().collect::<Vec<_>>(), ["b", "a"]);
    }

    #[test]
    fn leading_outputs_go_before_the_first_output() {
        let sig = Signature::parse("q -> answer").unwrap();
        let cot = sig.with_leading_output(Field::output("reasoning")).unwrap();
        let names: Vec<&str> = cot.fields().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["q", "reasoning", "answer"]);
        assert!(cot.with_leading_output(Field::output("answer")).is_err());
        assert!(sig.with_leading_output(Field::input("ctx")).is_err());
    }

    #[test]
    fn instructions_are_replaced_without_touching_the_original() {
        let sig = Signature::parse("q -> a").unwrap();
        let terse = sig.with_instructions("Be terse.");
        assert_eq!(terse.instructions(), "Be terse.");
        assert!(!sig.has_instructions());
        assert_ne!(sig.cache_key(), terse.cache_key());
        assert!(!sig.with_instructions("  ").has_instructions());
    }

    #[test]
    fn json_round_trip_validates_on_the_way_in() {
        let sig = Signature::parse("q -> tags: array[string]")
            .unwrap()
            .with_instructions("Tag it.");
        let text = serde_json::to_string(&sig).unwrap();
        assert_eq!(Signature::from_json(&text).unwrap(), sig);

        let bad = r#"{"fields":[{"name":"a","kind":"output","type":"mystery"}]}"#;
        assert!(Signature::from_json(bad).is_err());
    }

    #[test]
    fn specs_resolve_shorthand_and_report_unknown_names() {
        assert!(Signature::from_spec("q -> a").is_ok());
        let err = Signature::from_spec("no_such_signature").unwrap_err();
        assert!(err
            .to_string()
            .starts_with("unknown signature 'no_such_signature'"));
    }
}
