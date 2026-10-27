//! # cosmos-llm-adapter
//!
//! Prompting strategies for signatures: render a
//! [`Signature`](cosmos_llm_signature::Signature) and its inputs into chat
//! messages, and parse the model's reply back into typed outputs.
//!
//! Mirrors the adapters of the Ruby `cosmos-llm-predict` gem. This crate
//! makes no network calls and knows nothing about providers; it produces
//! [`Message`]s and request parameters, and consumes reply text. That makes
//! every strategy testable offline and usable with any client.
//!
//! ## Quick start
//!
//! ```
//! use cosmos_llm_adapter::{adapter, Adapter};
//! use cosmos_llm_signature::Signature;
//! use serde_json::{json, Map, Value};
//!
//! let sig = Signature::parse("question -> answer: int").unwrap();
//! let chat = adapter("chat").unwrap();
//!
//! let inputs: Map<String, Value> = json!({ "question": "What is 2+2?" })
//!     .as_object()
//!     .unwrap()
//!     .clone();
//! let messages = chat.format(&sig, &inputs, &[]).unwrap();
//! assert_eq!(messages.len(), 2);
//! assert!(messages[1].content.contains("[[ ## question ## ]]"));
//!
//! // ...send the messages with any client, then:
//! let out = chat.parse(&sig, "[[ ## answer ## ]]\n4\n[[ ## done ## ]]").unwrap();
//! assert_eq!(out["answer"], json!(4));
//! ```
//!
//! ## Built-in adapters
//!
//! | Name          | Layout                                                    |
//! |---------------|-----------------------------------------------------------|
//! | `chat`        | each field under a `[[ ## name ## ]]` header              |
//! | `json`        | one JSON object, requested in the prompt                  |
//! | `json_schema` | one JSON object, also enforced through `response_format`  |
//!
//! ## Swapping an adapter
//!
//! Adapters are found by name through [`AdapterPlugin`] registrations.
//! Register your own [`Adapter`] under a new name to add a strategy, or under
//! a built-in's name to replace it everywhere it is looked up by name —
//! including the CLI. See [`cosmos_llm_registry`] for how ties are settled.
//!
//! ## Command line
//!
//! With the `cli` feature this crate builds a `cosmos-llm-adapter` binary
//! that formats prompts and parses replies, so a prompt can be inspected or a
//! saved reply re-parsed without writing code. [`cli::main`] is public for
//! binaries that link extra plugins.

#![warn(missing_docs)]

mod adapter;
mod chat;
mod error;
mod json;
pub mod json_stream;
mod message;

#[cfg(feature = "cli")]
pub mod cli;

pub use adapter::{
    adapter, describe_field, input_values, output_values, render_value, Adapter, AdapterPlugin,
};
pub use chat::{ChatAdapter, TERMINATOR};
pub use error::AdapterError;
pub use json::{JsonAdapter, DEFAULT_SCHEMA_NAME};
pub use message::Message;

/// The registry crate, re-exported so plugins can register through it.
pub use cosmos_llm_registry as registry;
