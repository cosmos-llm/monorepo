//! Prompting strategies, selectable by name.
//!
//! A strategy turns a signature and its collaborators into a [`Module`].
//! `predict` and `chain_of_thought` are built in; registering under a new
//! name adds a strategy to the CLI and to any config-driven caller, and
//! registering under a built-in name replaces it.
//!
//! Best-of-N is not registered, because it needs a reward function and a
//! name cannot supply one.

use std::sync::Arc;

use cosmos_llm_adapter::Adapter;
use cosmos_llm_cache::CacheStore;
use cosmos_llm_example::Example;
use cosmos_llm_registry::{meta, Meta, Plugin};
use cosmos_llm_signature::Signature;

use crate::error::PredictError;
use crate::model::{CallOptions, LanguageModel};
use crate::module::Module;
use crate::predictor::Predict;

/// Everything a strategy may need to build its module.
#[derive(Clone)]
pub struct Parts {
    /// The signature to serve.
    pub signature: Signature,
    /// The model to call.
    pub model: Arc<dyn LanguageModel>,
    /// The adapter, or `None` for the default.
    pub adapter: Option<Arc<dyn Adapter>>,
    /// The cache, if any.
    pub cache: Option<Arc<dyn CacheStore>>,
    /// Demonstrations.
    pub demos: Vec<Example>,
    /// Default call options.
    pub options: CallOptions,
}

impl Parts {
    /// Parts with only the required collaborators set.
    pub fn new(signature: Signature, model: Arc<dyn LanguageModel>) -> Self {
        Self {
            signature,
            model,
            adapter: None,
            cache: None,
            demos: Vec::new(),
            options: CallOptions::default(),
        }
    }

    /// Applies the optional parts to a predictor.
    pub fn configure(&self, mut predict: Predict) -> Predict {
        if let Some(adapter) = &self.adapter {
            predict = predict.adapter(Arc::clone(adapter));
        }
        if let Some(cache) = &self.cache {
            predict = predict.cache(Arc::clone(cache));
        }
        predict
            .demos(self.demos.clone())
            .options(self.options.clone())
    }
}

/// A strategy registration.
pub struct StrategyPlugin {
    /// The strategy's name and aliases.
    pub meta: Meta,
    /// Builds the module.
    pub build: fn(Parts) -> Result<Box<dyn Module>, PredictError>,
}

impl Plugin for StrategyPlugin {
    fn meta(&self) -> &Meta {
        &self.meta
    }
}

cosmos_llm_registry::inventory::collect!(StrategyPlugin);

/// Builds the strategy registered under `name`.
///
/// # Errors
///
/// Returns [`PredictError::NotFound`] for an unregistered name, and whatever
/// the strategy's builder returns.
pub fn strategy(name: &str, parts: Parts) -> Result<Box<dyn Module>, PredictError> {
    let plugin = cosmos_llm_registry::require::<StrategyPlugin>("strategy", name)?;
    (plugin.build)(parts)
}

fn build_predict(parts: Parts) -> Result<Box<dyn Module>, PredictError> {
    let predict = Predict::new(parts.signature.clone(), Arc::clone(&parts.model));
    Ok(Box::new(parts.configure(predict)))
}

fn build_chain_of_thought(parts: Parts) -> Result<Box<dyn Module>, PredictError> {
    let predict = Predict::chain_of_thought(parts.signature.clone(), Arc::clone(&parts.model))?;
    Ok(Box::new(parts.configure(predict)))
}

cosmos_llm_registry::inventory::submit! {
    StrategyPlugin {
        meta: meta!("predict").builtin().summary("One call: render, ask, parse"),
        build: build_predict,
    }
}

cosmos_llm_registry::inventory::submit! {
    StrategyPlugin {
        meta: meta!("chain_of_thought")
            .builtin()
            .aliases(&["cot"])
            .summary("Adds a reasoning output ahead of the answer"),
        build: build_chain_of_thought,
    }
}
