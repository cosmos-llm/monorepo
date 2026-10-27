//! TrongoScript helpers for the IRs in this crate (feature `trongo`).
//!
//! TrongoScript values are `serde_json::Value`, so a [`PromptPlan`] loads from
//! a `.trongo` source by evaluating it and deserializing the result, and dumps
//! as inert data literals. The IR types know nothing about this module.

use serde::{de::DeserializeOwned, Serialize};

use crate::environment::EnvironmentFacts;
use crate::instructions::Instructions;
use crate::plan::PromptPlan;

/// Evaluates TrongoScript `src` and deserializes the value into any IR.
///
/// # Errors
///
/// Returns a message when evaluation fails or the value does not match `T`.
pub fn from_trongo<T: DeserializeOwned>(src: &str) -> Result<T, String> {
    let mut ctx = trongoscript::Context::new(trongoscript::EvalConfig::new());
    let v = trongoscript::eval_source(src, &mut ctx).map_err(|e| e.to_string())?;
    serde_json::from_value(v).map_err(|e| e.to_string())
}

/// Dumps any IR as TrongoScript data literals.
///
/// # Errors
///
/// Returns a message when `ir` cannot be serialized.
pub fn to_trongo<T: Serialize>(ir: &T) -> Result<String, String> {
    let v = serde_json::to_value(ir).map_err(|e| e.to_string())?;
    Ok(trongoscript::to_trongo_source_pretty(&v))
}

impl PromptPlan {
    /// Loads a plan from TrongoScript source.
    ///
    /// # Errors
    ///
    /// Returns a message on evaluation or shape errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_context::PromptPlan;
    ///
    /// let plan = PromptPlan::from_trongo(
    ///     r#"{:sections [{:kind "base" :content [{:type "text" :text "Hi."}] :stability "static"}]}"#,
    /// ).unwrap();
    /// assert_eq!(plan.render(), "Hi.");
    /// assert_eq!(PromptPlan::from_trongo(&plan.to_trongo().unwrap()).unwrap(), plan);
    /// ```
    pub fn from_trongo(src: &str) -> Result<Self, String> {
        from_trongo(src)
    }

    /// Dumps the plan as TrongoScript data literals.
    ///
    /// # Errors
    ///
    /// Returns a message when serialization fails.
    pub fn to_trongo(&self) -> Result<String, String> {
        to_trongo(self)
    }
}

impl Instructions {
    /// Loads instructions from TrongoScript source.
    ///
    /// # Errors
    ///
    /// Returns a message on evaluation or shape errors.
    pub fn from_trongo(src: &str) -> Result<Self, String> {
        from_trongo(src)
    }

    /// Dumps instructions as TrongoScript data literals.
    ///
    /// # Errors
    ///
    /// Returns a message when serialization fails.
    pub fn to_trongo(&self) -> Result<String, String> {
        to_trongo(self)
    }
}

impl EnvironmentFacts {
    /// Loads facts from TrongoScript source.
    ///
    /// # Errors
    ///
    /// Returns a message on evaluation or shape errors.
    pub fn from_trongo(src: &str) -> Result<Self, String> {
        from_trongo(src)
    }

    /// Dumps facts as TrongoScript data literals.
    ///
    /// # Errors
    ///
    /// Returns a message when serialization fails.
    pub fn to_trongo(&self) -> Result<String, String> {
        to_trongo(self)
    }
}
