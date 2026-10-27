//! # cosmos-llm-predict
//!
//! Signature-driven LLM calls.
//!
//! Mirrors the predictor half of the Ruby `cosmos-llm-predict` gem. A
//! [`Predict`] takes a [`Signature`](cosmos_llm_signature::Signature) and
//! makes one model call that satisfies it: an adapter renders the prompt, a
//! [`LanguageModel`] answers, and the adapter parses the reply into typed
//! values. [`Predict::chain_of_thought`] adds a reasoning field ahead of the
//! answer, and [`BestOfN`] samples any [`Module`] several times and keeps the
//! best draw.
//!
//! ## Quick start
//!
//! ```no_run
//! # #[cfg(feature = "client")] {
//! use std::sync::Arc;
//! use cosmos_llm::Client;
//! use cosmos_llm_predict::Predict;
//! use cosmos_llm_signature::Signature;
//! use serde_json::json;
//!
//! # tokio_test::block_on(async {
//! let model = Client::new("anthropic", std::env::var("ANTHROPIC_API_KEY").unwrap())
//!     .unwrap()
//!     .with_model("claude-sonnet-5-5");
//!
//! let triage = Predict::new(
//!     Signature::parse("ticket -> severity: int, team").unwrap(),
//!     Arc::new(model),
//! );
//!
//! let prediction = triage.call(&json!({ "ticket": "Checkout returns 500 for every user" })).await.unwrap();
//! println!("{} -> {}", prediction.values["severity"], prediction.values["team"]);
//! # })
//! # }
//! ```
//!
//! ## Swapping pieces
//!
//! Every collaborator is a trait object:
//!
//! | Piece | Trait | Registry |
//! |---|---|---|
//! | The model | [`LanguageModel`] | [`BackendPlugin`] (`cosmos` built in) |
//! | Prompt format | `cosmos_llm_adapter::Adapter` | `AdapterPlugin` (`chat`, `json`, `json_schema`) |
//! | Reply cache | `cosmos_llm_cache::CacheStore` | `CachePlugin` (`memory`, `disk`, `tiered`) |
//! | Strategy | [`Module`] | [`StrategyPlugin`] (`predict`, `chain_of_thought`) |
//!
//! In code, pass your implementation to the builder. By name — from the
//! CLI or a config file — register it with `inventory::submit!` through the
//! owning crate's re-exported `registry`. A registration under a built-in's
//! name replaces the built-in.
//!
//! With the default `client` feature off, this crate does not depend on
//! `cosmos-llm` at all, and the model is whatever [`LanguageModel`] you
//! supply.

#![warn(missing_docs)]

mod best_of_n;
mod error;
mod model;
mod module;
mod prediction;
mod predictor;
pub mod strategy;

#[cfg(feature = "cli")]
pub mod cli;

pub use best_of_n::{BestOfN, Reward, DEFAULT_TEMPERATURE};
pub use error::{BoxError, PredictError};
pub use model::{
    backend, BackendOptions, BackendPlugin, CallOptions, Completion, LanguageModel, Usage,
};
pub use module::Module;
pub use prediction::Prediction;
pub use predictor::{
    Predict, DEFAULT_ADAPTER, DEFAULT_MAX_PARSE_RETRIES, REASONING_DESC, REASONING_FIELD,
};
pub use strategy::{strategy, Parts, StrategyPlugin};

/// The registry crate, re-exported so plugins can register through it.
pub use cosmos_llm_registry as registry;
