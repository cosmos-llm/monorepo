//! TrongoScript helpers for the IRs in this crate (feature `trongo`).
//!
//! TrongoScript values are `serde_json::Value`, so an IR loads from a `.trongo`
//! source by evaluating it and deserializing the result, and dumps as data
//! literals. The IR types themselves know nothing about this module.

use serde::{de::DeserializeOwned, Serialize};

/// Evaluates TrongoScript `src` and deserializes the resulting value.
///
/// # Errors
///
/// Returns a message when evaluation fails or the value does not match `T`.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{trongo::from_trongo, OutputLimit};
///
/// let l: OutputLimit = from_trongo("{:max_bytes 100}").unwrap();
/// assert_eq!(l.max_bytes, 100);
/// ```
pub fn from_trongo<T: DeserializeOwned>(src: &str) -> Result<T, String> {
    let mut ctx = trongoscript::Context::new(trongoscript::EvalConfig::new());
    let v = trongoscript::eval_source(src, &mut ctx).map_err(|e| e.to_string())?;
    serde_json::from_value(v).map_err(|e| e.to_string())
}

/// Dumps an IR as inert TrongoScript data literals.
///
/// # Errors
///
/// Returns a message when `ir` cannot be serialized.
pub fn to_trongo<T: Serialize>(ir: &T) -> Result<String, String> {
    let v = serde_json::to_value(ir).map_err(|e| e.to_string())?;
    Ok(trongoscript::to_trongo_source_pretty(&v))
}
