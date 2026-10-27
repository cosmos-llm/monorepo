//! TrongoScript helpers for the IR types.
//!
//! [`to_trongo`] prints an IR as inert data literals; [`from_trongo`]
//! evaluates TrongoScript source (a data literal or a program that computes
//! one) in a default sandbox with no filesystem access, and deserializes the
//! result. The IR types themselves know nothing about TrongoScript.

use serde::{de::DeserializeOwned, Serialize};
use trongoscript::{eval_source, Context, EvalConfig};

/// Why [`from_trongo`] failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TrongoError {
    /// The source did not evaluate.
    #[error("trongo evaluation failed: {0}")]
    Eval(String),
    /// The value did not match the IR's shape.
    #[error("value does not match the IR: {0}")]
    Shape(#[from] serde_json::Error),
}

/// Evaluates `source` and deserializes the result into `T`.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_coding_tools::{trongo::from_trongo, EditSpec};
///
/// let e: EditSpec = from_trongo(r#"{:old_string "a" :new_string "b"}"#).unwrap();
/// assert_eq!(e.new_string, "b");
/// ```
pub fn from_trongo<T: DeserializeOwned>(source: &str) -> Result<T, TrongoError> {
    let mut ctx = Context::new(EvalConfig::default());
    let value = eval_source(source, &mut ctx).map_err(|e| TrongoError::Eval(e.to_string()))?;
    Ok(serde_json::from_value(value)?)
}

/// Prints `ir` as TrongoScript data literals.
pub fn to_trongo<T: Serialize>(ir: &T) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(ir)?;
    Ok(trongoscript::to_trongo_source_pretty(&value))
}
