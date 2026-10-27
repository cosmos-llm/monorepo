use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::codec::{self, FieldCodec};
use crate::error::SignatureError;

/// Whether a field is supplied to the model or produced by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A value the caller supplies.
    Input,
    /// A value the model produces.
    Output,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Input => "input",
            Self::Output => "output",
        })
    }
}

/// A single declared field on a signature, either an input or an output.
///
/// A field carries a name, a type, an optional description, and optional
/// constraints. The type is the *name* of a registered [`FieldCodec`], and
/// that codec drives three things: the JSON schema emitted for structured
/// output, the type hint rendered into the prompt, and the coercion applied to
/// whatever the model returns. Registering a codec under a new name adds a
/// type; registering one under a built-in name replaces it.
///
/// Fields are built with a chain of methods and validated when the
/// [`Signature`](crate::Signature) holding them is built, which is where an
/// unknown type name is reported.
///
/// # Examples
///
/// ```
/// use cosmos_llm_signature::Field;
///
/// let tags = Field::output("tags")
///     .ty("array")
///     .of("string")
///     .desc("lowercase keywords");
/// assert_eq!(tags.name, "tags");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// The field name, used as the key in input and output maps.
    pub name: String,
    /// Whether this is an input or an output.
    pub kind: Kind,
    /// The type name, resolved against the codec registry.
    #[serde(rename = "type", default = "default_type")]
    pub ty: String,
    /// The element type for array fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<String>,
    /// A description shown to the model.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub desc: String,
    /// The allowed values, if constrained.
    #[serde(default, rename = "enum", skip_serializing_if = "Option::is_none")]
    pub one_of: Option<Vec<Value>>,
    /// Whether the field must be present.
    #[serde(default = "default_required")]
    pub required: bool,
    /// The value used when the field is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

fn default_type() -> String {
    "string".to_owned()
}

fn default_required() -> bool {
    true
}

impl Field {
    /// Starts a required string field of the given kind.
    pub fn new(name: impl Into<String>, kind: Kind) -> Self {
        Self {
            name: name.into(),
            kind,
            ty: default_type(),
            of: None,
            desc: String::new(),
            one_of: None,
            required: true,
            default: None,
        }
    }

    /// Starts a required string input field.
    pub fn input(name: impl Into<String>) -> Self {
        Self::new(name, Kind::Input)
    }

    /// Starts a required string output field.
    pub fn output(name: impl Into<String>) -> Self {
        Self::new(name, Kind::Output)
    }

    /// Sets the type name, e.g. `"integer"` or an alias such as `"int"`.
    pub fn ty(mut self, ty: impl Into<String>) -> Self {
        self.ty = ty.into();
        self
    }

    /// Sets the element type of an array field.
    pub fn of(mut self, element: impl Into<String>) -> Self {
        self.of = Some(element.into());
        self
    }

    /// Sets the description shown to the model.
    pub fn desc(mut self, desc: impl Into<String>) -> Self {
        self.desc = desc.into();
        self
    }

    /// Constrains the field to a fixed set of values.
    pub fn one_of<I, V>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        self.one_of = Some(values.into_iter().map(Into::into).collect());
        self
    }

    /// Marks the field as optional: absent values are omitted, not an error.
    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    /// Sets the value used when the field is absent.
    pub fn default_value(mut self, value: impl Into<Value>) -> Self {
        self.default = Some(value.into());
        self
    }

    /// Whether this is an input field.
    pub fn is_input(&self) -> bool {
        self.kind == Kind::Input
    }

    /// Whether this is an output field.
    pub fn is_output(&self) -> bool {
        self.kind == Kind::Output
    }

    /// The codec for this field's type.
    ///
    /// # Errors
    ///
    /// Returns [`SignatureError::UnknownType`] when no codec is registered
    /// under the type name.
    pub fn codec(&self) -> Result<&'static dyn FieldCodec, SignatureError> {
        codec::lookup(&self.name, &self.ty)
    }

    /// A field describing one element of this array field.
    ///
    /// Element fields carry the parent's name, so a coercion error inside an
    /// array names the field the caller actually declared.
    pub fn element(&self) -> Option<Field> {
        self.of
            .as_ref()
            .map(|of| Field::new(self.name.clone(), self.kind).ty(of.clone()))
    }

    /// Renders the type as a short hint for inclusion in a prompt.
    ///
    /// # Errors
    ///
    /// Returns [`SignatureError::UnknownType`] for an unregistered type.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm_signature::Field;
    ///
    /// let field = Field::output("tags").ty("array").of("string");
    /// assert_eq!(field.type_hint().unwrap(), "a JSON array of strings");
    ///
    /// let field = Field::output("tone").one_of(["formal", "casual"]);
    /// assert_eq!(field.type_hint().unwrap(), "a string, one of: formal, casual");
    /// ```
    pub fn type_hint(&self) -> Result<String, SignatureError> {
        let hint = self.codec()?.type_hint(self)?;
        Ok(match &self.one_of {
            Some(values) => {
                let listed: Vec<String> = values.iter().map(display_value).collect();
                format!("{hint}, one of: {}", listed.join(", "))
            }
            None => hint,
        })
    }

    /// Builds the JSON schema fragment describing this field.
    ///
    /// # Errors
    ///
    /// Returns [`SignatureError::UnknownType`] for an unregistered type.
    pub fn json_schema(&self) -> Result<Value, SignatureError> {
        let mut schema = self.codec()?.json_schema(self)?;
        if let Some(object) = schema.as_object_mut() {
            if !self.desc.is_empty() {
                object.insert("description".to_owned(), json!(self.desc));
            }
            if let Some(values) = &self.one_of {
                object.insert("enum".to_owned(), Value::Array(values.clone()));
            }
        }
        Ok(schema)
    }

    /// Coerces a raw value into this field's type.
    ///
    /// Strings are parsed where the target type needs it, since a model
    /// writing free text will hand back `"42"` for an integer field. A value
    /// already of the right type passes through. After coercion the value is
    /// checked against [`Field::one_of`], if set.
    ///
    /// # Errors
    ///
    /// Returns [`SignatureError::Coercion`] when the value cannot be
    /// converted or is outside the allowed set.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm_signature::Field;
    /// use serde_json::json;
    ///
    /// let count = Field::output("count").ty("int");
    /// assert_eq!(count.coerce(json!(" 42 ")).unwrap(), json!(42));
    /// assert!(count.coerce(json!("banana")).is_err());
    /// ```
    pub fn coerce(&self, value: Value) -> Result<Value, SignatureError> {
        let coerced = self.codec()?.coerce(self, value)?;
        self.check_allowed(&coerced)?;
        Ok(coerced)
    }

    fn check_allowed(&self, value: &Value) -> Result<(), SignatureError> {
        let Some(allowed) = &self.one_of else {
            return Ok(());
        };
        // A model writes enum values as text, so a string matches an allowed
        // value whose display form is the same: "3" satisfies an enum of [3].
        let found = allowed.iter().any(|candidate| {
            candidate == value
                || value
                    .as_str()
                    .is_some_and(|text| display_value(candidate) == text)
        });
        if found {
            return Ok(());
        }
        let listed: Vec<String> = allowed.iter().map(display_value).collect();
        Err(SignatureError::coercion(
            &self.name,
            format!("expected one of: {}, got {}", listed.join(", "), value),
        ))
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.of {
            Some(of) => write!(f, "{}: {}[{}]", self.name, self.ty, of),
            None => write!(f, "{}: {}", self.name, self.ty),
        }
    }
}

/// Renders a JSON value the way it would read in a prompt: strings bare,
/// everything else as JSON.
pub(crate) fn display_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
