//! # cosmos-llm-signature
//!
//! Declarative descriptions of LLM calls: named typed inputs, named typed
//! outputs, and the instructions that connect them.
//!
//! Mirrors the Ruby `cosmos-llm-signature` gem. A [`Signature`] is data, not
//! behaviour; turning one into a prompt is an adapter's job, and calling a
//! model with it is a predictor's. Keeping the description separate is what
//! lets one signature work across providers and prompting strategies.
//!
//! ## Quick start
//!
//! ```
//! use cosmos_llm_signature::Signature;
//! use serde_json::json;
//!
//! let sig = Signature::parse("document -> summary, tags: array[string]").unwrap();
//!
//! // What a provider with structured output receives.
//! let schema = sig.output_json_schema().unwrap();
//! assert_eq!(schema["properties"]["tags"]["items"]["type"], "string");
//!
//! // What a model's raw reply becomes.
//! let out = sig
//!     .coerce_outputs(&json!({ "summary": "Short.", "tags": "[\"rust\", \"llm\"]" }))
//!     .unwrap();
//! assert_eq!(out["tags"], json!(["rust", "llm"]));
//! ```
//!
//! ## Swapping pieces
//!
//! Two registries live here, both built on [`cosmos_llm_registry`]:
//!
//! - [`codec::CodecPlugin`] maps type names to [`FieldCodec`]s. Register a
//!   codec under a new name to add a type usable in shorthand strings; under a
//!   built-in name (`string`, `integer`, `number`, `boolean`, `array`,
//!   `object`) to replace it.
//! - [`SignaturePlugin`] maps names to signatures, so CLIs and config files
//!   can say `triage` instead of spelling the fields out.
//!
//! ## Command line
//!
//! With the `cli` feature this crate builds a `cosmos-llm-signature` binary
//! for inspecting signatures, printing their schemas, and coercing JSON
//! through them. [`cli::main`] is public, so a binary that links your own
//! plugins can reuse the whole command line with one line of code.

#![warn(missing_docs)]

pub mod codec;
mod error;
mod field;
pub mod parser;
mod signature;

#[cfg(feature = "cli")]
pub mod cli;

pub use codec::FieldCodec;
pub use error::SignatureError;
pub use field::{Field, Kind};
pub use signature::{Signature, SignatureBuilder, SignaturePlugin};

/// The registry crate, re-exported so plugins can register through it.
pub use cosmos_llm_registry as registry;
