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

pub mod definition;
pub mod error;
pub mod executor;
pub mod parameter;
pub mod preset;
pub mod registry;
pub mod schemas;

pub use definition::ToolDefinition;
pub use error::ToolError;
pub use executor::Executor;
pub use parameter::ParameterType;
pub use registry::Registry;
