//! TrongoScript helpers for the IR types.
//!
//! TrongoScript values are `serde_json::Value`s, so an IR loads from a
//! `.trongo` file by evaluating it and deserializing the result, and dumps by
//! serializing to a value and printing it as data literals. The dump is inert:
//! evaluating it yields the value it came from. Enabled by the `trongo`
//! feature, on by default.
//!
//! ```
//! use cosmos_llm::{from_trongo, to_trongo, Message};
//!
//! let msg = Message::user("hi");
//! let text = to_trongo(&msg).unwrap();
//! let back: Message = from_trongo(&text).unwrap();
//! assert_eq!(msg, back);
//! ```

use serde::de::DeserializeOwned;
use serde::Serialize;
use trongoscript::{eval_source, Context, EvalConfig};

use crate::error::CosmosError;

/// Evaluates TrongoScript source and deserializes the result into `T`.
///
/// Evaluation has no filesystem, network or process access: no sandbox is
/// mounted, so `include` and `read-file` fail. Config that needs them is
/// evaluated by the host (which owns the sandbox) and handed over as a value.
///
/// # Errors
///
/// [`CosmosError::Configuration`] if evaluation fails or the result does not
/// match `T`'s shape.
pub fn from_trongo<T: DeserializeOwned>(source: &str) -> Result<T, CosmosError> {
    let mut ctx = Context::new(EvalConfig::default());
    let value = eval_source(source, &mut ctx)
        .map_err(|e| CosmosError::config(format!("trongo evaluation failed: {e}")))?;
    serde_json::from_value(value)
        .map_err(|e| CosmosError::config(format!("trongo value does not match the type: {e}")))
}

/// Prints `value` as TrongoScript data literals.
///
/// # Errors
///
/// [`CosmosError::Json`] if `value` cannot be serialized.
pub fn to_trongo<T: Serialize>(value: &T) -> Result<String, CosmosError> {
    let json = serde_json::to_value(value)?;
    Ok(trongoscript::to_trongo_source_pretty(&json))
}
