//! TrongoScript helpers for the IR types (005).
//!
//! [`to_trongo`] prints an IR as inert data literals (the readable dump and
//! golden-file format); [`from_trongo`] evaluates source with no filesystem
//! and deserializes the result. The IR types never mention the evaluator.

use serde::{de::DeserializeOwned, Serialize};

use crate::error::{CoreError, Result};
use crate::eval::{ConfigEvaluator, EvalSnapshot};

/// Evaluates `source` and deserializes the result into `T`.
pub fn from_trongo<T: DeserializeOwned>(source: &str) -> Result<T> {
    let v = ConfigEvaluator::new(EvalSnapshot::empty()).eval_source(source, None)?;
    serde_json::from_value(v)
        .map_err(|e| CoreError::Config(format!("value does not match the IR: {e}")))
}

/// Prints `ir` as TrongoScript data literals.
pub fn to_trongo<T: Serialize>(ir: &T) -> Result<String> {
    let value = serde_json::to_value(ir)?;
    Ok(trongoscript::to_trongo_source_pretty(&value))
}
