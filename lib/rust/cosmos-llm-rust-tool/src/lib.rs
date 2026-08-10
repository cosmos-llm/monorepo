//! # cosmos-llm-tool
//!
//! Function-calling layer and preset tools for LLM agents.
//!
//! Mirrors the Ruby `cosmos-llm-ruby-tool` and `cosmos-llm-ruby-tool-preset`
//! gems. Define tools with typed parameters, register them in a [`Registry`],
//! and execute them via the [`Executor`].
//!
//! ## Quick start
//!
//! ```rust
//! use cosmos_llm_tool::{ToolDefinition, ParameterType, Registry, Executor};
//! use serde_json::json;
//!
//! let tool = ToolDefinition::new("greet")
//!     .description("Greets a user by name")
//!     .param("name", ParameterType::String, true, "The user's name")
//!     .handler(|params| {
//!         let name = params["name"].as_str().unwrap_or("world");
//!         Ok(json!(format!("Hello, {name}!")))
//!     });
//!
//! let result = Executor::execute(&tool, &json!({ "name": "Alice" })).unwrap();
//! assert_eq!(result, json!("Hello, Alice!"));
//! ```
//!
//! ## Async tools
//!
//! Most real tools are I/O-bound. Give those an
//! [`async_handler`](ToolDefinition::async_handler), an optional
//! [`timeout`](ToolDefinition::timeout), and run them with
//! [`Executor::execute_async`] — or a whole batch concurrently with
//! [`Executor::execute_all`], which is what an agent loop wants when a model
//! requests several tools at once.
//!
//! ```rust
//! # #[cfg(feature = "async")] {
//! use std::time::Duration;
//! use cosmos_llm_tool::{Executor, ParameterType, ToolDefinition};
//! use serde_json::json;
//!
//! # tokio_test::block_on(async {
//! let fetch = ToolDefinition::new("fetch")
//!     .param("id", ParameterType::Integer, true, "Record to fetch")
//!     .timeout(Duration::from_secs(5))
//!     .async_handler(|params| Box::pin(async move {
//!         // Real work would await a network call here.
//!         Ok(json!({ "id": params["id"], "found": true }))
//!     }));
//!
//! let results = Executor::execute_all(
//!     vec![(&fetch, json!({"id": 1})), (&fetch, json!({"id": 2}))],
//!     4,
//! )
//! .await;
//!
//! assert_eq!(results.len(), 2);
//! assert_eq!(results[0].as_ref().unwrap()["id"], 1);
//! # })
//! # }
//! ```
//!
//! Synchronous tools stay synchronous — parsing and arithmetic gain nothing
//! from a future — and both kinds can share one registry and one batch.
//!
//! The `async` feature is on by default; turning it off leaves the sync half
//! intact with no `futures` or runtime dependency.

pub mod definition;
pub mod error;
pub mod executor;
pub mod parameter;
pub mod preset;
pub mod registry;
pub mod schemas;

pub use definition::{Handler, ToolDefinition};
pub use error::ToolError;
pub use executor::Executor;
pub use parameter::ParameterType;
pub use registry::Registry;
