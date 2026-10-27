use thiserror::Error;

use crate::field::Kind;

/// Errors produced while declaring a signature or coercing values through it.
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum SignatureError {
    /// The signature itself is malformed: a duplicate field, a bad shorthand
    /// string, no outputs.
    #[error("invalid signature: {0}")]
    Definition(String),

    /// A field names a type that no registered codec handles.
    #[error("unknown field type '{ty}' on field '{field}'; available: {}", available.join(", "))]
    UnknownType {
        /// The field that declared the type.
        field: String,
        /// The type name as written.
        ty: String,
        /// The type names that are registered.
        available: Vec<&'static str>,
    },

    /// A value could not be converted to its field's declared type.
    #[error("field '{field}' {message}")]
    Coercion {
        /// The field being coerced.
        field: String,
        /// What was expected and what arrived, e.g. `expected an integer, got "banana"`.
        message: String,
    },

    /// A required field had no value and no default.
    #[error("missing required {kind} field '{field}' (have: {})", have.join(", "))]
    MissingField {
        /// Whether the field is an input or an output.
        kind: Kind,
        /// The missing field.
        field: String,
        /// The keys that were present, to make a typo obvious.
        have: Vec<String>,
    },

    /// A named signature was not found in the registry.
    #[error(transparent)]
    NotFound(#[from] cosmos_llm_registry::NotFound),
}

impl SignatureError {
    /// Builds a [`SignatureError::Coercion`] for `field`.
    pub fn coercion(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Coercion {
            field: field.into(),
            message: message.into(),
        }
    }
}
